use std::sync::Arc;

use koakuma_core::domain::{OnNoBackground, TargetSelector, UiOp};
use koakuma_core::permission::{Permission, PermissionGrant};

use crate::backend::{InteractionBackend, ResolvedTarget, Tier, UiNode};
use crate::error::DispatchError;

/// Registry of [`InteractionBackend`] implementations.
///
/// At dispatch time the registry:
/// 1. Calls `resolve` on every backend and collects `(backend, target, tier)` triples.
/// 2. Tries the highest-tier (`Background`) candidate first.
/// 3. If none found, applies the [`OnNoBackground`] policy.
pub struct BackendRegistry {
    backends: Vec<Arc<dyn InteractionBackend>>,
}

impl BackendRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self { backends: Vec::new() }
    }

    /// Registers a backend. Backends are tried in registration order within the
    /// same tier; higher tiers always win over lower tiers.
    pub fn register<B: InteractionBackend + 'static>(&mut self, backend: B) {
        self.backends.push(Arc::new(backend));
    }

    /// Dispatches `op` to `sel` applying `policy` when no background backend is found.
    ///
    /// Permission checks for `WindowInteraction` / `BrowserControl` are enforced by
    /// [`InteractAction`](crate::action::InteractAction).  This method only checks
    /// `ForegroundTakeover` when `policy == Degrade`.
    pub async fn dispatch(
        &self,
        sel: &TargetSelector,
        op: &UiOp,
        policy: OnNoBackground,
        permissions: &PermissionGrant,
    ) -> Result<(), DispatchError> {
        let candidates = self.collect_candidates(sel).await;

        // Best background-capable backend
        if let Some(idx) = candidates.iter().position(|(_, _, t)| *t >= Tier::Background) {
            let (backend, target, _) = &candidates[idx];
            return backend.invoke(target, op).await.map_err(DispatchError::Backend);
        }

        match policy {
            OnNoBackground::Fail => Err(DispatchError::NoBackend),
            OnNoBackground::Queue => Err(DispatchError::Queued),
            OnNoBackground::Degrade => {
                if !permissions.allows(&Permission::ForegroundTakeover) {
                    return Err(DispatchError::PermissionDenied(
                        "ForegroundTakeover permission required for Degrade fallback".into(),
                    ));
                }
                // Fall back to ForegroundSynthetic
                if let Some(idx) = candidates
                    .iter()
                    .position(|(_, _, t)| *t >= Tier::ForegroundSynthetic)
                {
                    let (backend, target, _) = &candidates[idx];
                    backend.invoke(target, op).await.map_err(DispatchError::Backend)
                } else {
                    Err(DispatchError::NoBackend)
                }
            }
        }
    }

    /// Enumerate UI nodes of the best available target for `sel`.
    pub async fn enumerate_nodes(&self, sel: &TargetSelector) -> Result<Vec<UiNode>, DispatchError> {
        let candidates = self.collect_candidates(sel).await;
        if let Some((backend, target, _)) = candidates.into_iter().max_by_key(|(_, _, t)| *t) {
            backend.enumerate(&target).await.map_err(DispatchError::Backend)
        } else {
            Ok(vec![])
        }
    }

    /// Collect all (backend, target, tier) triples for `sel`, sorted tier-descending.
    async fn collect_candidates(
        &self,
        sel: &TargetSelector,
    ) -> Vec<(Arc<dyn InteractionBackend>, ResolvedTarget, Tier)> {
        let mut candidates = Vec::new();
        for backend in &self.backends {
            if let Ok(targets) = backend.resolve(sel).await {
                for target in targets {
                    let tier = backend.capability(&target);
                    if tier > Tier::Unsupported {
                        candidates.push((Arc::clone(backend), target, tier));
                    }
                }
            }
        }
        // Highest tier first
        candidates.sort_by(|a, b| b.2.cmp(&a.2));
        candidates
    }
}

impl Default for BackendRegistry {
    fn default() -> Self {
        Self::new()
    }
}
