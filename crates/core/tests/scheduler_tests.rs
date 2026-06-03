//! M2.2 integration tests for [`WorkflowScheduler`].
//!
//! Exercises all six [`ConcurrencyPolicy`] variants and shared-resource
//! arbitration. No platform dependencies — all workflows use in-process
//! synthetic actions that record execution counts or timing.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI32, AtomicU32, Ordering},
};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use koakuma_core::{
    context::{CancellationToken, ExecContext, LogHandle, ResourcePool},
    domain::{ActionConfig, ConcurrencyPolicy, WorkflowNode},
    error::ActionError,
    event::{Event, EventKind},
    permission::{PermissionGrant, PermissionSet},
    registry::Registry,
    scheduler::{RESOURCE_INPUT, WorkflowScheduler},
    state::StateStore,
    traits::{Action, Outcome},
    value::Value,
};
use koakuma_store::InMemoryStateStore;

// ── Shared helpers ────────────────────────────────────────────────────────────

fn manual_event() -> Event {
    Event {
        kind: EventKind::Manual,
        source: "sched-test".into(),
        timestamp: SystemTime::now(),
        payload: Value::Null,
    }
}

fn make_store() -> Arc<dyn StateStore> {
    Arc::new(InMemoryStateStore::new())
}

fn make_ctx(store: &Arc<dyn StateStore>, pool: ResourcePool) -> ExecContext {
    ExecContext {
        event: manual_event(),
        macro_id: uuid::Uuid::nil(),
        locals: Default::default(),
        store: Arc::clone(store),
        permissions: PermissionGrant::new(vec![]),
        cancel: CancellationToken::new(),
        log: LogHandle,
        resource_pool: pool,
    }
}

fn new_scheduler() -> (WorkflowScheduler, crossbeam_channel::Receiver<koakuma_core::engine::EngineEvent>) {
    let (tx, rx) = crossbeam_channel::unbounded();
    let sched = WorkflowScheduler::new(tokio::runtime::Handle::current(), tx);
    (sched, rx)
}

// ── Actions ───────────────────────────────────────────────────────────────────

/// Increments `count` and returns immediately.
struct CountAction(Arc<AtomicU32>);

#[async_trait]
impl Action for CountAction {
    fn id(&self) -> &'static str { "count" }
    fn required_permissions(&self) -> PermissionSet { Default::default() }
    async fn execute(&self, ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        if !ctx.cancel.is_cancelled() {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        Ok(Outcome::Continue)
    }
}

/// Sleeps for `delay_ms`, then increments `count` if not cancelled.
struct SlowAction {
    count: Arc<AtomicU32>,
    delay_ms: u64,
}

#[async_trait]
impl Action for SlowAction {
    fn id(&self) -> &'static str { "slow" }
    fn required_permissions(&self) -> PermissionSet { Default::default() }
    async fn execute(&self, ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        if !ctx.cancel.is_cancelled() {
            self.count.fetch_add(1, Ordering::SeqCst);
        }
        Ok(Outcome::Continue)
    }
}

/// Records concurrent executions to detect interleaving; uses RESOURCE_INPUT.
struct ResourceAction {
    concurrent: Arc<AtomicI32>,
    max_concurrent: Arc<AtomicI32>,
}

#[async_trait]
impl Action for ResourceAction {
    fn id(&self) -> &'static str { "resource" }
    fn required_permissions(&self) -> PermissionSet { Default::default() }
    fn resources(&self) -> Vec<String> { vec![RESOURCE_INPUT.to_string()] }
    async fn execute(&self, _ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        let c = self.concurrent.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_concurrent.fetch_max(c, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        self.concurrent.fetch_sub(1, Ordering::SeqCst);
        Ok(Outcome::Continue)
    }
}

fn make_registry_with<F>(factory: F) -> Arc<Registry>
where
    F: Fn(&ActionConfig) -> Option<Box<dyn Action>> + Send + Sync + 'static,
{
    let mut reg = Registry::with_builtins();
    reg.register_action(factory);
    Arc::new(reg)
}

// ── Parallel ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn parallel_runs_all_triggers_concurrently() {
    let count = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&count);
    let reg = make_registry_with(move |cfg| {
        if let ActionConfig::Custom { provider, .. } = cfg {
            if provider == "count" {
                let c = Arc::clone(&c);
                return Some(Box::new(CountAction(c)) as Box<dyn Action>);
            }
        }
        None
    });

