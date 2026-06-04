//! Rhai scripting integration for KoRkuma.
//!
//! Provides two runtime implementations:
//!
//! | Type | Config | Role |
//! |------|--------|------|
//! | [`RunScriptAction`] | [`ActionConfig::RunScript`] | Executes a Rhai script as a macro action |
//! | [`ExpressionConstraint`] | [`ConstraintConfig::Expression`] | Evaluates a Rhai bool expression as a constraint |
//!
//! # Sandbox
//!
//! Both types enforce resource limits via Rhai's built-in safety controls:
//! - `RunScript`: 50 000 operations, 50 call levels, 1 MiB strings.
//! - `Expression`: 10 000 operations, 20 call levels, 256 KiB strings.
//!
//! An infinite loop or excessive recursion causes the script to terminate
//! with [`ActionError::Failed`] / [`ConstraintError::EvalFailed`].
//!
//! # Host API (RunScript only)
//!
//! | Function | Signature | Permission |
//! |----------|-----------|------------|
//! | `get_var` | `(key: String) -> Dynamic` | none |
//! | `set_var` | `(key: String, val: Dynamic)` | none |
//! | `log`    | `(msg: String)` | none |
//!
//! # Usage
//!
//! ```rust,no_run
//! use korkuma_core::registry::Registry;
//! use korkuma_script::{register_actions, register_constraints};
//!
//! let mut registry = Registry::with_builtins();
//! register_actions(&mut registry);
//! register_constraints(&mut registry);
//! ```

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rhai::{Dynamic, Engine, Map as RhaiMap, Scope};

use korkuma_core::{
    context::{EvalContext, ExecContext},
    domain::{ActionConfig, ConstraintConfig},
    error::{ActionError, ConstraintError},
    permission::{Permission, PermissionSet},
    registry::Registry,
    traits::{Action, Constraint, Outcome},
    value::Value,
};

// ── Value ↔ Dynamic conversion ───────────────────────────────────────────────

fn value_to_dynamic(v: &Value) -> Dynamic {
    match v {
        Value::Null => Dynamic::UNIT,
        Value::Bool(b) => Dynamic::from(*b),
        Value::Int(n) => Dynamic::from(*n),
        Value::Float(f) => Dynamic::from(*f),
        Value::Str(s) => Dynamic::from(s.clone()),
        Value::List(l) => {
            let arr: Vec<Dynamic> = l.iter().map(value_to_dynamic).collect();
            Dynamic::from(arr)
        }
        Value::Map(m) => {
            let mut rhai_map = RhaiMap::new();
            for (k, v) in m {
                rhai_map.insert(k.as_str().into(), value_to_dynamic(v));
            }
            Dynamic::from(rhai_map)
        }
    }
}

fn dynamic_to_value(d: Dynamic) -> Value {
    if d.is_unit() {
        return Value::Null;
    }
    if let Ok(b) = d.as_bool() {
        return Value::Bool(b);
    }
    if let Ok(n) = d.as_int() {
        return Value::Int(n);
    }
    if let Ok(f) = d.as_float() {
        return Value::Float(f);
    }
    // try_cast consumes d — must be last
    d.try_cast::<String>()
        .map(Value::Str)
        .unwrap_or(Value::Null)
}

// ── Sandbox factory ──────────────────────────────────────────────────────────

fn build_sandbox(max_ops: u64, max_levels: usize, max_str: usize) -> Engine {
    let mut engine = Engine::new();
    engine.set_max_operations(max_ops);
    engine.set_max_call_levels(max_levels);
    engine.set_max_string_size(max_str);
    engine
}

// ── RunScriptAction ──────────────────────────────────────────────────────────

/// Executes a Rhai script as a macro action step.
///
/// Requires [`Permission::ScriptExecution`] on the macro. On permission
/// denial the action returns [`ActionError::PermissionDenied`] immediately.
///
/// The script can read and write variables via the host functions:
/// - `get_var(key)` — reads from global store or local scope.
/// - `set_var(key, value)` — writes to the global state store after the script finishes.
/// - `log(msg)` — prints a `[SCRIPT]` line to stdout.
///
/// The `event` scope variable is pre-populated with the event payload.
///
/// **Config**: [`ActionConfig::RunScript`]
pub struct RunScriptAction {
    source: String,
}

