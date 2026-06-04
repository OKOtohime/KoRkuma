/// Tests for `permission::aggregate_from_configs` and the central permission
/// enforcement gate added to `EventRouter::execute_pipeline` in M1.4.
///
/// All tests are zero-platform — no OS APIs are called.
use std::sync::Arc;

use async_trait::async_trait;
use korkuma_core::{
    context::ExecContext,
    domain::{ActionConfig, ConstraintExpr, Macro, ScriptLang, TriggerConfig},
    engine::EngineEvent,
    error::ActionError,
    permission::{Permission, PermissionSet, aggregate_from_configs},
    registry::Registry,
    router::EventRouter,
    state::StateStore,
    traits::{Action, Outcome},
};
use korkuma_store::InMemoryStateStore;

// ── aggregate_from_configs ────────────────────────────────────────────────────

#[test]
fn aggregate_empty_actions_yields_empty_set() {
    assert!(aggregate_from_configs(&[]).0.is_empty());
}

#[test]
fn aggregate_notify_yields_no_permissions() {
    let actions = [ActionConfig::Notify {
        title: "T".into(),
        body: "B".into(),
    }];
    assert!(aggregate_from_configs(&actions).0.is_empty());
}

#[test]
fn aggregate_delay_yields_no_permissions() {
    let actions = [ActionConfig::Delay { millis: 100 }];
    assert!(aggregate_from_configs(&actions).0.is_empty());
}

#[test]
fn aggregate_set_variable_yields_no_permissions() {
    use korkuma_core::domain::VarScope;
    use korkuma_core::value::Value;
    let actions = [ActionConfig::SetVariable {
        scope: VarScope::Global,
        key: "k".into(),
        value: Value::Null,
    }];
    assert!(aggregate_from_configs(&actions).0.is_empty());
}

#[test]
fn aggregate_run_command_yields_run_command_permission() {
    let actions = [ActionConfig::RunCommand {
        program: "echo".into(),
        args: vec![],
        capture: false,
    }];
    let set = aggregate_from_configs(&actions);
    assert_eq!(set.0, vec![Permission::RunCommand]);
}

#[test]
fn aggregate_simulate_input_yields_input_simulation() {
    let actions = [ActionConfig::SimulateInput { sequence: vec![] }];
    let set = aggregate_from_configs(&actions);
    assert_eq!(set.0, vec![Permission::InputSimulation]);
}

#[test]
fn aggregate_run_script_yields_script_execution() {
    let actions = [ActionConfig::RunScript {
        lang: ScriptLang::Rhai,
        source: String::new(),
    }];
    let set = aggregate_from_configs(&actions);
    assert_eq!(set.0, vec![Permission::ScriptExecution]);
}

#[test]
fn aggregate_http_request_yields_network() {
    let actions = [ActionConfig::HttpRequest {
        method: "GET".into(),
        url: "http://example.com".into(),
        body: None,
    }];
    let set = aggregate_from_configs(&actions);
    assert_eq!(set.0, vec![Permission::Network]);
}

#[test]
fn aggregate_deduplicates_repeated_same_action_type() {
    let actions = [
        ActionConfig::RunCommand {
            program: "a".into(),
            args: vec![],
            capture: false,
        },
        ActionConfig::RunCommand {
            program: "b".into(),
            args: vec![],
            capture: true,
        },
    ];
    let set = aggregate_from_configs(&actions);
    assert_eq!(
        set.0,
        vec![Permission::RunCommand],
        "two RunCommand → one permission"
    );
}

#[test]
fn aggregate_collects_multiple_distinct_permissions() {
    let actions = [
        ActionConfig::RunCommand {
            program: "a".into(),
            args: vec![],
            capture: false,
        },
        ActionConfig::SimulateInput { sequence: vec![] },
        ActionConfig::Notify {
            title: "T".into(),
            body: "B".into(),
        }, // no special permission
    ];
    let set = aggregate_from_configs(&actions);
    assert_eq!(set.0.len(), 2, "should have exactly 2 distinct permissions");
    assert!(set.0.contains(&Permission::RunCommand));
    assert!(set.0.contains(&Permission::InputSimulation));
}

// ── Central permission enforcement (EventRouter::execute_pipeline) ────────────

/// A test-only action that requires `Permission::Network` and records whether it ran.
struct PermissionedAction {
    ran: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait]
impl Action for PermissionedAction {
    fn id(&self) -> &'static str {
        "test_permissioned"
    }
    fn required_permissions(&self) -> PermissionSet {
        PermissionSet(vec![Permission::Network])
    }
    async fn execute(&self, _ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        self.ran.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(Outcome::Continue)
    }
}

fn make_permissioned_registry() -> (Registry, Arc<std::sync::atomic::AtomicBool>) {
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ran_clone = Arc::clone(&ran);
    let mut registry = Registry::with_builtins();
    registry.register_action(move |c| {
        if let ActionConfig::Custom { provider, .. } = c {
            if provider == "test_permissioned" {
                return Some(Box::new(PermissionedAction {
                    ran: Arc::clone(&ran_clone),
                }) as Box<dyn Action>);
            }
        }
        None
    });
    (registry, ran)
}

fn permissioned_macro(id: uuid::Uuid, perms: PermissionSet) -> Macro {
    Macro {
        id,
        name: "perm_test".into(),
        description: String::new(),
        enabled: true,
        category: None,
        triggers: vec![TriggerConfig::Manual],
        constraints: ConstraintExpr::Always,
        actions: vec![ActionConfig::Custom {
            provider: "test_permissioned".into(),
            params: serde_json::Value::Null,
        }],
        workflow: None,
        granted_permissions: perms,
        priority: 0,
        concurrency: Default::default(),
    }
}

#[tokio::test]
async fn central_check_blocks_action_when_permission_not_granted() {
    let (registry, ran) = make_permissioned_registry();
    let id = uuid::Uuid::new_v4();
    let m = permissioned_macro(id, PermissionSet::default()); // no Network

    let mut router = EventRouter::new();
    router.add_macro(m);
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());

    let events = router.dispatch_manual_trigger(id, &registry, &store).await;

    assert!(
        !ran.load(std::sync::atomic::Ordering::SeqCst),
        "action must not run"
    );
    let has_perm_error = events.iter().any(|e| {
        matches!(e, EngineEvent::Error { message, .. } if message.contains("permission denied"))
    });
    assert!(
        has_perm_error,
        "expected permission denied error; got: {events:?}"
    );
}

#[tokio::test]
async fn central_check_allows_action_when_permission_is_granted() {
    let (registry, ran) = make_permissioned_registry();
    let id = uuid::Uuid::new_v4();
    let m = permissioned_macro(id, PermissionSet(vec![Permission::Network])); // Network granted

    let mut router = EventRouter::new();
    router.add_macro(m);
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());

    let events = router.dispatch_manual_trigger(id, &registry, &store).await;

    assert!(
        ran.load(std::sync::atomic::Ordering::SeqCst),
        "action should have run"
    );
    let has_error = events
        .iter()
        .any(|e| matches!(e, EngineEvent::Error { .. }));
    assert!(!has_error, "should not produce an error; got: {events:?}");
}
