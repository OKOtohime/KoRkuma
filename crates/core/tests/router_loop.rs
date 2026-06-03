/// M1.1 integration tests: full Hook → Constraint → Action pipeline via EventRouter.
///
/// Uses a mock recording action registered through the `Custom` provider slot —
/// zero platform dependencies.
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use async_trait::async_trait;
use koakuma_core::{
    context::ExecContext,
    domain::{ActionConfig, ConstraintExpr, Macro, TriggerConfig, VarScope},
    engine::EngineEvent,
    error::ActionError,
    event::{Event, EventKind},
    permission::PermissionSet,
    registry::Registry,
    router::EventRouter,
    state::StateStore,
    traits::{Action, Outcome},
    value::Value,
};
use koakuma_store::InMemoryStateStore;

// ── helpers ────────────────────────────────────────────────────────────────

fn manual_event() -> Event {
    Event {
        kind: EventKind::Manual,
        source: "test".into(),
        timestamp: SystemTime::now(),
        payload: Value::Null,
    }
}

/// Counts how many times the action was executed.
struct CountingAction(Arc<Mutex<u32>>);

#[async_trait]
impl Action for CountingAction {
    fn id(&self) -> &'static str {
        "counting"
    }
    fn required_permissions(&self) -> PermissionSet {
        PermissionSet::default()
    }
    async fn execute(&self, _ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        *self.0.lock().unwrap() += 1;
        Ok(Outcome::Continue)
    }
}

/// Stops after first call.
struct StopAction;
#[async_trait]
impl Action for StopAction {
    fn id(&self) -> &'static str {
        "stop"
    }
    fn required_permissions(&self) -> PermissionSet {
        PermissionSet::default()
    }
    async fn execute(&self, _ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        Ok(Outcome::Stop)
    }
}

fn register_counting(registry: &mut Registry, counter: Arc<Mutex<u32>>, provider: &'static str) {
    registry.register_action(move |c| {
        if let ActionConfig::Custom { provider: p, .. } = c {
            if p == provider {
                return Some(Box::new(CountingAction(Arc::clone(&counter))) as Box<dyn Action>);
            }
        }
        None
    });
}

fn make_macro(
    id: uuid::Uuid,
    triggers: Vec<TriggerConfig>,
    constraints: ConstraintExpr,
    actions: Vec<ActionConfig>,
    enabled: bool,
) -> Macro {
    Macro {
        id,
        name: "test".into(),
        description: "".into(),
        enabled,
        category: None,
        triggers,
        constraints,
        actions,
        workflow: None,
        granted_permissions: PermissionSet::default(),
        priority: 0,
        concurrency: Default::default(),
    }
}

// ── core pipeline ──────────────────────────────────────────────────────────

#[tokio::test]
async fn manual_trigger_always_fires_action() {
    let counter = Arc::new(Mutex::new(0u32));
    let mut registry = Registry::with_builtins();
    register_counting(&mut registry, Arc::clone(&counter), "counting_a");

    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());

    let m = make_macro(
        uuid::Uuid::new_v4(),
        vec![TriggerConfig::Manual],
        ConstraintExpr::Always,
        vec![ActionConfig::Custom {
            provider: "counting_a".into(),
            params: serde_json::Value::Null,
        }],
        true,
    );
    let mut router = EventRouter::new();
    router.add_macro(m.clone());

    let events = router.dispatch(&manual_event(), &registry, &store).await;

    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::MacroFired { id, .. } if *id == m.id)),
        "expected MacroFired"
    );
    assert_eq!(*counter.lock().unwrap(), 1, "action should run once");
}