    let (sched, _rx) = new_scheduler();
    let store = make_store();
    let macro_id = uuid::Uuid::new_v4();
    let wf = WorkflowNode::Action(ActionConfig::Custom {
        provider: "count".into(),
        params: serde_json::Value::Null,
    });

    for _ in 0..3 {
        sched.schedule(
            macro_id,
            &ConcurrencyPolicy::Parallel,
            make_ctx(&store, sched.resource_pool().clone()),
            wf.clone(),
            Arc::clone(&reg),
        );
    }

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(count.load(Ordering::SeqCst), 3, "all 3 parallel triggers must execute");
}

// ── DropIfRunning ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn drop_if_running_discards_second_trigger() {
    let count = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&count);
    let reg = make_registry_with(move |cfg| {
        if let ActionConfig::Custom { provider, .. } = cfg {
            if provider == "slow" {
                let c = Arc::clone(&c);
                return Some(Box::new(SlowAction { count: c, delay_ms: 60 }) as Box<dyn Action>);
            }
        }
        None
    });

    let (sched, _rx) = new_scheduler();
    let store = make_store();
    let macro_id = uuid::Uuid::new_v4();
    let wf = WorkflowNode::Action(ActionConfig::Custom {
        provider: "slow".into(),
        params: serde_json::Value::Null,
    });

    // Schedule twice; the second must be dropped because the first is running.
    sched.schedule(
        macro_id,
        &ConcurrencyPolicy::DropIfRunning,
        make_ctx(&store, sched.resource_pool().clone()),
        wf.clone(),
        Arc::clone(&reg),
    );
    sched.schedule(
        macro_id,
        &ConcurrencyPolicy::DropIfRunning,
        make_ctx(&store, sched.resource_pool().clone()),
        wf.clone(),
        Arc::clone(&reg),
    );

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(count.load(Ordering::SeqCst), 1, "second trigger must be dropped");
}

// ── Queue ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn queue_serialises_all_triggers() {
    let order = Arc::new(Mutex::new(Vec::<u32>::new()));
    let seq = Arc::new(AtomicU32::new(0));
    let o = Arc::clone(&order);
    let s = Arc::clone(&seq);
    let reg = make_registry_with(move |cfg| {
        if let ActionConfig::Custom { provider, .. } = cfg {
            if provider == "ordered" {
                let o = Arc::clone(&o);
                let s = Arc::clone(&s);
                return Some(Box::new(OrderedAction { order: o, seq: s }) as Box<dyn Action>);
            }
        }
        None
    });

    let (sched, _rx) = new_scheduler();
    let store = make_store();
    let macro_id = uuid::Uuid::new_v4();
    let wf = WorkflowNode::Action(ActionConfig::Custom {
        provider: "ordered".into(),
        params: serde_json::Value::Null,
    });

    for _ in 0..3 {
        sched.schedule(
            macro_id,
            &ConcurrencyPolicy::Queue { max: 4 },
            make_ctx(&store, sched.resource_pool().clone()),
            wf.clone(),
            Arc::clone(&reg),
        );
    }

    tokio::time::sleep(Duration::from_millis(500)).await;
    let o = order.lock().unwrap();
    assert_eq!(o.len(), 3, "all 3 queue items must execute");
    assert_eq!(*o, vec![0, 1, 2], "queue must run in order");
}

struct OrderedAction {
    order: Arc<Mutex<Vec<u32>>>,
    seq: Arc<AtomicU32>,
}

#[async_trait]
impl Action for OrderedAction {
    fn id(&self) -> &'static str { "ordered" }
    fn required_permissions(&self) -> PermissionSet { Default::default() }
    async fn execute(&self, _ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        let n = self.seq.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        self.order.lock().unwrap().push(n);
        Ok(Outcome::Continue)
    }
}

// ── Queue max exceeded — extras are dropped ───────────────────────────────────

