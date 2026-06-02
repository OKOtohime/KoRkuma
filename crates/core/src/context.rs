use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use crate::domain::MacroId;
use crate::event::Event;
use crate::permission::PermissionGrant;
use crate::state::StateStore;
use crate::value::Value;

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
}