#[tokio::test]
async fn macro_fires_only_for_its_event_kind() {
    let counter = Arc::new(Mutex::new(0u32));
    let mut registry = Registry::with_builtins();
    register_counting(&mut registry, Arc::clone(&counter), "kind_check");

    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());

    // Macro subscribes to Manual only
    let m = make_macro(
        uuid::Uuid::new_v4(),
        vec![TriggerConfig::Manual],
        ConstraintExpr::Always,
        vec![ActionConfig::Custom {
            provider: "kind_check".into(),
            params: serde_json::Value::Null,
        }],
        true,
    );
    let mut router = EventRouter::new();
    router.add_macro(m);

    // Dispatch a non-Manual event (Hotkey); router index should skip this macro
    let hotkey_event = Event {
        kind: EventKind::Hotkey,
        source: "test".into(),
        timestamp: SystemTime::now(),
        payload: Value::Null,
    };
    let events = router.dispatch(&hotkey_event, &registry, &store).await;

    assert!(events.is_empty(), "no events for wrong kind");
    assert_eq!(*counter.lock().unwrap(), 0);
}

// ── disabled macro ─────────────────────────────────────────────────────────

#[tokio::test]
async fn disabled_macro_does_not_fire() {
    let counter = Arc::new(Mutex::new(0u32));
    let mut registry = Registry::with_builtins();
    register_counting(&mut registry, Arc::clone(&counter), "disabled_check");

    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());

    let m = make_macro(
        uuid::Uuid::new_v4(),
        vec![TriggerConfig::Manual],
        ConstraintExpr::Always,
        vec![ActionConfig::Custom {
            provider: "disabled_check".into(),
            params: serde_json::Value::Null,
        }],
        false, // disabled
    );
    let mut router = EventRouter::new();
    router.add_macro(m);

    let events = router.dispatch(&manual_event(), &registry, &store).await;

    assert!(events.is_empty());
    assert_eq!(*counter.lock().unwrap(), 0);
}

#[tokio::test]
async fn set_enabled_toggles_firing() {
    let counter = Arc::new(Mutex::new(0u32));
    let mut registry = Registry::with_builtins();
    register_counting(&mut registry, Arc::clone(&counter), "toggle_check");

    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());

    let id = uuid::Uuid::new_v4();
    let m = make_macro(
        id,
        vec![TriggerConfig::Manual],
        ConstraintExpr::Always,
        vec![ActionConfig::Custom {
            provider: "toggle_check".into(),
            params: serde_json::Value::Null,
        }],
        false, // starts disabled
    );
    let mut router = EventRouter::new();
    router.add_macro(m);

    // Disabled → no fire
    router.dispatch(&manual_event(), &registry, &store).await;
    assert_eq!(*counter.lock().unwrap(), 0);

    // Enable → fires
    router.set_enabled(id, true);
    router.dispatch(&manual_event(), &registry, &store).await;
    assert_eq!(*counter.lock().unwrap(), 1);
}

// ── constraint evaluation ──────────────────────────────────────────────────

#[tokio::test]
async fn constraint_false_blocks_action() {
    let counter = Arc::new(Mutex::new(0u32));
    let mut registry = Registry::with_builtins();
    register_counting(&mut registry, Arc::clone(&counter), "blocked_check");

    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());

    // Not(Always) = always false
    let m = make_macro(
        uuid::Uuid::new_v4(),
        vec![TriggerConfig::Manual],
        ConstraintExpr::Not {
            expr: Box::new(ConstraintExpr::Always),
        },
        vec![ActionConfig::Custom {
            provider: "blocked_check".into(),
            params: serde_json::Value::Null,
        }],
        true,
    );
    let mut router = EventRouter::new();
    router.add_macro(m);

    let events = router.dispatch(&manual_event(), &registry, &store).await;

    assert!(events.is_empty(), "constraint false → macro must not fire");
    assert_eq!(*counter.lock().unwrap(), 0);
}

// ── action semantics ───────────────────────────────────────────────────────

