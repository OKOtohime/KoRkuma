use crate::domain::MacroId;
use crate::event::Event;
use crate::permission::PermissionGrant;
use crate::state::StateStore;
use crate::value::Value;
use std::collections::{BTreeMap, HashMap};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Mutex as TokioMutex;

/// A shared pool of per-resource async locks.
///
/// Passed through [`ExecContext`] so the workflow engine can acquire exclusive
/// locks immediately before each action that declares
/// [`traits::Action::resources`], preventing interleaving of actions that write
/// to the same device (keyboard injection, clipboard, specific windows).
///
/// All concurrent workflows spawned by the same [`WorkflowScheduler`] share one
/// pool, so resource locks are effective across macros.
///
/// Built-in resource names: `"input"` (keyboard/mouse injection) and
/// `"clipboard"`. Window-specific resources use `"window:<id>"` naming.
///
/// # Examples
///
/// ```rust
/// # use koakuma_core::context::ResourcePool;
/// # let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
/// # rt.block_on(async {
/// let pool = ResourcePool::new();
/// let lock1 = pool.get_lock("input").await;
/// let lock2 = pool.get_lock("input").await;
/// // lock1 and lock2 point to the same underlying mutex.
/// # });
/// ```
#[derive(Clone, Default)]
pub struct ResourcePool(Arc<TokioMutex<HashMap<String, Arc<TokioMutex<()>>>>>);

impl ResourcePool {
    /// Creates a new empty pool. Locks are created on first access.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the async mutex for `id`, creating it if this is the first request.
    pub async fn get_lock(&self, id: &str) -> Arc<TokioMutex<()>> {
        let mut map = self.0.lock().await;
        map.entry(id.to_string())
            .or_insert_with(|| Arc::new(TokioMutex::new(())))
            .clone()
    }
}

/// Local variable storage scoped to one macro execution.
///
/// Written by [`traits::Action`] implementations via `ctx.locals` when
/// [`domain::VarScope::Local`] is selected; discarded after the macro finishes.
///
/// # Examples
///
/// ```rust
/// use koakuma_core::context::LocalVars;
/// use koakuma_core::value::Value;
///
/// let mut vars = LocalVars::new();
/// vars.insert("result".to_string(), Value::Int(0));
/// assert_eq!(vars.get("result"), Some(&Value::Int(0)));
/// ```
pub type LocalVars = BTreeMap<String, Value>;

/// A cloneable, thread-safe cancellation signal for a running macro.
///
/// Any holder can call [`cancel`](CancellationToken::cancel); all clones of the same
/// token will observe the cancellation immediately.
///
/// # Examples
///
/// ```rust
/// use koakuma_core::context::CancellationToken;
///
/// let token = CancellationToken::new();
/// let clone = token.clone();
///
/// assert!(!clone.is_cancelled());
/// token.cancel();
/// assert!(clone.is_cancelled());
/// ```
#[derive(Clone)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    /// Creates a new token in the non-cancelled state.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::context::CancellationToken;
    ///
    /// let token = CancellationToken::new();
    /// assert!(!token.is_cancelled());
    /// ```
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    /// Marks the token as cancelled. All clones observe this change immediately.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::context::CancellationToken;
    ///
    /// let token = CancellationToken::new();
    /// token.cancel();
    /// assert!(token.is_cancelled());
    /// ```
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Returns `true` if [`cancel`](Self::cancel) has been called on any clone of this token.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::context::CancellationToken;
    ///
    /// let token = CancellationToken::new();
    /// assert!(!token.is_cancelled());
    /// token.cancel();
    /// assert!(token.is_cancelled());
    /// ```
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle for actions to emit structured log entries back to the UI.
/// Wired to the engine's EngineEvent channel in M1.2; no-op for now.
#[derive(Clone, Default)]
pub struct LogHandle;

impl LogHandle {
    /// Emits a log entry at the given level. No-op until M1.2 wires the engine channel.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::context::LogHandle;
    ///
    /// let log = LogHandle::default();
    /// log.log("info", "action completed"); // no-op in M1.1
    /// ```
    pub fn log(&self, _level: &str, _msg: &str) {}
}

/// Read-only evaluation context passed to Constraint::evaluate.
pub struct EvalContext<'a> {
    pub event: &'a Event,
    pub macro_id: MacroId,
    pub store: &'a dyn StateStore,
}

/// Mutable execution context passed to Action::execute.
/// Lives for the duration of one macro trigger.
pub struct ExecContext {
    pub event: Event,
    pub macro_id: MacroId,
    pub locals: LocalVars,
    pub store: Arc<dyn StateStore>,
    pub permissions: PermissionGrant,
    pub cancel: CancellationToken,
    pub log: LogHandle,
    /// M2.2: shared pool of per-resource async locks (see [`ResourcePool`]).
    pub resource_pool: ResourcePool,
}

impl ExecContext {
    /// Creates an independent context for a concurrent workflow branch.
    ///
    /// The fork shares the global state store, cancellation token, permission grant,
    /// and log handle (all cheaply cloned / reference-counted), but receives its **own
    /// copy** of the local variables. Local writes in a forked branch therefore do not
    /// propagate back to the parent — branches only communicate through the shared
    /// [`StateStore`]. Used by [`WorkflowNode::Parallel`](crate::domain::WorkflowNode::Parallel).
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use std::sync::Arc;
    /// # use koakuma_core::context::{ExecContext, CancellationToken, LogHandle, ResourcePool};
    /// # use koakuma_core::event::{Event, EventKind};
    /// # use koakuma_core::permission::PermissionGrant;
    /// # use koakuma_core::state::StateStore;
    /// # use koakuma_core::value::Value;
    /// # use koakuma_store::InMemoryStateStore;
    /// # use std::time::SystemTime;
    /// let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
    /// let mut ctx = ExecContext {
    ///     event: Event { kind: EventKind::Manual, source: "t".into(), timestamp: SystemTime::now(), payload: Value::Null },
    ///     macro_id: uuid::Uuid::nil(),
    ///     locals: Default::default(),
    ///     store,
    ///     permissions: PermissionGrant::new(vec![]),
    ///     cancel: CancellationToken::new(),
    ///     log: LogHandle::default(),
    ///     resource_pool: ResourcePool::default(),
    /// };
    /// ctx.locals.insert("seed".into(), Value::Int(1));
    /// let fork = ctx.fork();
    /// assert_eq!(fork.locals.get("seed"), Some(&Value::Int(1)));
    /// ```
    pub fn fork(&self) -> ExecContext {
        ExecContext {
            event: self.event.clone(),
            macro_id: self.macro_id,
            locals: self.locals.clone(),
            store: Arc::clone(&self.store),
            permissions: self.permissions.clone(),
            cancel: self.cancel.clone(),
            log: self.log.clone(),
            resource_pool: self.resource_pool.clone(),
        }
    }
}
