//! Built-in, platform-independent implementations of the core traits.
//!
//! These are wired into `Registry::with_builtins()`. Platform-specific implementations
//! (ActiveWindowConstraint, RunCommandAction, HotkeyProvider …) live in the dedicated
//! crates and are registered at app startup.

use std::time::UNIX_EPOCH;

use crate::context::{EvalContext, ExecContext};
use crate::domain::{ActionConfig, CompareOp, ConstraintConfig, TriggerConfig, VarScope};
use crate::error::{ActionError, ConstraintError};
use crate::event::{Event, EventKind};
use crate::permission::PermissionSet;
use crate::traits::{Action, Constraint, Outcome, TriggerSpec};
use crate::value::Value;

// ── TriggerSpec impls ──────────────────────────────────────────────────────

pub(crate) struct ManualTriggerSpec;

impl TriggerSpec for ManualTriggerSpec {
    fn subscribed_kinds(&self) -> &[EventKind] {
        &[EventKind::Manual]
    }
    fn matches(&self, event: &Event) -> bool {
        event.kind == EventKind::Manual
    }
}

pub(crate) fn build_trigger(c: &TriggerConfig) -> Option<Box<dyn TriggerSpec>> {
    match c {
        TriggerConfig::Manual => Some(Box::new(ManualTriggerSpec)),
        _ => None,
    }
}

// ── Constraint impls ───────────────────────────────────────────────────────

/// Time range check using the event's UTC timestamp.
/// Wraps midnight correctly (e.g. from = "23:00", to = "01:00").
pub(crate) struct TimeRangeConstraint {
    from_min: u32,
    to_min: u32,
}

fn parse_hhmm(s: &str) -> Option<u32> {
    let mut parts = s.splitn(2, ':');
    let h: u32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    (h < 24 && m < 60).then_some(h * 60 + m)
}

fn event_utc_minutes(event: &Event) -> u32 {
    let secs = event.timestamp
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    ((secs % 86400) / 60) as u32
}

impl Constraint for TimeRangeConstraint {
    fn evaluate(&self, ctx: &EvalContext) -> Result<bool, ConstraintError> {
        let now = event_utc_minutes(ctx.event);
        let in_range = if self.from_min <= self.to_min {
            now >= self.from_min && now <= self.to_min
        } else {
            // wraps midnight
            now >= self.from_min || now <= self.to_min
        };
        Ok(in_range)
    }
}

pub(crate) fn build_time_range(c: &ConstraintConfig) -> Option<Box<dyn Constraint>> {
    if let ConstraintConfig::TimeRange { from, to } = c {
        let from_min = parse_hhmm(from)?;
        let to_min = parse_hhmm(to)?;
        Some(Box::new(TimeRangeConstraint { from_min, to_min }))
    } else {
        None
    }
}

/// Compares a StateStore variable to a literal Value.
pub(crate) struct VarCompareConstraint {
    key: String,
    op: CompareOp,
    expected: Value,
}

impl Constraint for VarCompareConstraint {
    fn evaluate(&self, ctx: &EvalContext) -> Result<bool, ConstraintError> {
        let actual = ctx.store.get(&self.key).unwrap_or(Value::Null);
        Ok(compare(&actual, self.op, &self.expected))
    }
}

fn compare(a: &Value, op: CompareOp, b: &Value) -> bool {
    match op {
        CompareOp::Eq => a == b,
        CompareOp::Ne => a != b,
        CompareOp::Lt => ord_cmp(a, b) == Some(std::cmp::Ordering::Less),
        CompareOp::Gt => ord_cmp(a, b) == Some(std::cmp::Ordering::Greater),
        CompareOp::Le => matches!(ord_cmp(a, b), Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)),
        CompareOp::Ge => matches!(ord_cmp(a, b), Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)),
    }
}

fn ord_cmp(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.partial_cmp(y),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        (Value::Str(x), Value::Str(y)) => x.partial_cmp(y),
        _ => None,
    }
}

pub(crate) fn build_var_compare(c: &ConstraintConfig) -> Option<Box<dyn Constraint>> {
    if let ConstraintConfig::VarCompare { key, op, value } = c {
        Some(Box::new(VarCompareConstraint {
            key: key.clone(),
            op: *op,
            expected: value.clone(),
        }))
    } else {
        None
    }
}

// ── Action impls ───────────────────────────────────────────────────────────

pub(crate) struct SetVariableAction {
    scope: VarScope,
    key: String,
    value: Value,
}

impl Action for SetVariableAction {
    fn id(&self) -> &'static str { "set_variable" }
    fn required_permissions(&self) -> PermissionSet { PermissionSet::default() }
    fn execute(&self, ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        match self.scope {
            VarScope::Global => ctx.store.set(&self.key, self.value.clone()),
            VarScope::Local  => { ctx.locals.insert(self.key.clone(), self.value.clone()); }
        }
        Ok(Outcome::Continue)
    }
}

pub(crate) fn build_set_variable(c: &ActionConfig) -> Option<Box<dyn Action>> {
    if let ActionConfig::SetVariable { scope, key, value } = c {
        Some(Box::new(SetVariableAction {
            scope: *scope,
            key: key.clone(),
            value: value.clone(),
        }))
    } else {
        None
    }
}

pub(crate) struct DelayAction { millis: u64 }

impl Action for DelayAction {
    fn id(&self) -> &'static str { "delay" }
    fn required_permissions(&self) -> PermissionSet { PermissionSet::default() }
    fn execute(&self, _ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        std::thread::sleep(std::time::Duration::from_millis(self.millis));
        Ok(Outcome::Continue)
    }
}

pub(crate) fn build_delay(c: &ActionConfig) -> Option<Box<dyn Action>> {
    if let ActionConfig::Delay { millis } = c {
        Some(Box::new(DelayAction { millis: *millis }))
    } else {
        None
    }
}
