//! [`NotifyAction`] — shows a desktop notification.

use async_trait::async_trait;
use koakuma_core::context::ExecContext;
use koakuma_core::domain::ActionConfig;
use koakuma_core::error::ActionError;
use koakuma_core::permission::PermissionSet;
use koakuma_core::traits::{Action, Outcome};

/// Shows a desktop notification with a title and body text.
///
/// Uses [`notify-rust`](https://crates.io/crates/notify-rust) which targets:
/// - **Windows** — WinRT `ToastNotification` via `winrt-notification`
/// - **Linux** — D-Bus / libnotify via `zbus` (pure Rust, no system dep)
/// - **macOS** — native user notifications
///
/// Does not require any special permission (notifications are non-destructive).
///
/// **Config**: [`ActionConfig::Notify`]
pub struct NotifyAction {
    title: String,
    body: String,
}

#[async_trait]
impl Action for NotifyAction {
    fn id(&self) -> &'static str {
        "notify"
    }

    fn required_permissions(&self) -> PermissionSet {
        PermissionSet::default()
    }

    async fn execute(&self, _ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        notify_rust::Notification::new()
            .summary(&self.title)
            .body(&self.body)
            .show()
            .map_err(|e| ActionError::Failed(format!("notification failed: {e}")))?;
        Ok(Outcome::Continue)
    }
}

/// Factory: builds [`NotifyAction`] from [`ActionConfig::Notify`].
pub fn build(c: &ActionConfig) -> Option<Box<dyn Action>> {
    if let ActionConfig::Notify { title, body } = c {
        Some(Box::new(NotifyAction {
            title: title.clone(),
            body: body.clone(),
        }))
    } else {
        None
    }
}
