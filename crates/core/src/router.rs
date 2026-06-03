use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use crate::context::{CancellationToken, EvalContext, ExecContext, LogHandle, ResourcePool};
use crate::domain::{Macro, MacroId, TriggerConfig};
use crate::engine::{EngineEvent, EngineSnapshot};
use crate::event::{Event, EventKind};
use crate::permission::PermissionGrant;
use crate::registry::Registry;
use crate::scheduler::WorkflowScheduler;
use crate::state::StateStore;
use crate::value::Value;
use crate::workflow::run_workflow;

/// Event Router: the central dispatcher of the H→C→A pipeline.
///
/// Maintains a `HashMap<EventKind, Vec<MacroId>>` index so each incoming event
/// only visits the macros that subscribe to its kind — O(1) lookup instead of
/// broadcast over all macros.
pub struct EventRouter {
    /// EventKind → candidate macro IDs (maintained on add/remove).
    index: HashMap<EventKind, Vec<MacroId>>,
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
        Self {
            index: HashMap::new(),
            macros: HashMap::new(),
        }
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
    ///     workflow: None,
    ///     granted_permissions: PermissionSet::default(),
    ///     priority: 0,
    ///     concurrency: Default::default(),
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
    ///     workflow: None,
    ///     granted_permissions: PermissionSet::default(),
    ///     priority: 0,
    ///     concurrency: Default::default(),
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
    /// Async since M2.1: matching and constraint evaluation are synchronous, but the
    /// Action leg runs as an async [`workflow`](crate::workflow) tree. The engine drives
    /// this future with `Runtime::block_on`. Returns [`EngineEvent`]s (MacroFired, Error)
    /// for the caller to forward to the UI thread.
    ///
    /// Macros are evaluated in descending `priority` order within this event.
    pub async fn dispatch(
        &self,
        event: &Event,
        registry: &Registry,
        store: &Arc<dyn StateStore>,
    ) -> Vec<EngineEvent> {
        let Some(candidates) = self.index.get(&event.kind) else {
            return vec![];
        };

        // Collect enabled candidates with their priorities, then sort high→low.
        let mut ordered: Vec<(i32, MacroId)> = candidates
            .iter()
            .filter_map(|&id| {
                let m = self.macros.get(&id)?;
                if m.enabled { Some((m.priority, id)) } else { None }
            })
            .collect();
        ordered.sort_unstable_by(|a, b| b.0.cmp(&a.0));

        let mut output = Vec::new();
        for (_, macro_id) in ordered {
            let Some(m) = self.macros.get(&macro_id) else {
                continue;
            };

            // Fine-grained trigger matching (OR semantics across triggers list)
            let trigger_ok = m.triggers.iter().any(|tc| {
                registry
                    .build_trigger(tc)
                    .map(|spec| spec.matches(event))
                    .unwrap_or(false)
            });
            if !trigger_ok {
                continue;
            }

            output.extend(
                self.execute_pipeline(macro_id, m, event, registry, store)
                    .await,
            );
        }
        output
    }

