use std::sync::Arc;

use async_trait::async_trait;
use koakuma_core::context::ExecContext;
use koakuma_core::domain::{ActionConfig, OnNoBackground, TargetSelector, UiOp};
use koakuma_core::error::ActionError;
use koakuma_core::permission::{Permission, PermissionSet};
use koakuma_core::registry::Registry;
use koakuma_core::traits::{Action, Outcome};

use crate::error::DispatchError;
use crate::negotiator::BackendRegistry;

/// Runtime action that dispatches a [`UiOp`] to a [`TargetSelector`] via the
/// registered [`InteractionBackend`](crate::backend::InteractionBackend) pool.
pub struct InteractAction {
    target: TargetSelector,
    op: UiOp,
    on_no_background: OnNoBackground,
    registry: Arc<BackendRegistry>,
}

#[async_trait]
impl Action for InteractAction {
    fn id(&self) -> &'static str {
        "interact"
    }

    fn required_permissions(&self) -> PermissionSet {
        let mut perms = Vec::new();
        match &self.target {
            TargetSelector::BrowserTab { .. } => perms.push(Permission::BrowserControl),
            _ => perms.push(Permission::WindowInteraction),
        }
        if matches!(self.on_no_background, OnNoBackground::Degrade) {
            perms.push(Permission::ForegroundTakeover);
        }
        PermissionSet(perms)
    }

    async fn execute(&self, ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        ctx.log.log(
            "debug",
            &format!("interact: {:?} → target={:?}", self.op, self.target),
        );

        self.registry
            .dispatch(&self.target, &self.op, self.on_no_background, &ctx.permissions)
            .await
            .map_err(dispatch_to_action_error)?;

        Ok(Outcome::Continue)
    }
}

fn dispatch_to_action_error(e: DispatchError) -> ActionError {
    match e {
        DispatchError::PermissionDenied(msg) => ActionError::PermissionDenied(msg),
        DispatchError::NoBackend => {
            ActionError::Failed("no interaction backend available for target".into())
        }
        DispatchError::Queued => {
            ActionError::Failed("operation queued; retry when target becomes available".into())
        }
        DispatchError::Backend(be) => ActionError::Failed(format!("backend error: {be}")),
    }
}

/// Register the `Interact` action factory with `registry`, backed by `backend_registry`.
///
/// Call this during application start-up, after creating and populating a
/// [`BackendRegistry`] with the platform backends.
pub fn register_actions(registry: &mut Registry, backend_registry: Arc<BackendRegistry>) {
    let br = backend_registry;
    registry.register_action(move |cfg| match cfg {
        ActionConfig::Interact { target, op, on_no_background } => {
            Some(Box::new(InteractAction {
                target: target.clone(),
                op: op.clone(),
                on_no_background: *on_no_background,
                registry: Arc::clone(&br),
            }) as Box<dyn Action>)
        }
        _ => None,
    });
}
