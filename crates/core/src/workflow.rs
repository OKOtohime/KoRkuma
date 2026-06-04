//! Asynchronous workflow engine: drives the [`WorkflowNode`] tree (M2.1).
//!
//! The router selects which macros fire (Hook + Constraint legs, synchronous and
//! lock-free); this module executes the Action leg as an async control-flow tree on
//! the engine's Tokio runtime. It is the V2 replacement for the V1 flat action loop.
//!
//! # Control flow
//!
//! [`run_workflow`] recursively interprets each [`WorkflowNode`]:
//!
//! | Node | Behaviour |
//! |------|-----------|
//! | `Action` | Build, permission-gate, then `await` the action. |
//! | `Seq` | Run children in order; halt on the first Stop / failure. |
//! | `Parallel` | Run children concurrently with forked contexts ([`ExecContext::fork`]). |
//! | `If` | Evaluate `cond`; run `then` or `otherwise`. |
//! | `While` | Loop `body` while `cond` holds, bounded by `max_iter`. |
//! | `ForEach` | Bind each list element to a local var and run `body`. |
//! | `Retry` | Re-run `body` up to `times` attempts on failure, with backoff. |
//! | `Timeout` | Fail `body` if it exceeds `millis`. |
//! | `Wait` | Block on a [`WaitCondition`]. |
//!
//! Permission enforcement is centralised in `run_action`: an action whose
//! [`required_permissions`](crate::traits::Action::required_permissions) are not all
//! granted never runs and yields an [`EngineEvent::Error`].

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use futures::future::join_all;

use crate::context::{EvalContext, ExecContext};
use crate::domain::{ConstraintExpr, WaitCondition, WorkflowNode};
use crate::engine::EngineEvent;
use crate::error::ConstraintError;
use crate::registry::Registry;
use crate::traits::Outcome;
use crate::value::Value;

/// Control-flow result of executing a [`WorkflowNode`].
///
/// Internal to the workflow engine; the public [`Action`](crate::traits::Action)
/// surface still uses [`Outcome`]. `Failed` is distinguished from `Stop` so that
/// [`WorkflowNode::Retry`] can re-run only on genuine failures.
///
/// # Examples
///
/// ```rust
/// use korkuma_core::workflow::Flow;
///
/// assert!(matches!(Flow::Continue, Flow::Continue));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Node completed normally; the enclosing sequence continues.
    Continue,
    /// Node requested the enclosing sequence to halt (action returned [`Outcome::Stop`]).
    Stop,
    /// Node failed (build error, permission denial, action error, or timeout).
    Failed,
}

/// Executes a workflow tree against `ctx`, returning the engine events it produced.
///
/// This is the entry point used by the router after a macro's constraints pass. The
/// returned [`EngineEvent`]s (errors only in M2.1; action logs flow through
/// [`LogHandle`](crate::context::LogHandle)) are forwarded to the UI. The terminal
/// [`Flow`] is discarded here because the caller only needs the emitted events.
///
/// # Examples
///
/// ```rust
/// # use std::sync::Arc;
/// # use korkuma_core::context::{ExecContext, CancellationToken, LogHandle, ResourcePool};
/// # use korkuma_core::domain::{WorkflowNode, ActionConfig, VarScope};
/// # use korkuma_core::event::{Event, EventKind};
/// # use korkuma_core::permission::PermissionGrant;
/// # use korkuma_core::registry::Registry;
/// # use korkuma_core::state::StateStore;
/// # use korkuma_core::value::Value;
/// # use korkuma_core::workflow::run_workflow;
/// # use korkuma_store::InMemoryStateStore;
/// # use std::time::SystemTime;
/// # let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
/// # rt.block_on(async {
/// let reg = Registry::with_builtins();
/// let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
/// let mut ctx = ExecContext {
///     event: Event { kind: EventKind::Manual, source: "t".into(), timestamp: SystemTime::now(), payload: Value::Null },
///     macro_id: uuid::Uuid::nil(),
///     locals: Default::default(),
///     store: Arc::clone(&store),
///     permissions: PermissionGrant::new(vec![]),
///     cancel: CancellationToken::new(),
///     log: LogHandle::default(),
///     resource_pool: ResourcePool::default(),
///     dry_run: false,
/// };
/// let wf = WorkflowNode::Action(ActionConfig::SetVariable {
///     scope: VarScope::Global, key: "k".into(), value: Value::Int(7),
/// });
/// let events = run_workflow(&wf, &mut ctx, &reg).await;
/// assert!(events.is_empty());
/// assert_eq!(store.get("k"), Some(Value::Int(7)));
/// # });
/// ```
pub async fn run_workflow(
    root: &WorkflowNode,
    ctx: &mut ExecContext,
    reg: &Registry,
) -> Vec<EngineEvent> {
    let (_flow, events) = run_node(root, ctx, reg).await;
    events
}

