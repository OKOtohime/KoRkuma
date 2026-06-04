use async_trait::async_trait;
use koakuma_core::domain::{TargetSelector, UiOp};

use crate::backend::{InteractionBackend, ResolvedTarget, TargetInner, Tier, UiNode};
use crate::error::BackendError;

/// Configurable stub backend for tests.
///
/// Resolves every selector to a single `Stub` target at the configured tier.
/// If `invoke_error` is set, `invoke` returns that error instead of `Ok(())`.
pub struct StubBackend {
    pub tier: Tier,
    pub invoke_error: Option<String>,
}

impl StubBackend {
    /// Creates a stub that succeeds at `tier`.
    pub fn new(tier: Tier) -> Self {
        Self { tier, invoke_error: None }
    }

    /// Creates a stub that fails every `invoke` with `error`.
    pub fn failing(tier: Tier, error: &str) -> Self {
        Self { tier, invoke_error: Some(error.to_string()) }
    }
}

#[async_trait]
impl InteractionBackend for StubBackend {
    fn id(&self) -> &'static str {
        "stub"
    }

    async fn resolve(&self, sel: &TargetSelector) -> Result<Vec<ResolvedTarget>, BackendError> {
        if self.tier == Tier::Unsupported {
            return Ok(vec![]);
        }
        let id = format!("{sel:?}");
        Ok(vec![ResolvedTarget {
            backend_id: "stub",
            display_name: id.clone(),
            inner: TargetInner::Stub { id, tier: self.tier },
        }])
    }

    fn capability(&self, t: &ResolvedTarget) -> Tier {
        match &t.inner {
            TargetInner::Stub { tier, .. } => *tier,
            _ => Tier::Unsupported,
        }
    }

    async fn invoke(&self, _t: &ResolvedTarget, _op: &UiOp) -> Result<(), BackendError> {
        if let Some(err) = &self.invoke_error {
            return Err(BackendError::Internal(err.clone()));
        }
        Ok(())
    }

    async fn enumerate(&self, _t: &ResolvedTarget) -> Result<Vec<UiNode>, BackendError> {
        Ok(vec![UiNode {
            path: "stub-node".into(),
            name: "Stub Element".into(),
            control_type: "Button".into(),
        }])
    }
}
