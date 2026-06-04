use thiserror::Error;

/// Errors produced by a [`traits::HookProvider`] during initialization or startup.
///
/// # Examples
///
/// ```rust
/// use korkuma_core::error::HookError;
///
/// let err = HookError::InitFailed("device not found".to_string());
/// assert!(err.to_string().contains("device not found"));
///
/// let running = HookError::AlreadyRunning;
/// assert_eq!(running.to_string(), "hook already running");
/// ```
#[derive(Debug, Error)]
pub enum HookError {
    #[error("hook initialization failed: {0}")]
    InitFailed(String),
    #[error("hook already running")]
    AlreadyRunning,
}

/// Errors produced by [`registry::Registry`] when no registered factory handles a config value.
///
/// # Examples
///
/// ```rust
/// use korkuma_core::error::RegistryError;
///
/// let err = RegistryError::UnknownProvider("my_custom".to_string());
/// assert!(err.to_string().contains("my_custom"));
/// ```
#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("unknown provider: {0}")]
    UnknownProvider(String),
}

/// Errors produced during constraint evaluation in [`domain::ConstraintExpr::evaluate`].
///
/// `RegistryError` converts automatically via `#[from]`, so `?` works when calling
/// `Registry::build_constraint` inside an evaluation.
///
/// # Examples
///
/// ```rust
/// use korkuma_core::error::{ConstraintError, RegistryError};
///
/// let reg_err = RegistryError::UnknownProvider("x".to_string());
/// let c_err = ConstraintError::from(reg_err);
/// assert!(c_err.to_string().contains("registry error"));
///
/// let eval_err = ConstraintError::EvalFailed("window title unavailable".to_string());
/// assert!(eval_err.to_string().contains("evaluation failed"));
/// ```
#[derive(Debug, Error)]
pub enum ConstraintError {
    #[error("registry error: {0}")]
    Registry(#[from] RegistryError),
    #[error("evaluation failed: {0}")]
    EvalFailed(String),
}

/// Errors produced during action execution in [`traits::Action::execute`].
///
/// # Examples
///
/// ```rust
/// use korkuma_core::error::ActionError;
///
/// let err = ActionError::Failed("process exited with code 1".to_string());
/// assert!(err.to_string().contains("action failed"));
///
/// let denied = ActionError::PermissionDenied("RunCommand".to_string());
/// assert!(denied.to_string().contains("permission denied"));
///
/// assert_eq!(ActionError::Cancelled.to_string(), "cancelled");
/// ```
#[derive(Debug, Error)]
pub enum ActionError {
    #[error("action failed: {0}")]
    Failed(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("cancelled")]
    Cancelled,
}