/// Boxed-future return type for the recursive node interpreter.
///
/// `async fn` cannot call itself directly; boxing the future breaks the recursion.
/// `+ Send` is required so the M2.2 `WorkflowScheduler` can spawn workflows on
/// the Tokio multi-thread runtime via `tokio::spawn`.
type NodeFuture<'a> = Pin<Box<dyn Future<Output = (Flow, Vec<EngineEvent>)> + Send + 'a>>;

/// Recursively interprets one [`WorkflowNode`].
fn run_node<'a>(
    node: &'a WorkflowNode,
    ctx: &'a mut ExecContext,
    reg: &'a Registry,
) -> NodeFuture<'a> {
    Box::pin(async move {
        // Cooperative cancellation: bail out before doing any more work.
        if ctx.cancel.is_cancelled() {
            return (Flow::Stop, Vec::new());
        }

        match node {
            WorkflowNode::Action(cfg) => run_action(cfg, ctx, reg).await,

            WorkflowNode::Seq(nodes) => {
                let mut events = Vec::new();
                for n in nodes {
                    let (flow, evs) = run_node(n, ctx, reg).await;
                    events.extend(evs);
                    match flow {
                        Flow::Continue => {}
                        Flow::Stop => return (Flow::Stop, events),
                        Flow::Failed => return (Flow::Failed, events),
                    }
                }
                (Flow::Continue, events)
            }

            WorkflowNode::Parallel(nodes) => {
                // Each branch gets an isolated forked context (shared global store).
                let mut forks: Vec<ExecContext> = nodes.iter().map(|_| ctx.fork()).collect();
                let futs: Vec<NodeFuture> = nodes
                    .iter()
                    .zip(forks.iter_mut())
                    .map(|(n, fork)| run_node(n, fork, reg))
                    .collect();
                let results = join_all(futs).await;

                let mut events = Vec::new();
                let mut outcome = Flow::Continue;
                for (flow, evs) in results {
                    events.extend(evs);
                    match flow {
                        Flow::Failed => outcome = Flow::Failed,
                        Flow::Stop if outcome != Flow::Failed => outcome = Flow::Stop,
                        _ => {}
                    }
                }
                (outcome, events)
            }

            WorkflowNode::If {
                cond,
                then,
                otherwise,
            } => {
                let mut events = Vec::new();
                match eval_condition(cond, ctx, reg) {
                    Ok(true) => {
                        let (flow, evs) = run_node(then, ctx, reg).await;
                        events.extend(evs);
                        (flow, events)
                    }
                    Ok(false) => match otherwise {
                        Some(else_node) => {
                            let (flow, evs) = run_node(else_node, ctx, reg).await;
                            events.extend(evs);
                            (flow, events)
                        }
                        None => (Flow::Continue, events),
                    },
                    Err(e) => {
                        events.push(error_event(ctx, format!("if condition failed: {e}")));
                        (Flow::Failed, events)
                    }
                }
            }

            WorkflowNode::While {
                cond,
                body,
                max_iter,
            } => {
                let mut events = Vec::new();
                let mut iter = 0u32;
                while iter < *max_iter {
                    if ctx.cancel.is_cancelled() {
                        break;
                    }
                    match eval_condition(cond, ctx, reg) {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(e) => {
                            events.push(error_event(ctx, format!("while condition failed: {e}")));
                            return (Flow::Failed, events);
                        }
                    }
                    let (flow, evs) = run_node(body, ctx, reg).await;
                    events.extend(evs);
                    match flow {
                        Flow::Continue => {}
                        Flow::Stop => return (Flow::Stop, events),
                        Flow::Failed => return (Flow::Failed, events),
                    }
                    iter += 1;
                }
                (Flow::Continue, events)
            }

            WorkflowNode::ForEach { items, var, body } => {
                let mut events = Vec::new();
                if let Value::List(list) = items {
                    for item in list {
                        if ctx.cancel.is_cancelled() {
                            break;
                        }
                        ctx.locals.insert(var.clone(), item.clone());
                        let (flow, evs) = run_node(body, ctx, reg).await;
                        events.extend(evs);
                        match flow {
                            Flow::Continue => {}
                            Flow::Stop => return (Flow::Stop, events),
                            Flow::Failed => return (Flow::Failed, events),
                        }
                    }
                }
                (Flow::Continue, events)
            }

            WorkflowNode::Retry {
                body,
                times,
                backoff_ms,
            } => {
                let attempts = (*times).max(1);
                let mut events = Vec::new();
                for attempt in 0..attempts {
                    if ctx.cancel.is_cancelled() {
                        return (Flow::Stop, events);
                    }
                    let (flow, evs) = run_node(body, ctx, reg).await;
                    events.extend(evs);
                    if flow != Flow::Failed {
                        return (flow, events);
                    }
                    if attempt + 1 < attempts && *backoff_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(*backoff_ms)).await;
                    }
                }
                (Flow::Failed, events)
            }

            WorkflowNode::Timeout { body, millis } => {
                match tokio::time::timeout(Duration::from_millis(*millis), run_node(body, ctx, reg))
                    .await
                {
                    Ok(result) => result,
                    Err(_elapsed) => {
                        // Signal nested cancellation-aware work (e.g. scripts) to abort.
                        ctx.cancel.cancel();
                        (
                            Flow::Failed,
                            vec![error_event(
                                ctx,
                                format!("workflow timed out after {millis}ms"),
                            )],
                        )
                    }
                }
            }

            WorkflowNode::Wait { until } => {
                match until {
                    WaitCondition::Duration { millis } => {
                        tokio::time::sleep(Duration::from_millis(*millis)).await;
                    }
                }
                (Flow::Continue, Vec::new())
            }
        }
    })
}

