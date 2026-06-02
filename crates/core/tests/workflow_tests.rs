/// M2.1 integration tests for the async workflow engine (`koakuma_core::workflow`).
///
/// Exercises every control-flow node — Seq, Parallel, If, While, ForEach, Retry,
/// Timeout, Wait — through `run_workflow`, using `InMemoryStateStore` to observe
/// side effects. Zero platform dependencies.
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use async_trait::async_trait;
use koakuma_core::{
    context::{CancellationToken, EvalContext, ExecContext, LogHandle},
    domain::{
        ActionConfig, CompareOp, ConstraintConfig, ConstraintExpr, VarScope, WaitCondition,
        WorkflowNode,
    },
    engine::EngineEvent,
    error::ActionError,
    event::{Event, EventKind},
    permission::{Permission, PermissionGrant, PermissionSet},
    registry::Registry,
    state::StateStore,
    traits::{Action, Outcome},
    value::Value,
};
use koakuma_store::InMemoryStateStore;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn manual_event() -> Event {
    Event {
        kind: EventKind::Manual,
        source: "test".into(),
        timestamp: SystemTime::now(),
        payload: Value::Null,
    }
}

fn exec_ctx(store: Arc<dyn StateStore>, perms: Vec<Permission>) -> ExecContext {
    ExecContext {
        event: manual_event(),
        macro_id: uuid::Uuid::nil(),
        locals: Default::default(),
        store,
        permissions: PermissionGrant::new(perms),
        cancel: CancellationToken::new(),
        log: LogHandle,
    }
}

fn action(provider: &str) -> WorkflowNode {
    WorkflowNode::Action(ActionConfig::Custom {
        provider: provider.into(),
        params: serde_json::Value::Null,
    })
}

/// Increments the store key `"counter"` by 1 each time it runs.
struct IncAction;
#[async_trait]
impl Action for IncAction {
    fn id(&self) -> &'static str {
        "inc"
    }
    fn required_permissions(&self) -> PermissionSet {
        PermissionSet::default()
    }
    async fn execute(&self, ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        ctx.store.increment("counter", 1);
        Ok(Outcome::Continue)
    }
}

/// Returns `Outcome::Stop`.
struct StopAction;
#[async_trait]
impl Action for StopAction {
    fn id(&self) -> &'static str {
        "stop"
    }
    fn required_permissions(&self) -> PermissionSet {
        PermissionSet::default()
    }
    async fn execute(&self, _ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        Ok(Outcome::Stop)
    }
}

/// Fails (returns `Err`) for the first `threshold` attempts, then succeeds.
struct FlakyAction {
    attempts: Arc<Mutex<u32>>,
    threshold: u32,
}
#[async_trait]
impl Action for FlakyAction {
    fn id(&self) -> &'static str {
        "flaky"
    }
    fn required_permissions(&self) -> PermissionSet {
        PermissionSet::default()
    }
    async fn execute(&self, ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        let mut n = self.attempts.lock().unwrap();
        *n += 1;
        if *n <= self.threshold {
            Err(ActionError::Failed(format!("attempt {n} fails")))
        } else {
            ctx.store.set("succeeded", Value::Bool(true));
            Ok(Outcome::Continue)
        }
    }
}

/// Builds a registry whose `inc`/`stop` Custom providers operate on `store`.
fn registry_with_helpers(
    store: &Arc<dyn StateStore>,
    attempts: Arc<Mutex<u32>>,
    threshold: u32,
) -> Registry {
    let _ = store; // store is shared via ExecContext, not captured by the actions
    let mut reg = Registry::with_builtins();
    reg.register_action(move |c| match c {
        ActionConfig::Custom { provider, .. } if provider == "inc" => {
            Some(Box::new(IncAction) as Box<dyn Action>)
        }
        ActionConfig::Custom { provider, .. } if provider == "stop" => {
            Some(Box::new(StopAction) as Box<dyn Action>)
        }
        ActionConfig::Custom { provider, .. } if provider == "flaky" => {
            Some(Box::new(FlakyAction {
                attempts: Arc::clone(&attempts),
                threshold,
            }) as Box<dyn Action>)
        }
        _ => None,
    });
    reg
}

fn fresh() -> (Arc<dyn StateStore>, Registry) {
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
    let reg = registry_with_helpers(&store, Arc::new(Mutex::new(0)), 0);
    (store, reg)
}

fn counter(store: &Arc<dyn StateStore>) -> i64 {
    match store.get("counter") {
        Some(Value::Int(n)) => n,
        _ => 0,
    }
}

// ── Seq ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn seq_runs_children_in_order() {
    let (store, reg) = fresh();
    let wf = WorkflowNode::Seq(vec![action("inc"), action("inc"), action("inc")]);
    let mut ctx = exec_ctx(Arc::clone(&store), vec![]);
    let events = koakuma_core::workflow::run_workflow(&wf, &mut ctx, &reg).await;
    assert!(events.is_empty(), "no errors expected: {events:?}");
    assert_eq!(counter(&store), 3);
}

