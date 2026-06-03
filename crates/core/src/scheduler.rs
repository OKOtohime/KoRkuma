//! Workflow scheduler: the layer between the synchronous Event Router and the
//! Tokio runtime (DESIGN.md §14.3).
//!
//! [`WorkflowScheduler`] manages per-macro concurrency state, shared resource
//! locks, and cancellation propagation. The Router remains single-threaded and
//! lock-free; all coordination lives here.
//!
//! # Concurrency policies
//!
//! Each macro carries a [`ConcurrencyPolicy`] that governs what happens when the
//! same macro is triggered while a previous instance is still running:
//!
//! | Policy | Behaviour |
//! |--------|-----------|
//! | `Parallel` | Every trigger spawns an independent workflow (V1 default). |
//! | `Queue{max}` | Serialise; up to `max` pending items queued, extras dropped. |
//! | `DropIfRunning` | Ignore new triggers while one instance is running. |
//! | `RestartIfRunning` | Cancel the running instance and start a fresh one. |
//! | `Debounce{ms}` | Wait `ms` after last trigger; resets on each new trigger. |
//! | `Throttle{ms}` | At most one execution per `ms` window (first-trigger wins). |
//!
//! # Resource arbitration
//!
//! Actions may declare [`traits::Action::resources`] to claim exclusive access to
//! a shared device (e.g., `"input"` for keyboard/mouse injection). The scheduler
//! provides a single [`ResourcePool`](crate::context::ResourcePool) shared by all
//! concurrent workflows; the workflow engine acquires the relevant locks in
//! [`crate::workflow::run_workflow`] before each action.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;

use crate::context::{CancellationToken, ExecContext, LogHandle, ResourcePool};
use crate::domain::{ConcurrencyPolicy, MacroId, WorkflowNode};
use crate::engine::EngineEvent;
use crate::registry::Registry;
use crate::workflow::run_workflow;

// ── Built-in resource names ───────────────────────────────────────────────────

/// Resource ID for keyboard/mouse injection actions.
pub const RESOURCE_INPUT: &str = "input";
/// Resource ID for clipboard read/write actions.
pub const RESOURCE_CLIPBOARD: &str = "clipboard";

// ── Per-macro runtime state ───────────────────────────────────────────────────

struct PendingWork {
    ctx: ExecContext,
    workflow: WorkflowNode,
    reg: Arc<Registry>,
}

#[derive(Default)]
struct MacroState {
    /// Number of in-flight workflow instances.
    running: u32,
    /// Pending work items for the `Queue` policy.
    queue: VecDeque<PendingWork>,
    /// Active cancel tokens held for `RestartIfRunning`.
    active_cancels: Vec<CancellationToken>,
    /// Last execution start time, used by `Throttle`.
    last_fire: Option<Instant>,
    /// Pending debounce cancel token, used by `Debounce`.
    debounce_cancel: Option<CancellationToken>,
}

// ── SchedulerInner ────────────────────────────────────────────────────────────

struct SchedulerInner {
    states: Mutex<HashMap<MacroId, MacroState>>,
    pool: ResourcePool,
}

// ── WorkflowScheduler ─────────────────────────────────────────────────────────

/// Async workflow dispatcher with per-macro concurrency policy enforcement.
///
/// Sits between the synchronous [`EventRouter`](crate::router::EventRouter) and
/// the Tokio multi-thread runtime. The router remains single-threaded and
/// lock-free; the scheduler owns all async coordination.
///
/// # Thread safety
///
/// `WorkflowScheduler` is cheaply cloneable (`Arc`-backed) and `Send + Sync`.
/// A single instance is shared between the engine loop and the spawned workflow
/// tasks.
#[derive(Clone)]
pub struct WorkflowScheduler {
    inner: Arc<SchedulerInner>,
    evt_tx: crossbeam_channel::Sender<EngineEvent>,
    handle: tokio::runtime::Handle,
}

