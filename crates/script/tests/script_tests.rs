/// M1.4 integration tests for `korkuma-script`.
///
/// Covers `RunScriptAction` (permission gate, get_var/set_var/log, sandbox) and
/// `ExpressionConstraint` (bool evaluation, get_var, resource limits).
/// Uses `InMemoryStateStore` — no OS APIs called.
use std::sync::Arc;
use std::time::SystemTime;

use korkuma_core::{
    context::{CancellationToken, EvalContext, ExecContext, LogHandle},
    domain::{ActionConfig, ConstraintConfig, ScriptLang},
    error::{ActionError, ConstraintError},
    event::{Event, EventKind},
    permission::{Permission, PermissionGrant},
    state::StateStore,
    traits::{Action, Constraint, Outcome},
    value::Value,
};
use korkuma_script::{build_expression, build_run_script};
use korkuma_store::InMemoryStateStore;

// ── Shared helpers ────────────────────────────────────────────────────────────

fn null_event() -> Event {
    Event {
        kind: EventKind::Manual,
        source: "test".into(),
        timestamp: SystemTime::UNIX_EPOCH,
        payload: Value::Null,
    }
}

fn store() -> Arc<InMemoryStateStore> {
    Arc::new(InMemoryStateStore::new())
}

fn exec_ctx(s: Arc<InMemoryStateStore>, perms: Vec<Permission>) -> ExecContext {
    ExecContext {
        event: null_event(),
        macro_id: uuid::Uuid::nil(),
        locals: Default::default(),
        store: s as Arc<dyn StateStore>,
        permissions: PermissionGrant::new(perms),
        cancel: CancellationToken::default(),
        log: LogHandle::default(),
        resource_pool: Default::default(),
        dry_run: false,
    }
}

fn eval_ctx<'a>(event: &'a Event, s: &'a InMemoryStateStore) -> EvalContext<'a> {
    EvalContext {
        event,
        macro_id: uuid::Uuid::nil(),
        store: s,
    }
}

fn script_action(source: &str) -> Box<dyn Action> {
    build_run_script(&ActionConfig::RunScript {
        lang: ScriptLang::Rhai,
        source: source.into(),
    })
    .expect("build_run_script should succeed for valid config")
}

fn expr_constraint(dsl: &str) -> Box<dyn Constraint> {
    build_expression(&ConstraintConfig::Expression { dsl: dsl.into() })
        .expect("build_expression should succeed for valid config")
}

// ── RunScriptAction — permission gate ─────────────────────────────────────────

#[tokio::test]
async fn run_script_without_permission_returns_denied() {
    let mut ctx = exec_ctx(store(), vec![]); // no ScriptExecution
    let result = script_action("let x = 1;").execute(&mut ctx).await;
    assert!(
        matches!(result, Err(ActionError::PermissionDenied(_))),
        "expected PermissionDenied, got: {result:?}"
    );
}

#[tokio::test]
async fn run_script_with_permission_succeeds() {
    let mut ctx = exec_ctx(store(), vec![Permission::ScriptExecution]);
    let result = script_action("let x = 1 + 1;").execute(&mut ctx).await;
    assert!(
        matches!(result, Ok(Outcome::Continue)),
        "expected Continue, got: {result:?}"
    );
}

// ── RunScriptAction — host functions ─────────────────────────────────────────