    /// Synchronous scheduled dispatch: identify fired macros (with priority sort),
    /// emit `MacroFired` events, and hand workflow execution to the `scheduler`.
    ///
    /// Unlike [`dispatch`](Self::dispatch), this method returns immediately after
    /// handing off to the scheduler. Workflow [`EngineEvent`]s arrive asynchronously
    /// via the channel the scheduler was constructed with.
    ///
    /// Used by the engine loop in M2.2; the existing `dispatch` remains available
    /// for inline execution (backward-compatible tests and manual triggers).
    pub fn dispatch_scheduled(
        &self,
        event: &Event,
        registry: &Arc<Registry>,
        store: &Arc<dyn StateStore>,
        scheduler: &WorkflowScheduler,
    ) -> Vec<EngineEvent> {
        let Some(candidates) = self.index.get(&event.kind) else {
            return vec![];
        };

        let mut output = Vec::new();
        let mut to_fire: Vec<(i32, MacroId)> = Vec::new();

        for &macro_id in candidates {
            let Some(m) = self.macros.get(&macro_id) else {
                continue;
            };
            if !m.enabled {
                continue;
            }

            let trigger_ok = m.triggers.iter().any(|tc| {
                registry
                    .build_trigger(tc)
                    .map(|spec| spec.matches(event))
                    .unwrap_or(false)
            });
            if !trigger_ok {
                continue;
            }

            let eval_ctx = EvalContext {
                event,
                macro_id,
                store: store.as_ref(),
            };
            match m.constraints.evaluate(&eval_ctx, registry) {
                Ok(true) => to_fire.push((m.priority, macro_id)),
                Ok(false) => {}
                Err(e) => output.push(EngineEvent::Error {
                    macro_id: Some(macro_id),
                    message: e.to_string(),
                }),
            }
        }

        // Dispatch in descending priority order.
        to_fire.sort_unstable_by(|a, b| b.0.cmp(&a.0));

        for (_, macro_id) in to_fire {
            let m = &self.macros[&macro_id];
            output.push(EngineEvent::MacroFired {
                id: macro_id,
                name: m.name.clone(),
                at: event.timestamp,
            });

            let ctx = ExecContext {
                event: event.clone(),
                macro_id,
                locals: Default::default(),
                store: Arc::clone(store),
                permissions: PermissionGrant::from_set(&m.granted_permissions),
                cancel: CancellationToken::new(),
                log: LogHandle,
                resource_pool: scheduler.resource_pool().clone(),
            };

            scheduler.schedule(macro_id, &m.concurrency, ctx, m.root_workflow(), Arc::clone(registry));
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
    /// # use std::sync::Arc;
    /// # use koakuma_core::registry::Registry;
    /// # use koakuma_store::InMemoryStateStore;
    /// # let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    /// # rt.block_on(async {
    /// let router = EventRouter::new();
    /// let unknown_id = uuid::Uuid::nil();
    /// // Returns empty vec for unknown macro IDs.
    /// let results = router.dispatch_manual_trigger(
    ///     unknown_id,
    ///     &Registry::with_builtins(),
    ///     &(Arc::new(InMemoryStateStore::new()) as _),
    /// ).await;
    /// assert!(results.is_empty());
    /// # });
    /// ```
    pub async fn dispatch_manual_trigger(
        &self,
        macro_id: MacroId,
        registry: &Registry,
        store: &Arc<dyn StateStore>,
    ) -> Vec<EngineEvent> {
        let Some(m) = self.macros.get(&macro_id) else {
            return vec![];
        };
        if !m.enabled {
            return vec![];
        }
        let event = Event {
            kind: EventKind::Manual,
            source: "engine".to_string(),
            timestamp: SystemTime::now(),
            payload: Value::Null,
        };
        self.execute_pipeline(macro_id, m, &event, registry, store)
            .await
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

    /// Evaluates constraints and runs the macro's workflow.
    ///
    /// Shared by [`dispatch`](Self::dispatch) (after trigger matching) and
    /// [`dispatch_manual_trigger`](Self::dispatch_manual_trigger) (trigger matching skipped).
    /// The Action leg is delegated to [`workflow::run_workflow`](crate::workflow::run_workflow),
    /// which interprets the macro's [`WorkflowNode`](crate::domain::WorkflowNode) tree
    /// (or, for V1 configs, the flat action list wrapped in a `Seq`).
    async fn execute_pipeline(
        &self,
        macro_id: MacroId,
        m: &Macro,
        event: &Event,
        registry: &Registry,
        store: &Arc<dyn StateStore>,
    ) -> Vec<EngineEvent> {
        let mut output = Vec::new();

        // Constraint evaluation
        let eval_ctx = EvalContext {
            event,
            macro_id,
            store: store.as_ref(),
        };
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
        output.push(EngineEvent::MacroFired {
            id: macro_id,
            name: m.name.clone(),
            at: event.timestamp,
        });

        // Async workflow execution: permission gating and control flow live in the
        // workflow engine. A V1 macro (no `workflow`) runs as a sequential action list.
        let mut exec_ctx = ExecContext {
            event: event.clone(),
            macro_id,
            locals: Default::default(),
            store: Arc::clone(store),
            permissions: PermissionGrant::from_set(&m.granted_permissions),
            cancel: Default::default(),
            log: LogHandle,
            resource_pool: ResourcePool::default(),
        };

        let root = m.root_workflow();
        output.extend(run_workflow(&root, &mut exec_ctx, registry).await);

        output
    }
}

impl Default for EventRouter {
    fn default() -> Self {
        Self::new()
    }
}

fn event_kind_of(tc: &TriggerConfig) -> EventKind {
    match tc {
        TriggerConfig::Hotkey { .. } => EventKind::Hotkey,
        TriggerConfig::WindowFocus { .. } => EventKind::WindowFocus,
        TriggerConfig::Process { .. } => EventKind::Process,
        TriggerConfig::Schedule { .. } => EventKind::Timer,
        TriggerConfig::FileChange { .. } => EventKind::FileChange,
        TriggerConfig::Manual => EventKind::Manual,
        TriggerConfig::Custom { .. } => EventKind::Custom,
    }
}