#[async_trait]
impl Action for RunScriptAction {
    fn id(&self) -> &'static str {
        "run_script"
    }

    fn required_permissions(&self) -> PermissionSet {
        PermissionSet(vec![Permission::ScriptExecution])
    }

    async fn execute(&self, ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        if !ctx.permissions.allows(&Permission::ScriptExecution) {
            return Err(ActionError::PermissionDenied("ScriptExecution".to_string()));
        }

        let mut engine = build_sandbox(50_000, 50, 1_000_000);

        // Cancellation: abort the script when the macro's cancel token fires.
        let cancel = ctx.cancel.clone();
        engine.on_progress(move |_| {
            if cancel.is_cancelled() {
                Some(Dynamic::UNIT)
            } else {
                None
            }
        });

        // get_var — snapshot-based read of global store + local vars
        let store_for_read: Arc<dyn korkuma_core::state::StateStore> = Arc::clone(&ctx.store);
        let locals_snap = ctx.locals.clone();
        engine.register_fn("get_var", move |key: &str| -> Dynamic {
            if let Some(v) = locals_snap.get(key) {
                return value_to_dynamic(v);
            }
            store_for_read
                .get(key)
                .map(|v| value_to_dynamic(&v))
                .unwrap_or(Dynamic::UNIT)
        });

        // set_var — deferred writes applied after script exits
        let pending: Arc<Mutex<Vec<(String, Value)>>> = Arc::new(Mutex::new(Vec::new()));
        let pending_clone = Arc::clone(&pending);
        engine.register_fn("set_var", move |key: &str, val: Dynamic| {
            pending_clone
                .lock()
                .unwrap()
                .push((key.to_string(), dynamic_to_value(val)));
        });

        // log — emit a tagged line to stdout
        engine.register_fn("log", |msg: &str| {
            println!("[SCRIPT] {msg}");
        });

        // Build scope: inject the triggering event payload
        let mut scope = Scope::new();
        scope.push("event", value_to_dynamic(&ctx.event.payload));

        // Execute
        engine
            .run_with_scope(&mut scope, &self.source)
            .map_err(|e| ActionError::Failed(format!("script error: {e}")))?;

        // Flush deferred variable writes to the global store
        for (key, val) in pending.lock().unwrap().drain(..) {
            ctx.store.set(&key, val);
        }

        Ok(Outcome::Continue)
    }
}

/// Factory: builds [`RunScriptAction`] from [`ActionConfig::RunScript`].
pub fn build_run_script(c: &ActionConfig) -> Option<Box<dyn Action>> {
    if let ActionConfig::RunScript { source, .. } = c {
        Some(Box::new(RunScriptAction {
            source: source.clone(),
        }))
    } else {
        None
    }
}

// ── ExpressionConstraint ─────────────────────────────────────────────────────

/// Evaluates a Rhai boolean expression as a macro constraint leaf.
///
/// The expression runs in a tight sandbox (10 000 operations, 20 call levels).
/// It has access to:
/// - `event` — the event payload as a Dynamic map.
/// - `get_var(key)` — reads a variable from the global state store.
///
/// # Example DSL
///
/// ```text
/// get_var("mode") == "active"
/// event.count > 5
/// ```
///
/// **Config**: [`ConstraintConfig::Expression`]
pub struct ExpressionConstraint {
    dsl: String,
}

impl Constraint for ExpressionConstraint {
    fn evaluate(&self, ctx: &EvalContext) -> Result<bool, ConstraintError> {
        let mut engine = build_sandbox(10_000, 20, 256_000);

        // Inject get_var from a store snapshot
        let snap = ctx.store.snapshot();
        engine.register_fn("get_var", move |key: &str| -> Dynamic {
            snap.get(key)
                .map(|v| value_to_dynamic(v))
                .unwrap_or(Dynamic::UNIT)
        });

        let mut scope = Scope::new();
        scope.push("event", value_to_dynamic(&ctx.event.payload));

        engine
            .eval_with_scope::<bool>(&mut scope, &self.dsl)
            .map_err(|e| ConstraintError::EvalFailed(format!("expression error: {e}")))
    }
}

/// Factory: builds [`ExpressionConstraint`] from [`ConstraintConfig::Expression`].
pub fn build_expression(c: &ConstraintConfig) -> Option<Box<dyn Constraint>> {
    if let ConstraintConfig::Expression { dsl } = c {
        Some(Box::new(ExpressionConstraint { dsl: dsl.clone() }))
    } else {
        None
    }
}

// ── Registration ─────────────────────────────────────────────────────────────

/// Registers the [`RunScriptAction`] factory with `registry`.
pub fn register_actions(registry: &mut Registry) {
    registry.register_action(build_run_script);
}

/// Registers the [`ExpressionConstraint`] factory with `registry`.
pub fn register_constraints(registry: &mut Registry) {
    registry.register_constraint(build_expression);
}