#[tokio::test]
async fn queue_drops_when_max_exceeded() {
    let count = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&count);
    let reg = make_registry_with(move |cfg| {
        if let ActionConfig::Custom { provider, .. } = cfg {
            if provider == "slow" {
                let c = Arc::clone(&c);
                return Some(Box::new(SlowAction { count: c, delay_ms: 40 }) as Box<dyn Action>);
            }
        }
        None
    });

    let (sched, _rx) = new_scheduler();
    let store = make_store();
    let macro_id = uuid::Uuid::new_v4();
    let wf = WorkflowNode::Action(ActionConfig::Custom {
        provider: "slow".into(),
        params: serde_json::Value::Null,
    });

    // Schedule 5 times; queue max is 1 so only 2 total can run (1 running + 1 queued).
    for _ in 0..5 {
        sched.schedule(
            macro_id,
            &ConcurrencyPolicy::Queue { max: 1 },
            make_ctx(&store, sched.resource_pool().clone()),
            wf.clone(),
            Arc::clone(&reg),
        );
    }

    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(count.load(Ordering::SeqCst), 2, "only 1 running + 1 queued should execute");
}

// ── RestartIfRunning ──────────────────────────────────────────────────────────

#[tokio::test]
async fn restart_cancels_old_and_runs_new() {
    let count = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&count);
    let reg = make_registry_with(move |cfg| {
        if let ActionConfig::Custom { provider, .. } = cfg {
            if provider == "slow" {
                let c = Arc::clone(&c);
                return Some(Box::new(SlowAction { count: c, delay_ms: 100 }) as Box<dyn Action>);
            }
        }
        None
    });

    let (sched, _rx) = new_scheduler();
    let store = make_store();
    let macro_id = uuid::Uuid::new_v4();
    let wf = WorkflowNode::Action(ActionConfig::Custom {
        provider: "slow".into(),
        params: serde_json::Value::Null,
    });

    // First trigger starts; immediately restart — only the second should count.
    sched.schedule(
        macro_id,
        &ConcurrencyPolicy::RestartIfRunning,
        make_ctx(&store, sched.resource_pool().clone()),
        wf.clone(),
        Arc::clone(&reg),
    );
    sched.schedule(
        macro_id,
        &ConcurrencyPolicy::RestartIfRunning,
        make_ctx(&store, sched.resource_pool().clone()),
        wf.clone(),
        Arc::clone(&reg),
    );

    tokio::time::sleep(Duration::from_millis(400)).await;
    // The first is cancelled (never increments counter), the second runs fully.
    assert_eq!(count.load(Ordering::SeqCst), 1, "only the restarted workflow should complete");
}

// ── Debounce ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn debounce_fires_only_last_trigger() {
    let count = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&count);
    let reg = make_registry_with(move |cfg| {
        if let ActionConfig::Custom { provider, .. } = cfg {
            if provider == "count" {
                let c = Arc::clone(&c);
                return Some(Box::new(CountAction(c)) as Box<dyn Action>);
            }
        }
        None
    });

    let (sched, _rx) = new_scheduler();
    let store = make_store();
    let macro_id = uuid::Uuid::new_v4();
    let wf = WorkflowNode::Action(ActionConfig::Custom {
        provider: "count".into(),
        params: serde_json::Value::Null,
    });

    // Fire 3 times within 10ms intervals; debounce window is 80ms.
    // Only the last trigger should execute (after 80ms of silence).
    for _ in 0..3 {
        sched.schedule(
            macro_id,
            &ConcurrencyPolicy::Debounce { ms: 80 },
            make_ctx(&store, sched.resource_pool().clone()),
            wf.clone(),
            Arc::clone(&reg),
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Wait for debounce window to expire and action to complete.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(count.load(Ordering::SeqCst), 1, "debounce must fire exactly once after last trigger");
}

// ── Throttle ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn throttle_fires_only_first_trigger_in_window() {
    let count = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&count);
    let reg = make_registry_with(move |cfg| {
        if let ActionConfig::Custom { provider, .. } = cfg {
            if provider == "count" {
                let c = Arc::clone(&c);
                return Some(Box::new(CountAction(c)) as Box<dyn Action>);
            }
        }
        None
    });

    let (sched, _rx) = new_scheduler();
    let store = make_store();
    let macro_id = uuid::Uuid::new_v4();
    let wf = WorkflowNode::Action(ActionConfig::Custom {
        provider: "count".into(),
        params: serde_json::Value::Null,
    });

    // Fire 3 times rapidly within the throttle window.
    for _ in 0..3 {
        sched.schedule(
            macro_id,
            &ConcurrencyPolicy::Throttle { ms: 150 },
            make_ctx(&store, sched.resource_pool().clone()),
            wf.clone(),
            Arc::clone(&reg),
        );
    }

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(count.load(Ordering::SeqCst), 1, "throttle must fire exactly once per window");
}