/// Returns a short human-readable description of an action config for dry-run logs.
fn action_summary(cfg: &crate::domain::ActionConfig) -> String {
    use crate::domain::ActionConfig;
    match cfg {
        ActionConfig::Notify { title, .. } => format!("Notify \"{title}\""),
        ActionConfig::RunCommand { program, args, .. } => {
            format!("RunCommand: {program} {}", args.join(" "))
        }
        ActionConfig::SimulateInput { sequence } => {
            format!("SimulateInput ({} events)", sequence.len())
        }
        ActionConfig::Delay { millis } => format!("Delay {millis}ms"),
        ActionConfig::SetVariable { key, .. } => format!("SetVariable ${key}"),
        ActionConfig::RunScript { .. } => "RunScript".to_string(),
        ActionConfig::HttpRequest { method, url, .. } => format!("HttpRequest {method} {url}"),
        ActionConfig::Interact { target, op, .. } => {
            format!("Interact {op:?} on {target:?}")
        }
        ActionConfig::Custom { provider, .. } => format!("Custom:{provider}"),
    }
}

/// Builds, permission-gates, acquires resource locks, and awaits a single action.
async fn run_action(
    cfg: &crate::domain::ActionConfig,
    ctx: &mut ExecContext,
    reg: &Registry,
) -> (Flow, Vec<EngineEvent>) {
    let action = match reg.build_action(cfg) {
        Ok(a) => a,
        Err(e) => return (Flow::Failed, vec![error_event(ctx, e.to_string())]),
    };

    // Dry-run: skip execution, just log what would happen.
    if ctx.dry_run {
        return (
            Flow::Continue,
            vec![EngineEvent::ActionLog {
                macro_id: ctx.macro_id,
                action: action.id().to_string(),
                level: crate::engine::LogLevel::Info,
                message: format!("[DRY RUN] {}", action_summary(cfg)),
            }],
        );
    }

    // Central permission gate: every required permission must be granted.
    if let Some(missing) = action
        .required_permissions()
        .0
        .iter()
        .find(|p| !ctx.permissions.allows(p))
    {
        let msg = format!("permission denied: {missing:?}");
        return (Flow::Failed, vec![error_event(ctx, msg)]);
    }

    // Acquire exclusive resource locks to prevent concurrent access to shared
    // devices (keyboard/mouse input, clipboard, specific windows). Locks are
    // held for the duration of the action and released on drop.
    let resource_ids = action.resources();
    let mut guards = Vec::new();
    for id in &resource_ids {
        let lock = ctx.resource_pool.get_lock(id).await;
        guards.push(lock.lock_owned().await);
    }

    let result = match action.execute(ctx).await {
        Ok(Outcome::Continue) => (Flow::Continue, Vec::new()),
        Ok(Outcome::Stop) => (Flow::Stop, Vec::new()),
        Err(e) => (Flow::Failed, vec![error_event(ctx, e.to_string())]),
    };
    drop(guards);
    result
}

/// Evaluates an `If`/`While` condition against a read-only view of the context.
fn eval_condition(
    expr: &ConstraintExpr,
    ctx: &ExecContext,
    reg: &Registry,
) -> Result<bool, ConstraintError> {
    let eval_ctx = EvalContext {
        event: &ctx.event,
        macro_id: ctx.macro_id,
        store: ctx.store.as_ref(),
    };
    expr.evaluate(&eval_ctx, reg)
}

/// Constructs an [`EngineEvent::Error`] tagged with the current macro id.
fn error_event(ctx: &ExecContext, message: String) -> EngineEvent {
    EngineEvent::Error {
        macro_id: Some(ctx.macro_id),
        message,
    }
}
