use std::sync::Arc;

use koakuma_core::context::{CancellationToken, ExecContext, LogHandle, ResourcePool};
use koakuma_core::domain::{ActionConfig, OnNoBackground, TargetSelector, UiOp};
use koakuma_core::event::{Event, EventKind};
use koakuma_core::permission::{Permission, PermissionGrant};
use koakuma_core::registry::Registry;
use koakuma_core::value::Value;
use koakuma_interact::backend::Tier;
use koakuma_interact::backends::StubBackend;
use koakuma_interact::{BackendRegistry, register_actions};
use koakuma_store::InMemoryStateStore;
use std::time::SystemTime;

// ── helpers ──────────────────────────────────────────────────────────────────

fn registry_with(backend_registry: Arc<BackendRegistry>) -> Registry {
    let mut reg = Registry::with_builtins();
    register_actions(&mut reg, backend_registry);
    reg
}

fn make_ctx(permissions: Vec<Permission>) -> ExecContext {
    ExecContext {
        event: Event {
            kind: EventKind::Manual,
            source: "test".into(),
            timestamp: SystemTime::UNIX_EPOCH,
            payload: Value::Null,
        },
        macro_id: uuid::Uuid::nil(),
        locals: Default::default(),
        store: Arc::new(InMemoryStateStore::new()),
        permissions: PermissionGrant::new(permissions),
        cancel: CancellationToken::new(),
        log: LogHandle::default(),
        resource_pool: ResourcePool::new(),
    }
}

fn click_cfg(on_no_background: OnNoBackground) -> ActionConfig {
    ActionConfig::Interact {
        target: TargetSelector::Window {
            title_pattern: "Notepad".into(),
            regex: false,
        },
        op: UiOp::Click { node: "name:New".into() },
        on_no_background,
    }
}

// ── Background dispatch ───────────────────────────────────────────────────────

#[tokio::test]
async fn background_dispatch_succeeds() {
    let mut breg = BackendRegistry::new();
    breg.register(StubBackend::new(Tier::Background));
    let breg = Arc::new(breg);

    let reg = registry_with(Arc::clone(&breg));
    let cfg = click_cfg(OnNoBackground::Fail); // Fail policy; should succeed without needing it
    let action = reg.build_action(&cfg).expect("build action");

    let mut ctx = make_ctx(vec![Permission::WindowInteraction]);
    let result = action.execute(&mut ctx).await;
    assert!(result.is_ok(), "background dispatch should succeed: {result:?}");
}

// ── Fail policy ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn fail_policy_when_no_background() {
    // Backend resolves targets but is Unsupported (empty resolve)
    let mut breg = BackendRegistry::new();
    breg.register(StubBackend::new(Tier::Unsupported));
    let breg = Arc::new(breg);

    let reg = registry_with(Arc::clone(&breg));
    let cfg = click_cfg(OnNoBackground::Fail);
    let action = reg.build_action(&cfg).expect("build action");

    let mut ctx = make_ctx(vec![Permission::WindowInteraction]);
    let result = action.execute(&mut ctx).await;
    assert!(result.is_err(), "Fail policy must return error when no background");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("no interaction backend") || err.contains("action failed"),
        "unexpected error: {err}"
    );
}

// ── Degrade policy ────────────────────────────────────────────────────────────

#[tokio::test]
async fn degrade_policy_with_permission_uses_fg_synthetic() {
    let mut breg = BackendRegistry::new();
    // Only ForegroundSynthetic backend available
    breg.register(StubBackend::new(Tier::ForegroundSynthetic));
    let breg = Arc::new(breg);

    let reg = registry_with(Arc::clone(&breg));
    let cfg = click_cfg(OnNoBackground::Degrade);
    let action = reg.build_action(&cfg).expect("build action");

    // Has both WindowInteraction + ForegroundTakeover
    let mut ctx = make_ctx(vec![Permission::WindowInteraction, Permission::ForegroundTakeover]);
    let result = action.execute(&mut ctx).await;
    assert!(
        result.is_ok(),
        "Degrade with ForegroundTakeover permission should succeed: {result:?}"
    );
}

#[tokio::test]
async fn degrade_policy_without_foreground_takeover_is_denied() {
    let mut breg = BackendRegistry::new();
    breg.register(StubBackend::new(Tier::ForegroundSynthetic));
    let breg = Arc::new(breg);

    let reg = registry_with(Arc::clone(&breg));
    let cfg = click_cfg(OnNoBackground::Degrade);
    let action = reg.build_action(&cfg).expect("build action");

    // Missing ForegroundTakeover
    let mut ctx = make_ctx(vec![Permission::WindowInteraction]);
    let result = action.execute(&mut ctx).await;
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("ForegroundTakeover") || err.contains("permission denied"),
        "should mention ForegroundTakeover: {err}"
    );
}