impl WorkflowScheduler {
    /// Creates a new scheduler.
    ///
    /// - `handle` — the Tokio runtime on which workflow tasks are spawned.
    /// - `evt_tx` — channel used to forward [`EngineEvent`]s (errors) back to
    ///   the engine loop's `on_event` callback.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use koakuma_core::scheduler::WorkflowScheduler;
    ///
    /// let rt = tokio::runtime::Builder::new_multi_thread()
    ///     .enable_all().build().unwrap();
    /// let (tx, _rx) = crossbeam_channel::unbounded();
    /// let sched = WorkflowScheduler::new(rt.handle().clone(), tx);
    /// ```
    pub fn new(
        handle: tokio::runtime::Handle,
        evt_tx: crossbeam_channel::Sender<EngineEvent>,
    ) -> Self {
        Self {
            inner: Arc::new(SchedulerInner {
                states: Mutex::new(HashMap::new()),
                pool: ResourcePool::new(),
            }),
            evt_tx,
            handle,
        }
    }

    /// Returns the shared [`ResourcePool`] that all scheduled workflows use.
    ///
    /// Pass this pool when constructing [`ExecContext`] so resource locks are
    /// effective across concurrent macros.
    pub fn resource_pool(&self) -> &ResourcePool {
        &self.inner.pool
    }

    /// Schedule a workflow according to `policy`.
    ///
    /// Returns immediately; the actual execution is spawned as a Tokio task.
    /// Any [`EngineEvent`]s produced (errors) are forwarded via the channel
    /// provided at construction time.
    pub fn schedule(
        &self,
        macro_id: MacroId,
        policy: &ConcurrencyPolicy,
        ctx: ExecContext,
        workflow: WorkflowNode,
        reg: Arc<Registry>,
    ) {
        let inner = Arc::clone(&self.inner);
        let evt_tx = self.evt_tx.clone();
        let policy = policy.clone();

        self.handle.spawn(async move {
            Self::apply_policy(inner, macro_id, policy, ctx, workflow, reg, evt_tx).await;
        });
    }

    // ── Policy application ────────────────────────────────────────────────────

    async fn apply_policy(
        inner: Arc<SchedulerInner>,
        macro_id: MacroId,
        policy: ConcurrencyPolicy,
        ctx: ExecContext,
        workflow: WorkflowNode,
        reg: Arc<Registry>,
        evt_tx: crossbeam_channel::Sender<EngineEvent>,
    ) {
        match policy {
            ConcurrencyPolicy::Parallel => {
                {
                    inner.states.lock().await.entry(macro_id).or_default().running += 1;
                }
                Self::execute(inner, macro_id, ctx, workflow, reg, evt_tx).await;
            }

            ConcurrencyPolicy::DropIfRunning => {
                let should_run = {
                    let mut g = inner.states.lock().await;
                    let s = g.entry(macro_id).or_default();
                    if s.running > 0 {
                        false
                    } else {
                        s.running += 1;
                        true
                    }
                };
                if should_run {
                    Self::execute(inner, macro_id, ctx, workflow, reg, evt_tx).await;
                }
            }

            ConcurrencyPolicy::Queue { max } => {
                let mut work = Some((ctx, workflow, reg));
                let run_now = {
                    let mut g = inner.states.lock().await;
                    let s = g.entry(macro_id).or_default();
                    if s.running == 0 {
                        s.running += 1;
                        true
                    } else if s.queue.len() < max {
                        let (ctx, workflow, reg) = work.take().unwrap();
                        s.queue.push_back(PendingWork { ctx, workflow, reg });
                        false
                    } else {
                        false // queue full — drop
                    }
                };
                if run_now {
                    let (ctx, workflow, reg) = work.unwrap();
                    Self::execute(inner, macro_id, ctx, workflow, reg, evt_tx).await;
                }
            }

            ConcurrencyPolicy::RestartIfRunning => {
                let to_cancel: Vec<CancellationToken> = {
                    let mut g = inner.states.lock().await;
                    let s = g.entry(macro_id).or_default();
                    let prev = s.active_cancels.drain(..).collect();
                    s.running += 1;
                    s.active_cancels.push(ctx.cancel.clone());
                    prev
                };
                for token in to_cancel {
                    token.cancel();
                }
                Self::execute(inner, macro_id, ctx, workflow, reg, evt_tx).await;
            }

            ConcurrencyPolicy::Debounce { ms } => {
                let debounce_token = {
                    let mut g = inner.states.lock().await;
                    let s = g.entry(macro_id).or_default();
                    if let Some(prev) = s.debounce_cancel.take() {
                        prev.cancel();
                    }
                    let tok = CancellationToken::new();
                    s.debounce_cancel = Some(tok.clone());
                    tok
                };

                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;

                if debounce_token.is_cancelled() {
                    return;
                }

                {
                    let mut g = inner.states.lock().await;
                    let s = g.entry(macro_id).or_default();
                    s.debounce_cancel = None;
                    s.running += 1;
                }
                Self::execute(inner, macro_id, ctx, workflow, reg, evt_tx).await;
            }

            ConcurrencyPolicy::Throttle { ms } => {
                let should_run = {
                    let mut g = inner.states.lock().await;
                    let s = g.entry(macro_id).or_default();
                    let now = Instant::now();
                    let ok = s
                        .last_fire
                        .map(|t| now.duration_since(t).as_millis() >= u128::from(ms))
                        .unwrap_or(true);
                    if ok {
                        s.last_fire = Some(now);
                        s.running += 1;
                    }
                    ok
                };
                if should_run {
                    Self::execute(inner, macro_id, ctx, workflow, reg, evt_tx).await;
                }
            }
        }
    }