#[tokio::test]
async fn stop_outcome_halts_action_chain() {
    let counter = Arc::new(Mutex::new(0u32));
    let mut registry = Registry::with_builtins();

    // First action: Stop
    registry.register_action(|c| {
        if let ActionConfig::Custom { provider, .. } = c {
            if provider == "stop_action" {
                return Some(Box::new(StopAction) as Box<dyn Action>);
            }
        }
        None
    });
    let c2 = Arc::clone(&counter);
    register_counting(&mut registry, c2, "after_stop");

    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());

    let m = make_macro(
        uuid::Uuid::new_v4(),
        vec![TriggerConfig::Manual],
        ConstraintExpr::Always,
        vec![
            ActionConfig::Custom {
                provider: "stop_action".into(),
                params: serde_json::Value::Null,
            },
            ActionConfig::Custom {
                provider: "after_stop".into(),
                params: serde_json::Value::Null,
            },
        ],
        true,
    );
    let mut router = EventRouter::new();
    router.add_macro(m);

    router.dispatch(&manual_event(), &registry, &store).await;

    assert_eq!(
        *counter.lock().unwrap(),
        0,
        "second action must not run after Stop"
    );
}

#[tokio::test]
async fn set_variable_action_writes_to_store() {
    let registry = Registry::with_builtins();
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());

    let m = make_macro(
        uuid::Uuid::new_v4(),
        vec![TriggerConfig::Manual],
        ConstraintExpr::Always,
        vec![ActionConfig::SetVariable {
            scope: VarScope::Global,
            key: "answer".into(),
            value: Value::Int(42),
        }],
        true,
    );
    let mut router = EventRouter::new();
    router.add_macro(m);

    router.dispatch(&manual_event(), &registry, &store).await;

    assert_eq!(store.get("answer"), Some(Value::Int(42)));
}

// ── multiple macros ────────────────────────────────────────────────────────

#[tokio::test]
async fn multiple_macros_both_fire_independently() {
    let c1 = Arc::new(Mutex::new(0u32));
    let c2 = Arc::new(Mutex::new(0u32));
    let mut registry = Registry::with_builtins();
    register_counting(&mut registry, Arc::clone(&c1), "macro1");
    register_counting(&mut registry, Arc::clone(&c2), "macro2");

    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());

    let m1 = make_macro(
        uuid::Uuid::new_v4(),
        vec![TriggerConfig::Manual],
        ConstraintExpr::Always,
        vec![ActionConfig::Custom {
            provider: "macro1".into(),
            params: serde_json::Value::Null,
        }],
        true,
    );
    let m2 = make_macro(
        uuid::Uuid::new_v4(),
        vec![TriggerConfig::Manual],
        ConstraintExpr::Always,
        vec![ActionConfig::Custom {
            provider: "macro2".into(),
            params: serde_json::Value::Null,
        }],
        true,
    );

    let mut router = EventRouter::new();
    router.add_macro(m1);
    router.add_macro(m2);

    let events = router.dispatch(&manual_event(), &registry, &store).await;

    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, EngineEvent::MacroFired { .. }))
            .count(),
        2
    );
    assert_eq!(*c1.lock().unwrap(), 1);
    assert_eq!(*c2.lock().unwrap(), 1);
}

// ── remove macro ───────────────────────────────────────────────────────────

#[tokio::test]
async fn removed_macro_does_not_fire() {
    let counter = Arc::new(Mutex::new(0u32));
    let mut registry = Registry::with_builtins();
    register_counting(&mut registry, Arc::clone(&counter), "remove_check");

    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());

    let id = uuid::Uuid::new_v4();
    let m = make_macro(
        id,
        vec![TriggerConfig::Manual],
        ConstraintExpr::Always,
        vec![ActionConfig::Custom {
            provider: "remove_check".into(),
            params: serde_json::Value::Null,
        }],
        true,
    );
    let mut router = EventRouter::new();
    router.add_macro(m);
    router.remove_macro(id);

    let events = router.dispatch(&manual_event(), &registry, &store).await;

    assert!(events.is_empty());
    assert_eq!(*counter.lock().unwrap(), 0);
}
