/// M1.1 unit tests: ConstraintExpr recursive evaluation — Always, Not, All, Any,
/// VarCompare, TimeRange.
///
/// All tests are zero-platform and deterministic (time is taken from event.timestamp,
/// not from SystemTime::now()).
use std::time::{Duration, UNIX_EPOCH};

use korkuma_core::{
    context::EvalContext,
    domain::{CompareOp, ConstraintConfig, ConstraintExpr},
    event::{Event, EventKind},
    registry::Registry,
    state::StateStore,
    value::Value,
};
use korkuma_store::InMemoryStateStore;

// ── helpers ────────────────────────────────────────────────────────────────

/// 2024-01-01 00:30:00 UTC  →  minute-of-day = 30
fn event_at(minute_of_day: u64) -> Event {
    Event {
        kind: EventKind::Manual,
        source: "test".into(),
        timestamp: UNIX_EPOCH + Duration::from_secs(minute_of_day * 60),
        payload: Value::Null,
    }
}

fn ctx<'a>(event: &'a Event, store: &'a InMemoryStateStore) -> EvalContext<'a> {
    EvalContext {
        event,
        macro_id: uuid::Uuid::new_v4(),
        store,
    }
}

fn reg() -> Registry {
    Registry::with_builtins()
}

// ── Always ─────────────────────────────────────────────────────────────────

#[test]
fn always_is_true() {
    let store = InMemoryStateStore::new();
    let ev = event_at(0);
    assert!(
        ConstraintExpr::Always
            .evaluate(&ctx(&ev, &store), &reg())
            .unwrap()
    );
}

// ── Not ────────────────────────────────────────────────────────────────────

#[test]
fn not_inverts_always() {
    let store = InMemoryStateStore::new();
    let ev = event_at(0);
    let expr = ConstraintExpr::Not {
        expr: Box::new(ConstraintExpr::Always),
    };
    assert!(!expr.evaluate(&ctx(&ev, &store), &reg()).unwrap());
}

#[test]
fn not_not_always_is_true() {
    let store = InMemoryStateStore::new();
    let ev = event_at(0);
    let expr = ConstraintExpr::Not {
        expr: Box::new(ConstraintExpr::Not {
            expr: Box::new(ConstraintExpr::Always),
        }),
    };
    assert!(expr.evaluate(&ctx(&ev, &store), &reg()).unwrap());
}

// ── All (AND) ──────────────────────────────────────────────────────────────

#[test]
fn all_empty_is_true() {
    let store = InMemoryStateStore::new();
    let ev = event_at(0);
    assert!(
        ConstraintExpr::All { exprs: vec![] }
            .evaluate(&ctx(&ev, &store), &reg())
            .unwrap()
    );
}

#[test]
fn all_of_true_is_true() {
    let store = InMemoryStateStore::new();
    let ev = event_at(0);
    let expr = ConstraintExpr::All {
        exprs: vec![ConstraintExpr::Always, ConstraintExpr::Always],
    };
    assert!(expr.evaluate(&ctx(&ev, &store), &reg()).unwrap());
}

#[test]
fn all_short_circuits_on_first_false() {
    let store = InMemoryStateStore::new();
    let ev = event_at(0);
    let not_always = ConstraintExpr::Not {
        expr: Box::new(ConstraintExpr::Always),
    };
    let expr = ConstraintExpr::All {
        exprs: vec![not_always, ConstraintExpr::Always],
    };
    assert!(!expr.evaluate(&ctx(&ev, &store), &reg()).unwrap());
}

// ── Any (OR) ───────────────────────────────────────────────────────────────

#[test]
fn any_empty_is_false() {
    let store = InMemoryStateStore::new();
    let ev = event_at(0);
    assert!(
        !ConstraintExpr::Any { exprs: vec![] }
            .evaluate(&ctx(&ev, &store), &reg())
            .unwrap()
    );
}

#[test]
fn any_with_one_true_is_true() {
    let store = InMemoryStateStore::new();
    let ev = event_at(0);
    let not_always = ConstraintExpr::Not {
        expr: Box::new(ConstraintExpr::Always),
    };
    let expr = ConstraintExpr::Any {
        exprs: vec![not_always, ConstraintExpr::Always],
    };
    assert!(expr.evaluate(&ctx(&ev, &store), &reg()).unwrap());
}

#[test]
fn any_all_false_is_false() {
    let store = InMemoryStateStore::new();
    let ev = event_at(0);
    let not_always = || ConstraintExpr::Not {
        expr: Box::new(ConstraintExpr::Always),
    };
    let expr = ConstraintExpr::Any {
        exprs: vec![not_always(), not_always()],
    };
    assert!(!expr.evaluate(&ctx(&ev, &store), &reg()).unwrap());
}