    // ── Execution and completion ──────────────────────────────────────────────

    /// Runs one workflow instance and loops over any queued follow-on work.
    ///
    /// Using a loop (instead of recursive `tokio::spawn`) avoids the circular
    /// `Send` proof that the Rust compiler cannot resolve for recursive async fns.
    ///
    /// After each workflow completes:
    /// - Decrements `running`.
    /// - Removes stale cancel tokens.
    /// - Pops and executes the next queued item if one exists (Queue policy).
    async fn execute(
        inner: Arc<SchedulerInner>,
        macro_id: MacroId,
        ctx: ExecContext,
        workflow: WorkflowNode,
        reg: Arc<Registry>,
        evt_tx: crossbeam_channel::Sender<EngineEvent>,
    ) {
        let mut cur_ctx = ctx;
        let mut cur_wf = workflow;
        let mut cur_reg = reg;

        loop {
            let events = run_workflow(&cur_wf, &mut cur_ctx, &cur_reg).await;
            for ev in events {
                let _ = evt_tx.send(ev);
            }

            // Decrement running; pop queued item if queue is non-empty.
            let next = {
                let mut g = inner.states.lock().await;
                let s = g.entry(macro_id).or_default();
                s.running = s.running.saturating_sub(1);
                s.active_cancels.retain(|t| !t.is_cancelled());
                if s.queue.is_empty() {
                    None
                } else {
                    s.running += 1;
                    s.queue.pop_front()
                }
            };

            match next {
                Some(PendingWork {
                    ctx: next_ctx,
                    workflow: next_wf,
                    reg: next_reg,
                }) => {
                    cur_ctx = next_ctx;
                    cur_wf = next_wf;
                    cur_reg = next_reg;
                }
                None => break,
            }
        }
    }
}

// ── Default ExecContext builder (test helper re-exported) ─────────────────────

/// Constructs a minimal [`ExecContext`] with the given store and resource pool.
///
/// Intended for unit tests that exercise the scheduler directly, without the
/// full engine pipeline.
pub fn test_exec_ctx(
    store: std::sync::Arc<dyn crate::state::StateStore>,
    pool: ResourcePool,
) -> ExecContext {
    use crate::event::{Event, EventKind};
    use crate::permission::PermissionGrant;
    use crate::value::Value;
    use std::time::SystemTime;

    ExecContext {
        event: Event {
            kind: EventKind::Manual,
            source: "scheduler-test".into(),
            timestamp: SystemTime::now(),
            payload: Value::Null,
        },
        macro_id: MacroId::nil(),
        locals: Default::default(),
        store,
        permissions: PermissionGrant::new(vec![]),
        cancel: CancellationToken::new(),
        log: LogHandle,
        resource_pool: pool,
    }
}
