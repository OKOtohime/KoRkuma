use crate::context::{EvalContext, ExecContext};
use crate::error::{ActionError, ConstraintError, HookError};
use crate::event::{Event, EventKind};
use crate::permission::PermissionSet;
use async_trait::async_trait;
use crossbeam_channel::Sender;

/// The channel end that HookProviders use to push events into the engine.
pub type EventSink = Sender<Event>;

/// A platform-specific event source. Runs on its own thread.
///
/// Implement this trait to add a new event source (keyboard hook, window monitor,
/// network listener, etc.). Register the implementation with the engine before
/// dispatching events through [`router::EventRouter`].
///
/// # Examples
///
/// ```rust,no_run
/// use koakuma_core::traits::{HookProvider, EventSink};
/// use koakuma_core::event::EventKind;
/// use koakuma_core::error::HookError;
///
/// struct MyProvider;
///
/// impl HookProvider for MyProvider {
///     fn id(&self) -> &'static str { "my_provider" }
///     fn produces(&self) -> &'static [EventKind] { &[EventKind::Custom] }
///     fn start(&mut self, _sink: EventSink) -> Result<(), HookError> { Ok(()) }
///     fn stop(&mut self) {}
/// }
/// ```
pub trait HookProvider: Send {
    /// Returns a stable, unique identifier for this provider (e.g. `"hotkey_win32"`).
    fn id(&self) -> &'static str;
    /// Declares which EventKinds this provider produces — used to build the Router index.
    fn produces(&self) -> &'static [EventKind];
    /// Starts the provider on a background thread, sending events to `sink`.
    ///
    /// # Errors
    ///
    /// Returns [`HookError::InitFailed`] if the OS hook cannot be installed,
    /// or [`HookError::AlreadyRunning`] if called while the provider is active.
    fn start(&mut self, sink: EventSink) -> Result<(), HookError>;
    /// Signals the provider to stop and releases any OS resources it holds.
    fn stop(&mut self);
}

/// Instantiated from a TriggerConfig: fine-grained per-event matching after the Router selects
/// candidate macros by EventKind.
///
/// The router performs a two-level filter: coarse `EventKind` lookup (O(1) via index),
/// then `TriggerSpec::matches` for precise matching within the candidate set.
///
/// # Examples
///
/// ```rust,no_run
/// use koakuma_core::traits::TriggerSpec;
/// use koakuma_core::event::{Event, EventKind};
///
/// struct AlwaysMatches;
///
/// impl TriggerSpec for AlwaysMatches {
///     fn subscribed_kinds(&self) -> &[EventKind] { &[EventKind::Manual] }
///     fn matches(&self, _event: &Event) -> bool { true }
/// }
/// ```
pub trait TriggerSpec: Send + Sync {
    /// Returns the `EventKind` values this spec subscribes to.
    fn subscribed_kinds(&self) -> &[EventKind];
    /// Returns `true` if this spec matches the given event.
    fn matches(&self, event: &Event) -> bool;
}

/// A single evaluatable constraint leaf.
///
/// Constraints gate macro execution: the engine evaluates the macro's
/// [`domain::ConstraintExpr`] tree, calling `evaluate` on each leaf node.
///
/// # Examples
///
/// ```rust,no_run
/// use koakuma_core::traits::Constraint;
/// use koakuma_core::context::EvalContext;
/// use koakuma_core::error::ConstraintError;
///
/// struct AlwaysTrue;
///
/// impl Constraint for AlwaysTrue {
///     fn evaluate(&self, _ctx: &EvalContext) -> Result<bool, ConstraintError> {
///         Ok(true)
///     }
/// }
/// ```
pub trait Constraint: Send + Sync {
    /// Evaluates the constraint against the current execution context.
    ///
    /// # Errors
    ///
    /// Returns [`ConstraintError::EvalFailed`] if runtime data required for evaluation
    /// (e.g., window title, variable value) is unavailable.
    fn evaluate(&self, ctx: &EvalContext) -> Result<bool, ConstraintError>;
}

/// Control-flow signal returned by an action. V2 will extend with branch/jump variants.
///
/// # Examples
///
/// ```rust
/// use koakuma_core::traits::Outcome;
///
/// fn decide(flag: bool) -> Outcome {
///     if flag { Outcome::Stop } else { Outcome::Continue }
/// }
/// assert!(matches!(decide(true),  Outcome::Stop));
/// assert!(matches!(decide(false), Outcome::Continue));
/// ```
#[derive(Debug)]
pub enum Outcome {
    /// Continue to the next action in the sequence.
    Continue,
    /// Halt execution; remaining actions in the macro are skipped.
    Stop,
}

/// A single executable action step. Executed asynchronously on the engine's Tokio runtime (V2).
///
/// Implement this trait to add a new action type. Check
/// `ctx.permissions.allows(...)` before performing any sensitive operation.
///
/// The trait uses [`macro@async_trait`] so it stays dyn-compatible: the registry stores
/// actions as `Box<dyn Action>` and the workflow engine awaits `execute`. Implementations
/// should `.await` on async I/O (timers, network) rather than blocking the runtime.
///
/// # Examples
///
/// ```rust,no_run
/// use async_trait::async_trait;
/// use koakuma_core::traits::{Action, Outcome};
/// use koakuma_core::context::ExecContext;
/// use koakuma_core::error::ActionError;
/// use koakuma_core::permission::PermissionSet;
///
/// struct NoOpAction;
///
/// #[async_trait]
/// impl Action for NoOpAction {
///     fn id(&self) -> &'static str { "noop" }
///     fn required_permissions(&self) -> PermissionSet { PermissionSet::default() }
///     async fn execute(&self, _ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
///         Ok(Outcome::Continue)
///     }
/// }
/// ```
#[async_trait]
pub trait Action: Send + Sync {
    /// Returns a stable, unique identifier for this action type (e.g. `"run_command"`).
    fn id(&self) -> &'static str;
    /// Declares the permissions this action needs; shown to the user at macro-save time.
    fn required_permissions(&self) -> PermissionSet;
    /// Resource IDs this action requires exclusive access to while executing.
    ///
    /// The scheduler acquires named async locks before the action runs and releases
    /// them on completion, preventing two concurrent macros from interleaving writes
    /// to the same device (keyboard/mouse input, clipboard, a specific window).
    ///
    /// Built-in names: `"input"` (keyboard/mouse injection), `"clipboard"`.
    /// Window-scoped: `"window:<id>"`. Default implementation returns no resources.
    fn resources(&self) -> Vec<String> {
        vec![]
    }
    /// Executes the action, mutating `ctx` as needed (e.g., writing local variables).
    ///
    /// # Errors
    ///
    /// Returns [`ActionError::PermissionDenied`] if a required permission was not granted,
    /// or [`ActionError::Cancelled`] if `ctx.cancel` was signalled before completion.
    async fn execute(&self, ctx: &mut ExecContext) -> Result<Outcome, ActionError>;
}