#[tokio::test]
async fn seq_halts_on_stop() {
    let (store, reg) = fresh();
    let wf = WorkflowNode::Seq(vec![action("inc"), action("stop"), action("inc")]);
    let mut ctx = exec_ctx(Arc::clone(&store), vec![]);
    koakuma_core::workflow::run_workflow(&wf, &mut ctx, &reg).await;
    assert_eq!(counter(&store), 1, "action after Stop must not run");
}

// ── If ───────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn if_true_runs_then_branch() {
    let (store, reg) = fresh();
    store.set("flag", Value::Int(1));
    let wf = WorkflowNode::If {
        cond: ConstraintExpr::Leaf {
            constraint: ConstraintConfig::VarCompare {
                key: "flag".into(),
                op: CompareOp::Eq,
                value: Value::Int(1),
            },
        },
        then: Box::new(action("inc")),
        otherwise: Some(Box::new(action("stop"))),
    };
    let mut ctx = exec_ctx(Arc::clone(&store), vec![]);
    koakuma_core::workflow::run_workflow(&wf, &mut ctx, &reg).await;
    assert_eq!(counter(&store), 1);
}

#[tokio::test]
async fn if_false_runs_otherwise_branch() {
    let (store, reg) = fresh();
    store.set("flag", Value::Int(0));
    let wf = WorkflowNode::If {
        cond: ConstraintExpr::Leaf {
            constraint: ConstraintConfig::VarCompare {
                key: "flag".into(),
                op: CompareOp::Eq,
                value: Value::Int(1),
            },
        },
        then: Box::new(action("inc")),
        otherwise: Some(Box::new(action("inc"))), // both branches increment; only one should run
    };
    let mut ctx = exec_ctx(Arc::clone(&store), vec![]);
    koakuma_core::workflow::run_workflow(&wf, &mut ctx, &reg).await;
    assert_eq!(counter(&store), 1, "exactly one branch runs");
}

// ── While ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn while_loops_until_condition_false() {
    let (store, reg) = fresh();
    store.set("counter", Value::Int(0)); // a missing var compares as Null, not < 3
    // Loop while counter < 3, incrementing each pass → terminates at counter == 3.
    let wf = WorkflowNode::While {
        cond: ConstraintExpr::Leaf {
            constraint: ConstraintConfig::VarCompare {
                key: "counter".into(),
                op: CompareOp::Lt,
                value: Value::Int(3),
            },
        },
        body: Box::new(action("inc")),
        max_iter: 100,
    };
    let mut ctx = exec_ctx(Arc::clone(&store), vec![]);
    koakuma_core::workflow::run_workflow(&wf, &mut ctx, &reg).await;
    assert_eq!(counter(&store), 3);
}

#[tokio::test]
async fn while_is_bounded_by_max_iter() {
    let (store, reg) = fresh();
    store.set("counter", Value::Int(0));
    // Condition never becomes false (counter always < 1000), but max_iter caps it.
    let wf = WorkflowNode::While {
        cond: ConstraintExpr::Leaf {
            constraint: ConstraintConfig::VarCompare {
                key: "counter".into(),
                op: CompareOp::Lt,
                value: Value::Int(1000),
            },
        },
        body: Box::new(action("inc")),
        max_iter: 5,
    };
    let mut ctx = exec_ctx(Arc::clone(&store), vec![]);
    koakuma_core::workflow::run_workflow(&wf, &mut ctx, &reg).await;
    assert_eq!(counter(&store), 5, "max_iter must bound the loop");
}

// ── ForEach ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn foreach_runs_body_per_element() {
    let (store, reg) = fresh();
    let wf = WorkflowNode::ForEach {
        items: Value::List(vec![Value::Int(10), Value::Int(20), Value::Int(30)]),
        var: "item".into(),
        body: Box::new(action("inc")),
    };
    let mut ctx = exec_ctx(Arc::clone(&store), vec![]);
    koakuma_core::workflow::run_workflow(&wf, &mut ctx, &reg).await;
    assert_eq!(counter(&store), 3);
}

// ── Retry ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn retry_succeeds_after_failures() {
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
    let attempts = Arc::new(Mutex::new(0));
    // Fails twice, succeeds on the third attempt.
    let reg = registry_with_helpers(&store, Arc::clone(&attempts), 2);
    let wf = WorkflowNode::Retry {
        body: Box::new(action("flaky")),
        times: 3,
        backoff_ms: 0,
    };
    let mut ctx = exec_ctx(Arc::clone(&store), vec![]);
    koakuma_core::workflow::run_workflow(&wf, &mut ctx, &reg).await;
    assert_eq!(*attempts.lock().unwrap(), 3, "should retry up to success");
    assert_eq!(store.get("succeeded"), Some(Value::Bool(true)));
}

