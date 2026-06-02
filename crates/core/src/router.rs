use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use crate::context::{EvalContext, ExecContext};
use crate::domain::{Macro, MacroId, TriggerConfig};
use crate::engine::{EngineEvent, EngineSnapshot};
use crate::event::{Event, EventKind};
use crate::permission::PermissionGrant;
use crate::registry::Registry;
use crate::state::StateStore;
use crate::traits::Outcome;
use crate::value::Value;

/// Event Router: the central dispatcher of the H→C→A pipeline.
///
/// Maintains a `HashMap<EventKind, Vec<MacroId>>` index so each incoming event
/// only visits the macros that subscribe to its kind — O(1) lookup instead of
/// broadcast over all macros.
pub struct EventRouter {
    /// EventKind → candidate macro IDs (maintained on add/remove).
    index:  HashMap<EventKind, Vec<MacroId>>,
    macros: HashMap<MacroId, Macro>,
}

impl EventRouter {
    /// Creates an empty router with no macros registered.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::router::EventRouter;
    ///
    /// let router = EventRouter::new();
    /// ```
    pub fn new() -> Self {
        Self { index: HashMap::new(), macros: HashMap::new() }
    }

    /// Registers a macro and updates the `EventKind` index for all its triggers.
    ///
    /// If a macro with the same `id` already exists it is silently replaced.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::router::EventRouter;
    /// use koakuma_core::domain::{Macro, ConstraintExpr, TriggerConfig};
    /// use koakuma_core::permission::PermissionSet;
    ///
    /// let mut router = EventRouter::new();
    /// let m = Macro {
    ///     id: uuid::Uuid::new_v4(),
    ///     name: "test".to_string(),
    ///     description: String::new(),
    ///     enabled: true,
    ///     category: None,
    ///     triggers: vec![TriggerConfig::Manual],
    ///     constraints: ConstraintExpr::Always,
    ///     actions: vec![],
    ///     granted_permissions: PermissionSet::default(),
    /// };
    /// router.add_macro(m);
    /// ```
    pub fn add_macro(&mut self, m: Macro) {
        for tc in &m.triggers {
            self.index.entry(event_kind_of(tc)).or_default().push(m.id);
        }
        self.macros.insert(m.id, m);
    }

    /// Unregisters a macro and cleans up its entries from the `EventKind` index.
    ///
    /// No-op if the macro ID is not registered.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::router::EventRouter;
    ///
    /// let mut router = EventRouter::new();
    /// let unknown_id = uuid::Uuid::nil();
    /// router.remove_macro(unknown_id); // no-op
    /// ```
    pub fn remove_macro(&mut self, id: MacroId) {
        if let Some(m) = self.macros.remove(&id) {
            for tc in &m.triggers {
                if let Some(v) = self.index.get_mut(&event_kind_of(tc)) {
                    v.retain(|mid| mid != &id);
                }
            }
        }
    }

    /// Enables or disables a macro without removing it from the index.
    ///
    /// Disabled macros remain in the index but are skipped during [`dispatch`](Self::dispatch).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::router::EventRouter;
    /// use koakuma_core::domain::{Macro, ConstraintExpr, TriggerConfig};
    /// use koakuma_core::permission::PermissionSet;
    ///
    /// let mut router = EventRouter::new();
    /// let id = uuid::Uuid::new_v4();
    /// let m = Macro {
    ///     id,
    ///     name: "test".to_string(),
    ///     description: String::new(),
    ///     enabled: true,
    ///     category: None,
    ///     triggers: vec![TriggerConfig::Manual],
    ///     constraints: ConstraintExpr::Always,
    ///     actions: vec![],
    ///     granted_permissions: PermissionSet::default(),
    /// };
    /// router.add_macro(m);
    /// router.set_enabled(id, false);
    /// ```
    pub fn set_enabled(&mut self, id: MacroId, enabled: bool) {
        if let Some(m) = self.macros.get_mut(&id) {
            m.enabled = enabled;
        }
    }

    /// Dispatch one event through the full Hook → Constraint → Action pipeline.
    ///
    /// Returns [`EngineEvent`]s (MacroFired, Error) for the caller to forward to
    /// the UI thread via `slint::invoke_from_event_loop` (wired in M1.3).
    pub fn dispatch(
        &self,
        event: &Event,
        registry: &Registry,
        store: &Arc<dyn StateStore>,
    ) -> Vec<EngineEvent> {
        let Some(candidates) = self.index.get(&event.kind) else {
            return vec![];
        };
        let mut output = Vec::new();
        for &macro_id in candidates {
            let Some(m) = self.macros.get(&macro_id) else { continue };
            if !m.enabled { continue; }

            // Fine-grained trigger matching (OR semantics across triggers list)
            let trigger_ok = m.triggers.iter().any(|tc| {
                registry
                    .build_trigger(tc)
                    .map(|spec| spec.matches(event))
                    .unwrap_or(false)
            });
            if !trigger_ok { continue; }

            output.extend(self.execute_pipeline(macro_id, m, event, registry, store));
        }
        output
    }