// ── Nested AND/OR/NOT ──────────────────────────────────────────────────────

#[test]
fn de_morgan_not_all_equals_any_not() {
    // NOT(A AND B) == (NOT A) OR (NOT B) when A=true, B=false
    let store = InMemoryStateStore::new();
    let ev = event_at(0);
    let r = reg();

    let a_and_b = ConstraintExpr::All {
        exprs: vec![
            ConstraintExpr::Always,
            ConstraintExpr::Not {
                expr: Box::new(ConstraintExpr::Always),
            },
        ],
    };
    let not_a_and_b = ConstraintExpr::Not {
        expr: Box::new(a_and_b),
    };
    assert!(not_a_and_b.evaluate(&ctx(&ev, &store), &r).unwrap());
}

// ── VarCompare ─────────────────────────────────────────────────────────────

fn var_leaf(key: &str, op: CompareOp, val: Value) -> ConstraintExpr {
    ConstraintExpr::Leaf {
        constraint: ConstraintConfig::VarCompare {
            key: key.into(),
            op,
            value: val,
        },
    }
}

#[test]
fn var_compare_eq() {
    let store = InMemoryStateStore::new();
    store.set("x", Value::Int(7));
    let ev = event_at(0);
    assert!(
        var_leaf("x", CompareOp::Eq, Value::Int(7))
            .evaluate(&ctx(&ev, &store), &reg())
            .unwrap()
    );
    assert!(
        !var_leaf("x", CompareOp::Eq, Value::Int(8))
            .evaluate(&ctx(&ev, &store), &reg())
            .unwrap()
    );
}

#[test]
fn var_compare_gt() {
    let store = InMemoryStateStore::new();
    store.set("counter", Value::Int(5));
    let ev = event_at(0);
    assert!(
        var_leaf("counter", CompareOp::Gt, Value::Int(3))
            .evaluate(&ctx(&ev, &store), &reg())
            .unwrap()
    );
    assert!(
        !var_leaf("counter", CompareOp::Gt, Value::Int(5))
            .evaluate(&ctx(&ev, &store), &reg())
            .unwrap()
    );
    assert!(
        !var_leaf("counter", CompareOp::Gt, Value::Int(10))
            .evaluate(&ctx(&ev, &store), &reg())
            .unwrap()
    );
}

#[test]
fn var_compare_missing_key_is_null() {
    let store = InMemoryStateStore::new();
    let ev = event_at(0);
    // missing key → Value::Null; Null == Null
    assert!(
        var_leaf("missing", CompareOp::Eq, Value::Null)
            .evaluate(&ctx(&ev, &store), &reg())
            .unwrap()
    );
    assert!(
        !var_leaf("missing", CompareOp::Eq, Value::Int(0))
            .evaluate(&ctx(&ev, &store), &reg())
            .unwrap()
    );
}

// ── TimeRange ──────────────────────────────────────────────────────────────

fn time_leaf(from: &str, to: &str) -> ConstraintExpr {
    ConstraintExpr::Leaf {
        constraint: ConstraintConfig::TimeRange {
            from: from.into(),
            to: to.into(),
        },
    }
}

#[test]
fn time_range_in_range() {
    // event timestamp = minute 30 UTC (00:30)
    let ev = event_at(30);
    let store = InMemoryStateStore::new();
    let r = reg();
    assert!(
        time_leaf("00:00", "01:00")
            .evaluate(&ctx(&ev, &store), &r)
            .unwrap()
    );
    assert!(
        time_leaf("00:30", "00:30")
            .evaluate(&ctx(&ev, &store), &r)
            .unwrap()
    ); // exact boundary
}

#[test]
fn time_range_out_of_range() {
    let ev = event_at(30); // 00:30 UTC
    let store = InMemoryStateStore::new();
    let r = reg();
    assert!(
        !time_leaf("01:00", "02:00")
            .evaluate(&ctx(&ev, &store), &r)
            .unwrap()
    );
    assert!(
        !time_leaf("00:31", "23:59")
            .evaluate(&ctx(&ev, &store), &r)
            .unwrap()
    );
}

#[test]
fn time_range_wraps_midnight() {
    // 23:30 UTC = minute 23*60+30 = 1410
    let ev = event_at(1410);
    let store = InMemoryStateStore::new();
    let r = reg();
    // 23:00–01:00 wraps midnight; 23:30 should be in range
    assert!(
        time_leaf("23:00", "01:00")
            .evaluate(&ctx(&ev, &store), &r)
            .unwrap()
    );
    // 09:00–10:00 should not match 23:30
    assert!(
        !time_leaf("09:00", "10:00")
            .evaluate(&ctx(&ev, &store), &r)
            .unwrap()
    );
}
