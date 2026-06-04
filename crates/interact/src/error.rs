use thiserror::Error;

/// Errors produced by an [`InteractionBackend`](crate::backend::InteractionBackend).
///
/// `NotAvailable` and `NotSupported` are non-fatal from the negotiator's perspective:
/// it will try the next candidate backend.  `Internal` and `NotFound` propagate to the
/// caller as [`DispatchError::Backend`].
///
/// # Examples
///
/// ```rust
/// use koakuma_interact::BackendError;
///
/// let e = BackendError::NotFound("Notepad".into());
/// assert!(e.to_string().contains("not found"));
/// ```
#[derive(Debug, Error)]
pub enum BackendError {
    /// The backend could resolve the selector but the requested element was not found.
    #[error("target not found: {0}")]
    NotFound(String),
    /// The backend itself is not available (e.g. CDP port not open, COM init failed).
    #[error("backend not available: {0}")]
    NotAvailable(String),
    /// The requested operation is not supported by this backend.
    #[error("operation not supported: {0}")]
    NotSupported(String),
    /// Unexpected internal error during execution.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Errors produced by [`BackendRegistry::dispatch`](crate::negotiator::BackendRegistry::dispatch).
///
/// `Queued` is a non-error signal: the caller should arrange a retry when the target
/// becomes reachable.  `NoBackend` means the operation must not be attempted.
///
/// # Examples
///
/// ```rust
/// use koakuma_interact::DispatchError;
///
/// let e = DispatchError::NoBackend;
/// assert!(e.to_string().contains("no background"));
/// ```
#[derive(Debug, Error)]
pub enum DispatchError {
    /// No backend can serve this target at any usable tier.
    #[error("no background-capable backend found for target")]
    NoBackend,
    /// The `Queue` policy was applied; the operation should be retried later.
    #[error("operation queued; will retry when target becomes available")]
    Queued,
    /// The underlying backend returned an error.
    #[error("backend error: {0}")]
    Backend(#[from] BackendError),
    /// A required permission was not granted.
    #[error("permission denied: {0}")]
    PermissionDenied(String),
}