    /// Fires a specific macro by ID, bypassing trigger matching and evaluating only constraints.
    ///
    /// Used by the engine to implement [`EngineCommand::TriggerManually`](crate::engine::EngineCommand::TriggerManually).
    /// The synthesized event has kind [`EventKind::Manual`] and a `Null` payload.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::router::EventRouter;
    ///
    /// let router = EventRouter::new();
    /// let unknown_id = uuid::Uuid::nil();
    /// // Returns empty vec for unknown macro IDs.
    /// # use std::sync::Arc;
    /// # use koakuma_core::registry::Registry;
    /// # use koakuma_store::InMemoryStateStore;
    /// let results = router.dispatch_manual_trigger(
    ///     unknown_id,
    ///     &Registry::with_builtins(),
    ///     &(Arc::new(InMemoryStateStore::new()) as _),
    /// );
    /// assert!(results.is_empty());
    /// ```
    pub fn dispatch_manual_trigger(
        &self,
        macro_id: MacroId,
        registry: &Registry,
        store: &Arc<dyn StateStore>,
    ) -> Vec<EngineEvent> {
        let Some(m) = self.macros.get(&macro_id) else { return vec![]; };
        if !m.enabled { return vec![]; }
        let event = Event {
            kind: EventKind::Manual,
            source: "engine".to_string(),
            timestamp: SystemTime::now(),
            payload: Value::Null,
        };
        self.execute_pipeline(macro_id, m, &event, registry, store)
    }

    /// Returns a point-in-time snapshot of all registered macros.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use koakuma_core::router::EventRouter;
    ///
    /// let router = EventRouter::new();
    /// let snap = router.snapshot();
    /// assert!(snap.macros.is_empty());
    /// ```
    pub fn snapshot(&self) -> EngineSnapshot {
        EngineSnapshot {
            macros: self.macros.values().cloned().collect(),
        }
    }

    // ── Private pipeline helper ───────────────────────────────────────────────

    /// Evaluates constraints and executes actions for a single macro.
    ///
    /// Shared by [`dispatch`](Self::dispatch) (after trigger matching) and
    /// [`dispatch_manual_trigger`](Self::dispatch_manual_trigger) (trigger matching skipped).
    fn execute_pipeline(
        &self,
        macro_id: MacroId,
        m: &Macro,
        event: &Event,
        registry: &Registry,
        store: &Arc<dyn StateStore>,
    ) -> Vec<EngineEvent> {
        let mut output = Vec::new();

        // Constraint evaluation
        let eval_ctx = EvalContext { event, macro_id, store: store.as_ref() };
        match m.constraints.evaluate(&eval_ctx, registry) {
            Ok(true) => {}
            Ok(false) => return output,
            Err(e) => {
                output.push(EngineEvent::Error {
                    macro_id: Some(macro_id),
                    message: e.to_string(),
                });
                return output;
            }
        }

        // Macro fires
        output.push(EngineEvent::MacroFired { id: macro_id, name: m.name.clone(), at: event.timestamp });

        // Sequential action execution (V1); V2 upgrades to async Tokio tasks
        let mut exec_ctx = ExecContext {
            event: event.clone(),
            macro_id,
            locals: Default::default(),
            store: Arc::clone(store),
            permissions: PermissionGrant::from_set(&m.granted_permissions),
            cancel: Default::default(),
            log: Default::default(),
        };

        for action_cfg in &m.actions {
            match registry.build_action(action_cfg) {
                Ok(action) => {
                    // Central permission gate: verify all required permissions before execution.
                    let denied = action
                        .required_permissions()
                        .0
                        .iter()
                        .find(|p| !exec_ctx.permissions.allows(p))
                        .map(|p| format!("{p:?}"));
                    if let Some(name) = denied {
                        output.push(EngineEvent::Error {
                            macro_id: Some(macro_id),
                            message: format!("permission denied: {name}"),
                        });
                        break;
                    }
                    match action.execute(&mut exec_ctx) {
                        Ok(Outcome::Continue) => {}
                        Ok(Outcome::Stop) => break,
                        Err(e) => {
                            output.push(EngineEvent::Error {
                                macro_id: Some(macro_id),
                                message: e.to_string(),
                            });
                            break;
                        }
                    }
                }
                Err(e) => {
                    output.push(EngineEvent::Error {
                        macro_id: Some(macro_id),
                        message: e.to_string(),
                    });
                    break;
                }
            }
        }

        output
    }
}

impl Default for EventRouter {
    fn default() -> Self { Self::new() }
}

fn event_kind_of(tc: &TriggerConfig) -> EventKind {
    match tc {
        TriggerConfig::Hotkey { .. }      => EventKind::Hotkey,
        TriggerConfig::WindowFocus { .. } => EventKind::WindowFocus,
        TriggerConfig::Process { .. }     => EventKind::Process,
        TriggerConfig::Schedule { .. }    => EventKind::Timer,
        TriggerConfig::FileChange { .. }  => EventKind::FileChange,
        TriggerConfig::Manual             => EventKind::Manual,
        TriggerConfig::Custom { .. }      => EventKind::Custom,
    }
}