#[tokio::test]
async fn run_script_set_var_writes_int_to_store() {
    let s = store();
    let mut ctx = exec_ctx(Arc::clone(&s), vec![Permission::ScriptExecution]);
    script_action(r#"set_var("answer", 42)"#)
        .execute(&mut ctx)
        .await
        .unwrap();
    assert_eq!(s.get("answer"), Some(Value::Int(42)));
}

#[tokio::test]
async fn run_script_set_var_writes_string_to_store() {
    let s = store();
    let mut ctx = exec_ctx(Arc::clone(&s), vec![Permission::ScriptExecution]);
    script_action(r#"set_var("mode", "active")"#)
        .execute(&mut ctx)
        .await
        .unwrap();
    assert_eq!(s.get("mode"), Some(Value::Str("active".into())));
}

#[tokio::test]
async fn run_script_get_var_reads_existing_value() {
    let s = store();
    s.set("counter", Value::Int(7));
    let mut ctx = exec_ctx(Arc::clone(&s), vec![Permission::ScriptExecution]);
    script_action(r#"let v = get_var("counter"); set_var("doubled", v * 2)"#)
        .execute(&mut ctx)
        .await
        .unwrap();
    assert_eq!(s.get("doubled"), Some(Value::Int(14)));
}

#[tokio::test]
async fn run_script_get_var_missing_key_returns_unit_without_crashing() {
    let s = store();
    let mut ctx = exec_ctx(Arc::clone(&s), vec![Permission::ScriptExecution]);
    // Accessing a missing key returns () — script should not panic or error.
    script_action(r#"let v = get_var("no_such_key"); set_var("was_unit", v == ())"#)
        .execute(&mut ctx)
        .await
        .unwrap();
    assert_eq!(s.get("was_unit"), Some(Value::Bool(true)));
}

#[tokio::test]
async fn run_script_set_var_writes_are_applied_after_script_exits() {
    let s = store();
    let mut ctx = exec_ctx(Arc::clone(&s), vec![Permission::ScriptExecution]);
    // Two set_var calls in sequence — both should appear in the store.
    script_action(r#"set_var("a", 1); set_var("b", 2)"#)
        .execute(&mut ctx)
        .await
        .unwrap();
    assert_eq!(s.get("a"), Some(Value::Int(1)));
    assert_eq!(s.get("b"), Some(Value::Int(2)));
}

#[tokio::test]
async fn run_script_log_does_not_crash() {
    let mut ctx = exec_ctx(store(), vec![Permission::ScriptExecution]);
    script_action(r#"log("hello from script")"#)
        .execute(&mut ctx)
        .await
        .unwrap();
}

// ── RunScriptAction — error paths ────────────────────────────────────────────

#[tokio::test]
async fn run_script_syntax_error_returns_failed() {
    let mut ctx = exec_ctx(store(), vec![Permission::ScriptExecution]);
    let result = script_action("let x = ;").execute(&mut ctx).await; // bad syntax
    assert!(
        matches!(result, Err(ActionError::Failed(_))),
        "expected Failed, got: {result:?}"
    );
}

#[tokio::test]
async fn run_script_runtime_error_returns_failed() {
    let mut ctx = exec_ctx(store(), vec![Permission::ScriptExecution]);
    // Divide by zero is a runtime error in Rhai.
    let result = script_action("let x = 1 / 0;").execute(&mut ctx).await;
    assert!(
        matches!(result, Err(ActionError::Failed(_))),
        "expected Failed for runtime error, got: {result:?}"
    );
}

#[tokio::test]
async fn run_script_infinite_loop_is_terminated_by_max_operations() {
    let mut ctx = exec_ctx(store(), vec![Permission::ScriptExecution]);
    // This loop will exceed set_max_operations(50_000) and terminate.
    let result = script_action("loop { let _x = 1 + 1; }")
        .execute(&mut ctx)
        .await;
    assert!(
        result.is_err(),
        "infinite loop must fail with resource-limit error"
    );
}

// ── ExpressionConstraint — happy path ────────────────────────────────────────

#[test]
fn expression_true_literal() {
    let s = InMemoryStateStore::new();
    let event = null_event();
    assert!(
        expr_constraint("true")
            .evaluate(&eval_ctx(&event, &s))
            .unwrap()
    );
}

#[test]
fn expression_false_literal() {
    let s = InMemoryStateStore::new();
    let event = null_event();
    assert!(
        !expr_constraint("false")
            .evaluate(&eval_ctx(&event, &s))
            .unwrap()
    );
}

#[test]
fn expression_arithmetic_comparison() {
    let s = InMemoryStateStore::new();
    let event = null_event();
    assert!(
        expr_constraint("1 + 1 == 2")
            .evaluate(&eval_ctx(&event, &s))
            .unwrap()
    );
}

#[test]
fn expression_get_var_with_matching_value() {
    let s = InMemoryStateStore::new();
    s.set("mode", Value::Str("active".into()));
    let event = null_event();
    assert!(
        expr_constraint(r#"get_var("mode") == "active""#)
            .evaluate(&eval_ctx(&event, &s))
            .unwrap()
    );
}

#[test]
fn expression_get_var_with_non_matching_value() {
    let s = InMemoryStateStore::new();
    s.set("mode", Value::Str("idle".into()));
    let event = null_event();
    assert!(
        !expr_constraint(r#"get_var("mode") == "active""#)
            .evaluate(&eval_ctx(&event, &s))
            .unwrap()
    );
}

#[test]
fn expression_get_var_integer_threshold() {
    let s = InMemoryStateStore::new();
    s.set("count", Value::Int(10));
    let event = null_event();
    assert!(
        expr_constraint(r#"get_var("count") > 5"#)
            .evaluate(&eval_ctx(&event, &s))
            .unwrap()
    );
}

// ── ExpressionConstraint — error paths ───────────────────────────────────────

#[test]
fn expression_non_bool_result_returns_eval_failed() {
    let s = InMemoryStateStore::new();
    let event = null_event();
    // Rhai expression returns int, not bool → eval_with_scope::<bool> fails.
    let result = expr_constraint("42").evaluate(&eval_ctx(&event, &s));
    assert!(
        matches!(result, Err(ConstraintError::EvalFailed(_))),
        "expected EvalFailed for non-bool expression, got: {result:?}"
    );
}

#[test]
fn expression_syntax_error_returns_eval_failed() {
    let s = InMemoryStateStore::new();
    let event = null_event();
    let result = expr_constraint("let x = ;").evaluate(&eval_ctx(&event, &s));
    assert!(
        matches!(result, Err(ConstraintError::EvalFailed(_))),
        "expected EvalFailed for bad syntax, got: {result:?}"
    );
}

#[test]
fn expression_infinite_loop_is_terminated_by_max_operations() {
    let s = InMemoryStateStore::new();
    let event = null_event();
    // While loop exceeds set_max_operations(10_000).
    let result =
        expr_constraint("let x = 0; while true { x += 1 }; x > 0").evaluate(&eval_ctx(&event, &s));
    assert!(
        result.is_err(),
        "infinite loop in expression must fail with resource-limit error"
    );
}