// ── Resource arbitration ──────────────────────────────────────────────────────

#[tokio::test]
async fn resource_lock_prevents_concurrent_input_access() {
    let concurrent = Arc::new(AtomicI32::new(0));
    let max_concurrent = Arc::new(AtomicI32::new(0));
    let c = Arc::clone(&concurrent);
    let m = Arc::clone(&max_concurrent);

    let reg = make_registry_with(move |cfg| {
        if let ActionConfig::Custom { provider, .. } = cfg {
            if provider == "resource" {
                let c = Arc::clone(&c);
                let m = Arc::clone(&m);
                return Some(Box::new(ResourceAction {
                    concurrent: c,
                    max_concurrent: m,
                }) as Box<dyn Action>);
            }
        }
        None
    });

    let (sched, _rx) = new_scheduler();
    let store = make_store();
    let wf = WorkflowNode::Action(ActionConfig::Custom {
        provider: "resource".into(),
        params: serde_json::Value::Null,
    });

    // Two DIFFERENT macros both needing the input resource — dispatched in parallel.
    for _ in 0..2 {
        sched.schedule(
            uuid::Uuid::new_v4(),  // different IDs → Parallel semantics
            &ConcurrencyPolicy::Parallel,
            make_ctx(&store, sched.resource_pool().clone()),
            wf.clone(),
            Arc::clone(&reg),
        );
    }

    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        max_concurrent.load(Ordering::SeqCst),
        1,
        "resource lock must serialise the two input-using workflows"
    );
}

// ── Priority ordering via dispatch_scheduled ─────────────────────────────────

#[tokio::test]
async fn dispatch_scheduled_fires_higher_priority_macro_first() {
    use koakuma_core::{
        domain::{ConstraintExpr, Macro, TriggerConfig},
        permission::PermissionSet,
        registry::Registry,
        router::EventRouter,
    };

    let order = Arc::new(Mutex::new(Vec::<String>::new()));
    let o1 = Arc::clone(&order);
    let o2 = Arc::clone(&order);

    let mut reg = Registry::with_builtins();
    reg.register_action(move |cfg| {
        if let ActionConfig::Custom { provider, .. } = cfg {
            if provider == "low" {
                let o = Arc::clone(&o1);
                return Some(Box::new(RecordAction { name: "low".into(), order: o }) as Box<dyn Action>);
            }
            if provider == "high" {
                let o = Arc::clone(&o2);
                return Some(Box::new(RecordAction { name: "high".into(), order: o }) as Box<dyn Action>);
            }
        }
        None
    });
    let reg = Arc::new(reg);

    let store: Arc<dyn StateStore> = make_store();
    let (sched, _rx) = new_scheduler();
    let mut router = EventRouter::new();

    let make_m = |priority: i32, provider: &str| Macro {
        id: uuid::Uuid::new_v4(),
        name: provider.to_string(),
        description: String::new(),
        enabled: true,
        category: None,
        triggers: vec![TriggerConfig::Manual],
        constraints: ConstraintExpr::Always,
        actions: vec![ActionConfig::Custom {
            provider: provider.to_string(),
            params: serde_json::Value::Null,
        }],
        workflow: None,
        granted_permissions: PermissionSet::default(),
        priority,
        concurrency: Default::default(),
    };

    router.add_macro(make_m(0, "low"));
    router.add_macro(make_m(10, "high"));

    let event = manual_event();
    router.dispatch_scheduled(&event, &reg, &store, &sched);

    tokio::time::sleep(Duration::from_millis(200)).await;
    let o = order.lock().unwrap();
    assert_eq!(o.len(), 2);
    assert_eq!(o[0], "high", "higher-priority macro must execute first");
    assert_eq!(o[1], "low");
}

struct RecordAction {
    name: String,
    order: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl Action for RecordAction {
    fn id(&self) -> &'static str { "record" }
    fn required_permissions(&self) -> PermissionSet { Default::default() }
    async fn execute(&self, _ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        self.order.lock().unwrap().push(self.name.clone());
        Ok(Outcome::Continue)
    }
}