// ── Queue policy ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn queue_policy_returns_queued_error() {
    let mut breg = BackendRegistry::new();
    breg.register(StubBackend::new(Tier::Unsupported));
    let breg = Arc::new(breg);

    let reg = registry_with(Arc::clone(&breg));
    let cfg = click_cfg(OnNoBackground::Queue);
    let action = reg.build_action(&cfg).expect("build action");

    let mut ctx = make_ctx(vec![Permission::WindowInteraction]);
    let result = action.execute(&mut ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("queued") || err.contains("retry"),
        "should signal queuing: {err}"
    );
}

// ── Browser target ────────────────────────────────────────────────────────────

#[tokio::test]
async fn browser_tab_dispatch_succeeds() {
    let mut breg = BackendRegistry::new();
    breg.register(StubBackend::new(Tier::Background));
    let breg = Arc::new(breg);

    let reg = registry_with(Arc::clone(&breg));
    let cfg = ActionConfig::Interact {
        target: TargetSelector::BrowserTab { url_pattern: "github.com".into() },
        op: UiOp::Click { node: "#submit".into() },
        on_no_background: OnNoBackground::Fail,
    };
    let action = reg.build_action(&cfg).expect("build action");

    let mut ctx = make_ctx(vec![Permission::BrowserControl]);
    let result = action.execute(&mut ctx).await;
    assert!(result.is_ok(), "browser dispatch should succeed: {result:?}");
}

// ── Permission aggregation ────────────────────────────────────────────────────

#[test]
fn interact_aggregate_window_permissions() {
    use koakuma_core::permission::aggregate_from_configs;

    let actions = vec![click_cfg(OnNoBackground::Fail)];
    let set = aggregate_from_configs(&actions);
    assert!(set.0.contains(&Permission::WindowInteraction));
    assert!(!set.0.contains(&Permission::ForegroundTakeover));
    assert!(!set.0.contains(&Permission::BrowserControl));
}

#[test]
fn interact_aggregate_degrade_adds_takeover() {
    use koakuma_core::permission::aggregate_from_configs;

    let actions = vec![click_cfg(OnNoBackground::Degrade)];
    let set = aggregate_from_configs(&actions);
    assert!(set.0.contains(&Permission::WindowInteraction));
    assert!(set.0.contains(&Permission::ForegroundTakeover));
}

#[test]
fn interact_aggregate_browser_permissions() {
    use koakuma_core::permission::aggregate_from_configs;

    let actions = vec![ActionConfig::Interact {
        target: TargetSelector::BrowserTab { url_pattern: "example.com".into() },
        op: UiOp::Click { node: "#btn".into() },
        on_no_background: OnNoBackground::Fail,
    }];
    let set = aggregate_from_configs(&actions);
    assert!(set.0.contains(&Permission::BrowserControl));
    assert!(!set.0.contains(&Permission::WindowInteraction));
}

// ── Tier ordering ─────────────────────────────────────────────────────────────

#[test]
fn tier_ordering() {
    assert!(Tier::Background > Tier::ForegroundSynthetic);
    assert!(Tier::ForegroundSynthetic > Tier::Unsupported);
    assert!(Tier::Background > Tier::Unsupported);
}

// ── Negotiation with multiple backends ───────────────────────────────────────

#[tokio::test]
async fn negotiator_picks_highest_tier() {
    // Two backends: ForegroundSynthetic and Background
    // Expect Background to be used (stub succeeds silently)
    let mut breg = BackendRegistry::new();
    breg.register(StubBackend::failing(Tier::ForegroundSynthetic, "fg invoked"));
    breg.register(StubBackend::new(Tier::Background));
    let breg = Arc::new(breg);

    let sel = TargetSelector::Window { title_pattern: "test".into(), regex: false };
    let op = UiOp::Click { node: String::new() };
    let permissions = PermissionGrant::new(vec![
        Permission::WindowInteraction,
        Permission::ForegroundTakeover,
    ]);

    let result = breg
        .dispatch(&sel, &op, OnNoBackground::Fail, &permissions)
        .await;
    assert!(result.is_ok(), "background backend should be picked: {result:?}");
}

// ── Stub enumerate ────────────────────────────────────────────────────────────

#[tokio::test]
async fn stub_enumerate_returns_node() {
    let mut breg = BackendRegistry::new();
    breg.register(StubBackend::new(Tier::Background));

    let sel = TargetSelector::Foreground;
    let nodes = breg.enumerate_nodes(&sel).await.expect("enumerate");
    assert!(!nodes.is_empty());
    assert_eq!(nodes[0].control_type, "Button");
}