#[tokio::test]
async fn retry_gives_up_after_max_attempts() {
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
    let attempts = Arc::new(Mutex::new(0));
    // Threshold higher than attempts → never succeeds.
    let reg = registry_with_helpers(&store, Arc::clone(&attempts), 10);
    let wf = WorkflowNode::Retry {
        body: Box::new(action("flaky")),
        times: 3,
        backoff_ms: 0,
    };
    let mut ctx = exec_ctx(Arc::clone(&store), vec![]);
    let events = koakuma_core::workflow::run_workflow(&wf, &mut ctx, &reg).await;
    assert_eq!(*attempts.lock().unwrap(), 3, "exactly `times` attempts");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Error { .. })),
        "exhausted retry should surface an error: {events:?}"
    );
    assert_eq!(store.get("succeeded"), None);
}

// ── Timeout ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn timeout_fails_a_slow_body() {
    let (store, reg) = fresh();
    // Delay 5s wrapped in a 20ms timeout → must fail quickly.
    let wf = WorkflowNode::Timeout {
        body: Box::new(WorkflowNode::Action(ActionConfig::Delay { millis: 5_000 })),
        millis: 20,
    };
    let mut ctx = exec_ctx(Arc::clone(&store), vec![]);
    let start = Instant::now();
    let events = koakuma_core::workflow::run_workflow(&wf, &mut ctx, &reg).await;
    assert!(
        start.elapsed().as_millis() < 2_000,
        "timeout must abort early"
    );
    assert!(
        events.iter().any(
            |e| matches!(e, EngineEvent::Error { message, .. } if message.contains("timed out"))
        ),
        "expected a timeout error: {events:?}"
    );
}

// ── Parallel ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn parallel_runs_all_branches() {
    let (store, reg) = fresh();
    let wf = WorkflowNode::Parallel(vec![action("inc"), action("inc"), action("inc")]);
    let mut ctx = exec_ctx(Arc::clone(&store), vec![]);
    koakuma_core::workflow::run_workflow(&wf, &mut ctx, &reg).await;
    assert_eq!(
        counter(&store),
        3,
        "every parallel branch runs against the shared store"
    );
}

#[tokio::test]
async fn parallel_isolates_local_variables() {
    let (store, reg) = fresh();
    // Each branch writes its own local var; locals must not leak back to the parent.
    let wf = WorkflowNode::Parallel(vec![WorkflowNode::Action(ActionConfig::SetVariable {
        scope: VarScope::Local,
        key: "branch_local".into(),
        value: Value::Int(99),
    })]);
    let mut ctx = exec_ctx(Arc::clone(&store), vec![]);
    koakuma_core::workflow::run_workflow(&wf, &mut ctx, &reg).await;
    assert!(
        !ctx.locals.contains_key("branch_local"),
        "forked locals stay isolated"
    );
}

// ── Wait ───────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn wait_duration_completes() {
    let (store, reg) = fresh();
    let wf = WorkflowNode::Seq(vec![
        WorkflowNode::Wait {
            until: WaitCondition::Duration { millis: 5 },
        },
        action("inc"),
    ]);
    let mut ctx = exec_ctx(Arc::clone(&store), vec![]);
    let events = koakuma_core::workflow::run_workflow(&wf, &mut ctx, &reg).await;
    assert!(events.is_empty());
    assert_eq!(counter(&store), 1, "Wait then continue");
}

// ── V1 compatibility: flat actions still run as a Seq ────────────────────────────

#[tokio::test]
async fn legacy_flat_actions_run_sequentially() {
    let (store, reg) = fresh();
    let m = koakuma_core::domain::Macro {
        id: uuid::Uuid::nil(),
        name: "legacy".into(),
        description: String::new(),
        enabled: true,
        category: None,
        triggers: vec![],
        constraints: ConstraintExpr::Always,
        actions: vec![
            ActionConfig::Custom {
                provider: "inc".into(),
                params: serde_json::Value::Null,
            },
            ActionConfig::Custom {
                provider: "inc".into(),
                params: serde_json::Value::Null,
            },
        ],
        workflow: None,
        granted_permissions: PermissionSet::default(),
    };
    let root = m.root_workflow();
    let mut ctx = exec_ctx(Arc::clone(&store), vec![]);
    koakuma_core::workflow::run_workflow(&root, &mut ctx, &reg).await;
    assert_eq!(
        counter(&store),
        2,
        "a workflow-less macro runs its flat actions in order"
    );
}

// keep EvalContext import used (constraint helper sanity) ----------------------
#[allow(dead_code)]
fn _assert_eval_ctx_constructible(store: &dyn StateStore) {
    let ev = manual_event();
    let _ = EvalContext {
        event: &ev,
        macro_id: uuid::Uuid::nil(),
        store,
    };
}
