use async_trait::async_trait;
use korkuma_core::domain::{TargetSelector, UiOp, UiPath};
use crate::error::BackendError;

/// Capability tier of an [`InteractionBackend`] for a specific resolved target.
///
/// Ordered: `Background` > `ForegroundSynthetic` > `Unsupported`.
/// The negotiator picks the highest achievable tier.
///
/// # Examples
///
/// ```rust
/// use korkuma_interact::backend::Tier;
///
/// assert!(Tier::Background > Tier::ForegroundSynthetic);
/// assert!(Tier::ForegroundSynthetic > Tier::Unsupported);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Backend cannot serve this target.
    Unsupported = 0,
    /// Must bring window to foreground first (user-visible focus change).
    ForegroundSynthetic = 1,
    /// Can interact without stealing focus.
    Background = 2,
}

/// Opaque resolved target handle produced by [`InteractionBackend::resolve`].
///
/// Carries the platform-specific data needed by a backend to operate on the
/// target.  The `backend_id` field identifies which backend created it.
pub struct ResolvedTarget {
    /// ID of the backend that created this handle.
    pub backend_id: &'static str,
    /// Human-readable label for logs and the target-picker UI.
    pub display_name: String,
    pub(crate) inner: TargetInner,
}

#[allow(dead_code)] // fields are part of the domain model; some only read in platform-specific paths
pub(crate) enum TargetInner {
    #[cfg(target_os = "windows")]
    WindowHandle { hwnd: isize },
    BrowserTab {
        port: u16,
        target_id: String,
        ws_url: String,
        url: String,
    },
    Stub { id: String, tier: Tier },
}

/// A single node in a UI element tree, returned by [`InteractionBackend::enumerate`].
#[derive(Debug, Clone)]
pub struct UiNode {
    /// Path usable in [`UiOp`] to target this element.
    pub path: UiPath,
    /// Accessible name of the element.
    pub name: String,
    /// Control-type string (e.g. `"Button"`, `"Edit"`, `"input"`).
    pub control_type: String,
}

/// A backend that can interact with a class of targets (windows, browser tabs, …).
///
/// Implementations are registered with [`BackendRegistry`](crate::negotiator::BackendRegistry).
/// The registry picks the best available backend via capability negotiation (§13.2).
#[async_trait]
pub trait InteractionBackend: Send + Sync {
    /// Stable unique identifier, e.g. `"windows_uia"`, `"cdp"`.
    fn id(&self) -> &'static str;

    /// Resolve `sel` to concrete target handles this backend can serve.
    ///
    /// Returns an empty vec (not an error) if the backend cannot match the selector.
    async fn resolve(&self, sel: &TargetSelector) -> Result<Vec<ResolvedTarget>, BackendError>;

    /// Report the highest capability tier achievable for `t`.
    ///
    /// This is a quick synchronous check; it should not perform I/O.
    fn capability(&self, t: &ResolvedTarget) -> Tier;

    /// Execute `op` on the resolved target `t`.
    async fn invoke(&self, t: &ResolvedTarget, op: &UiOp) -> Result<(), BackendError>;

    /// Enumerate child UI nodes of `t` for the target-picker UI.
    async fn enumerate(&self, t: &ResolvedTarget) -> Result<Vec<UiNode>, BackendError>;
}
