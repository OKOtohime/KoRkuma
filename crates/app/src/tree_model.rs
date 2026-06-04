//! Flat tree model for the visual constraint / workflow editors (M2.4 §16.2).
//!
//! `ConstraintExpr` and `WorkflowNode` are recursive trees; Slint's `ListView`
//! needs a flat `VecModel`. This module provides:
//!
//! - **Flatten** functions — depth-first walk producing `Vec<{Constraint,Workflow}Row>`
//!   with a `depth` field that drives indentation.
//! - **Edit helpers** — append leaf, delete at flat position, replace params JSON at
//!   flat position.  Positions are stable for a given tree snapshot (same ordering as
//!   the flatten walk).

use korkuma_core::domain::{
    ActionConfig, CompareOp, ConstraintConfig, ConstraintExpr, InputEvent, OnNoBackground,
    ScriptLang, TargetSelector, UiOp, VarScope, WaitCondition, WorkflowNode,
};
use korkuma_core::value::Value;

// ── Constraint model ─────────────────────────────────────────────────────────

/// One row in the flat constraint tree display model.
#[derive(Clone, Debug, Default)]
pub struct ConstraintTreeRow {
    /// Nesting depth — multiply by indent pixels to get left padding.
    pub depth: i32,
    /// Node kind: `"Always"` | `"Not"` | `"All"` | `"Any"` | `"Leaf"`.
    pub kind: String,
    /// For `Leaf` nodes: the config variant name (e.g. `"ActiveWindow"`).
    pub leaf_type: String,
    /// Human-readable one-liner shown in the list.
    pub summary: String,
    /// Serialized `ConstraintConfig` (Leaf only); empty for group nodes.
    pub params_json: String,
}

/// Flatten a `ConstraintExpr` tree into a depth-first ordered list.
pub fn flatten_constraint(expr: &ConstraintExpr) -> Vec<ConstraintTreeRow> {
    let mut rows = Vec::new();
    flatten_constraint_rec(expr, 0, &mut rows);
    rows
}

fn flatten_constraint_rec(expr: &ConstraintExpr, depth: i32, rows: &mut Vec<ConstraintTreeRow>) {
    match expr {
        ConstraintExpr::Always => rows.push(ConstraintTreeRow {
            depth,
            kind: "Always".into(),
            summary: "(always — no constraint)".into(),
            ..Default::default()
        }),
        ConstraintExpr::Leaf { constraint } => {
            let (leaf_type, summary) = describe_constraint(constraint);
            rows.push(ConstraintTreeRow {
                depth,
                kind: "Leaf".into(),
                leaf_type,
                summary,
                params_json: serde_json::to_string_pretty(constraint).unwrap_or_default(),
            });
        }
        ConstraintExpr::Not { expr } => {
            rows.push(ConstraintTreeRow {
                depth,
                kind: "Not".into(),
                summary: "NOT".into(),
                ..Default::default()
            });
            flatten_constraint_rec(expr, depth + 1, rows);
        }
        ConstraintExpr::All { exprs } => {
            rows.push(ConstraintTreeRow {
                depth,
                kind: "All".into(),
                summary: format!("AND  ({} conditions)", exprs.len()),
                ..Default::default()
            });
            for e in exprs {
                flatten_constraint_rec(e, depth + 1, rows);
            }
        }
        ConstraintExpr::Any { exprs } => {
            rows.push(ConstraintTreeRow {
                depth,
                kind: "Any".into(),
                summary: format!("OR  ({} conditions)", exprs.len()),
                ..Default::default()
            });
            for e in exprs {
                flatten_constraint_rec(e, depth + 1, rows);
            }
        }
    }
}

fn describe_constraint(c: &ConstraintConfig) -> (String, String) {
    match c {
        ConstraintConfig::ActiveWindow { title_pattern, regex } => (
            "ActiveWindow".into(),
            format!(
                "Active window ~ \"{}\"{}",
                title_pattern,
                if *regex { " (regex)" } else { "" }
            ),
        ),
        ConstraintConfig::TimeRange { from, to } => {
            ("TimeRange".into(), format!("Time {from}–{to}"))
        }
        ConstraintConfig::VarCompare { key, op, value } => (
            "VarCompare".into(),
            format!("${key} {} {value:?}", compare_op_str(*op)),
        ),
        ConstraintConfig::Expression { dsl } => {
            ("Expression".into(), format!("expr: {dsl}"))
        }
        ConstraintConfig::Custom { provider, .. } => {
            ("Custom".into(), format!("custom:{provider}"))
        }
    }
}

fn compare_op_str(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Eq => "==",
        CompareOp::Ne => "!=",
        CompareOp::Lt => "<",
        CompareOp::Gt => ">",
        CompareOp::Le => "<=",
        CompareOp::Ge => ">=",
    }
}

// ── Constraint edit operations ────────────────────────────────────────────────

/// Default `ConstraintConfig` for the given leaf type name.
pub fn default_constraint_config(leaf_type: &str) -> ConstraintConfig {
    match leaf_type {
        "ActiveWindow" => ConstraintConfig::ActiveWindow {
            title_pattern: String::new(),
            regex: false,
        },
        "TimeRange" => ConstraintConfig::TimeRange {
            from: "09:00".into(),
            to: "18:00".into(),
        },
        "VarCompare" => ConstraintConfig::VarCompare {
            key: "var".into(),
            op: CompareOp::Eq,
            value: Value::Int(0),
        },
        "Expression" => ConstraintConfig::Expression { dsl: "true".into() },
        _ => ConstraintConfig::Expression { dsl: "true".into() },
    }
}

/// Append a new leaf to the constraint tree.
/// - `Always` is replaced by a single Leaf.
/// - `All` gets the leaf appended.
/// - Anything else is wrapped in `All` with the leaf added.
pub fn add_constraint_leaf(root: ConstraintExpr, leaf: ConstraintConfig) -> ConstraintExpr {
    match root {
        ConstraintExpr::Always => ConstraintExpr::Leaf { constraint: leaf },
        ConstraintExpr::All { mut exprs } => {
            exprs.push(ConstraintExpr::Leaf { constraint: leaf });
            ConstraintExpr::All { exprs }
        }
        other => ConstraintExpr::All {
            exprs: vec![other, ConstraintExpr::Leaf { constraint: leaf }],
        },
    }
}

/// Replace the params of the Leaf at flat position `pos` with new JSON.
/// Returns the original tree unchanged if `pos` is out of range or not a Leaf,
/// or if the JSON fails to parse as a `ConstraintConfig`.
pub fn update_constraint_leaf(
    root: ConstraintExpr,
    pos: usize,
    new_json: &str,
) -> ConstraintExpr {
    let Ok(new_cfg) = serde_json::from_str::<ConstraintConfig>(new_json) else {
        return root;
    };
    let (result, _) = replace_leaf_rec(root, pos, new_cfg, &mut 0);
    result
}

fn replace_leaf_rec(
    expr: ConstraintExpr,
    target: usize,
    new_cfg: ConstraintConfig,
    counter: &mut usize,
) -> (ConstraintExpr, bool) {
    let pos = *counter;
    *counter += 1;

    match expr {
        ConstraintExpr::Leaf { .. } if pos == target => {
            (ConstraintExpr::Leaf { constraint: new_cfg }, true)
        }
        ConstraintExpr::Not { expr } => {
            let (child, found) = replace_leaf_rec(*expr, target, new_cfg, counter);
            (ConstraintExpr::Not { expr: Box::new(child) }, found)
        }
        ConstraintExpr::All { exprs } => {
            let mut out = Vec::new();
            let mut found = false;
            for e in exprs {
                let (ne, f) = replace_leaf_rec(e, target, new_cfg.clone(), counter);
                out.push(ne);
                found |= f;
            }
            (ConstraintExpr::All { exprs: out }, found)
        }
        ConstraintExpr::Any { exprs } => {
            let mut out = Vec::new();
            let mut found = false;
            for e in exprs {
                let (ne, f) = replace_leaf_rec(e, target, new_cfg.clone(), counter);
                out.push(ne);
                found |= f;
            }
            (ConstraintExpr::Any { exprs: out }, found)
        }
        other => (other, false),
    }
}

/// Delete the node (and its entire subtree) at flat position `pos`.
/// Group nodes with only one child remaining are simplified:
/// - `All/Any` with 1 child → the child directly.
/// - `All/Any` with 0 children → `Always`.
/// - `Not` with deleted child → `Always`.
pub fn delete_constraint_at(root: ConstraintExpr, pos: usize) -> ConstraintExpr {
    let (result, _) = delete_constraint_rec(root, pos, &mut 0);
    result.unwrap_or(ConstraintExpr::Always)
}

fn delete_constraint_rec(
    expr: ConstraintExpr,
    target: usize,
    counter: &mut usize,
) -> (Option<ConstraintExpr>, bool) {
    let pos = *counter;
    *counter += 1;

    if pos == target {
        // Skip counting this whole subtree — it's being deleted.
        skip_count(&expr, counter);
        return (None, true);
    }

    match expr {
        ConstraintExpr::Not { expr } => {
            let (child, found) = delete_constraint_rec(*expr, target, counter);
            let result = match child {
                Some(c) => ConstraintExpr::Not { expr: Box::new(c) },
                None => ConstraintExpr::Always,
            };
            (Some(result), found)
        }
        ConstraintExpr::All { exprs } => {
            let (children, found) = delete_group_children(exprs, target, counter);
            let result = simplify_and(children);
            (Some(result), found)
        }
        ConstraintExpr::Any { exprs } => {
            let (children, found) = delete_group_children(exprs, target, counter);
            let result = simplify_or(children);
            (Some(result), found)
        }
        other => (Some(other), false),
    }
}

fn delete_group_children(
    exprs: Vec<ConstraintExpr>,
    target: usize,
    counter: &mut usize,
) -> (Vec<ConstraintExpr>, bool) {
    let mut out = Vec::new();
    let mut found = false;
    for e in exprs {
        let (opt, f) = delete_constraint_rec(e, target, counter);
        if let Some(c) = opt {
            out.push(c);
        }
        found |= f;
    }
    (out, found)
}

fn simplify_and(mut children: Vec<ConstraintExpr>) -> ConstraintExpr {
    match children.len() {
        0 => ConstraintExpr::Always,
        1 => children.remove(0),
        _ => ConstraintExpr::All { exprs: children },
    }
}

fn simplify_or(mut children: Vec<ConstraintExpr>) -> ConstraintExpr {
    match children.len() {
        0 => ConstraintExpr::Always,
        1 => children.remove(0),
        _ => ConstraintExpr::Any { exprs: children },
    }
}

/// Count the flat positions taken by a subtree (without modifying the node).
fn skip_count(expr: &ConstraintExpr, counter: &mut usize) {
    match expr {
        ConstraintExpr::Not { expr } => {
            *counter += 1;
            skip_count(expr, counter);
        }
        ConstraintExpr::All { exprs } | ConstraintExpr::Any { exprs } => {
            for e in exprs {
                *counter += 1;
                skip_count(e, counter);
            }
        }
        _ => {} // Leaf and Always consume exactly 1 position (already counted at call site)
    }
}

// ── Workflow model ────────────────────────────────────────────────────────────

/// One row in the flat workflow tree display model.
#[derive(Clone, Debug, Default)]
pub struct WorkflowTreeRow {
    /// Nesting depth.
    pub depth: i32,
    /// Node kind: `"Action"` | `"Seq"` | `"Parallel"` | `"If"` | `"While"` | etc.
    pub kind: String,
    /// For `Action` nodes: the `ActionConfig` variant name.
    pub action_type: String,
    /// Human-readable one-liner.
    pub summary: String,
    /// Serialized `ActionConfig` (Action only).
    pub params_json: String,
    /// True for container nodes (Seq/Parallel/If/While/…).
    pub is_container: bool,
}

/// Flatten a `WorkflowNode` tree into a depth-first ordered list.
pub fn flatten_workflow(node: &WorkflowNode) -> Vec<WorkflowTreeRow> {
    let mut rows = Vec::new();
    flatten_workflow_rec(node, 0, &mut rows);
    rows
}

fn flatten_workflow_rec(node: &WorkflowNode, depth: i32, rows: &mut Vec<WorkflowTreeRow>) {
    match node {
        WorkflowNode::Action(cfg) => {
            let (action_type, summary) = describe_action(cfg);
            rows.push(WorkflowTreeRow {
                depth,
                kind: "Action".into(),
                action_type,
                summary,
                params_json: serde_json::to_string_pretty(cfg).unwrap_or_default(),
                is_container: false,
            });
        }
        WorkflowNode::Seq(children) => {
            rows.push(WorkflowTreeRow {
                depth,
                kind: "Seq".into(),
                summary: format!("Seq  ({} steps)", children.len()),
                is_container: true,
                ..Default::default()
            });
            for c in children {
                flatten_workflow_rec(c, depth + 1, rows);
            }
        }
        WorkflowNode::Parallel(children) => {
            rows.push(WorkflowTreeRow {
                depth,
                kind: "Parallel".into(),
                summary: format!("Parallel  ({} branches)", children.len()),
                is_container: true,
                ..Default::default()
            });
            for c in children {
                flatten_workflow_rec(c, depth + 1, rows);
            }
        }
        WorkflowNode::If { then, otherwise, .. } => {
            rows.push(WorkflowTreeRow {
                depth,
                kind: "If".into(),
                summary: "If  (condition)".into(),
                is_container: true,
                ..Default::default()
            });
            rows.push(WorkflowTreeRow {
                depth: depth + 1,
                kind: "Then".into(),
                summary: "then:".into(),
                is_container: true,
                ..Default::default()
            });
            flatten_workflow_rec(then, depth + 2, rows);
            if let Some(else_node) = otherwise {
                rows.push(WorkflowTreeRow {
                    depth: depth + 1,
                    kind: "Else".into(),
                    summary: "else:".into(),
                    is_container: true,
                    ..Default::default()
                });
                flatten_workflow_rec(else_node, depth + 2, rows);
            }
        }
        WorkflowNode::While { body, max_iter, .. } => {
            rows.push(WorkflowTreeRow {
                depth,
                kind: "While".into(),
                summary: format!("While  (max {max_iter} iterations)"),
                is_container: true,
                ..Default::default()
            });
            flatten_workflow_rec(body, depth + 1, rows);
        }
        WorkflowNode::ForEach { var, body, .. } => {
            rows.push(WorkflowTreeRow {
                depth,
                kind: "ForEach".into(),
                summary: format!("ForEach  (${var})"),
                is_container: true,
                ..Default::default()
            });
            flatten_workflow_rec(body, depth + 1, rows);
        }
        WorkflowNode::Retry { body, times, .. } => {
            rows.push(WorkflowTreeRow {
                depth,
                kind: "Retry".into(),
                summary: format!("Retry  ({times}×)"),
                is_container: true,
                ..Default::default()
            });
            flatten_workflow_rec(body, depth + 1, rows);
        }
        WorkflowNode::Timeout { body, millis } => {
            rows.push(WorkflowTreeRow {
                depth,
                kind: "Timeout".into(),
                summary: format!("Timeout  ({millis}ms)"),
                is_container: true,
                ..Default::default()
            });
            flatten_workflow_rec(body, depth + 1, rows);
        }
        WorkflowNode::Wait { until } => {
            let summary = match until {
                WaitCondition::Duration { millis } => format!("Wait  {millis}ms"),
            };
            rows.push(WorkflowTreeRow {
                depth,
                kind: "Wait".into(),
                summary,
                is_container: false,
                ..Default::default()
            });
        }
    }
}

fn describe_action(cfg: &ActionConfig) -> (String, String) {
    match cfg {
        ActionConfig::Notify { title, .. } => ("Notify".into(), format!("Notify: \"{title}\"")),
        ActionConfig::RunCommand { program, args, .. } => (
            "RunCommand".into(),
            format!("Run: {program} {}", args.join(" ")),
        ),
        ActionConfig::SimulateInput { sequence } => (
            "SimulateInput".into(),
            format!("SimulateInput  ({} events)", sequence.len()),
        ),
        ActionConfig::Delay { millis } => ("Delay".into(), format!("Delay  {millis}ms")),
        ActionConfig::SetVariable { key, .. } => {
            ("SetVariable".into(), format!("Set ${key}"))
        }
        ActionConfig::RunScript { .. } => ("RunScript".into(), "Run Script".into()),
        ActionConfig::HttpRequest { method, url, .. } => {
            ("HttpRequest".into(), format!("{method} {url}"))
        }
        ActionConfig::Interact { target, op, .. } => {
            let target_str = describe_target(target);
            let op_str = describe_op(op);
            ("Interact".into(), format!("Interact: {op_str} on {target_str}"))
        }
        ActionConfig::Custom { provider, .. } => {
            ("Custom".into(), format!("Custom: {provider}"))
        }
    }
}

fn describe_target(t: &TargetSelector) -> String {
    match t {
        TargetSelector::Foreground => "foreground".into(),
        TargetSelector::Window { title_pattern, .. } => format!("window:\"{title_pattern}\""),
        TargetSelector::Process { name } => format!("process:{name}"),
        TargetSelector::BrowserTab { url_pattern } => format!("tab:{url_pattern}"),
        TargetSelector::Custom { provider, .. } => format!("custom:{provider}"),
    }
}

fn describe_op(op: &UiOp) -> String {
    match op {
        UiOp::Click { node } => format!("click({node})"),
        UiOp::SetText { node, text } => format!("setText({node}, \"{text}\")"),
        UiOp::SendKeys { .. } => "sendKeys".into(),
        UiOp::Focus { node } => {
            format!("focus({})", node.as_deref().unwrap_or("root"))
        }
        UiOp::ReadValue { node } => format!("read({node})"),
    }
}

// ── Workflow edit operations ──────────────────────────────────────────────────

/// Returns a default `ActionConfig` for the given type name.
pub fn default_action_config(action_type: &str) -> ActionConfig {
    match action_type {
        "Notify" => ActionConfig::Notify {
            title: "Notification".into(),
            body: String::new(),
        },
        "RunCommand" => ActionConfig::RunCommand {
            program: "echo".into(),
            args: vec![],
            capture: false,
        },
        "Delay" => ActionConfig::Delay { millis: 1000 },
        "RunScript" => ActionConfig::RunScript {
            lang: ScriptLang::Rhai,
            source: "// your script here".into(),
        },
        "SimulateInput" => ActionConfig::SimulateInput { sequence: vec![] },
        "SetVariable" => ActionConfig::SetVariable {
            scope: VarScope::Global,
            key: "var".into(),
            value: Value::Int(0),
        },
        "HttpRequest" => ActionConfig::HttpRequest {
            method: "GET".into(),
            url: "https://example.com".into(),
            body: None,
        },
        "Interact" => ActionConfig::Interact {
            target: TargetSelector::Foreground,
            op: UiOp::Click { node: String::new() },
            on_no_background: OnNoBackground::Degrade,
        },
        _ => ActionConfig::Delay { millis: 0 },
    }
}

/// Append a new Action node to the workflow root.
/// Wraps in `Seq` if necessary.
pub fn add_workflow_action(root: WorkflowNode, cfg: ActionConfig) -> WorkflowNode {
    let new_node = WorkflowNode::Action(cfg);
    match root {
        WorkflowNode::Seq(mut children) => {
            children.push(new_node);
            WorkflowNode::Seq(children)
        }
        other => WorkflowNode::Seq(vec![other, new_node]),
    }
}

/// Append an `If` node (with `Always` condition and empty `Seq` body) to the root.
pub fn add_workflow_if(root: WorkflowNode) -> WorkflowNode {
    let if_node = WorkflowNode::If {
        cond: korkuma_core::domain::ConstraintExpr::Always,
        then: Box::new(WorkflowNode::Seq(vec![])),
        otherwise: None,
    };
    match root {
        WorkflowNode::Seq(mut children) => {
            children.push(if_node);
            WorkflowNode::Seq(children)
        }
        other => WorkflowNode::Seq(vec![other, if_node]),
    }
}

/// Append a `Parallel` node with two empty `Seq` branches.
pub fn add_workflow_parallel(root: WorkflowNode) -> WorkflowNode {
    let par_node = WorkflowNode::Parallel(vec![
        WorkflowNode::Seq(vec![]),
        WorkflowNode::Seq(vec![]),
    ]);
    match root {
        WorkflowNode::Seq(mut children) => {
            children.push(par_node);
            WorkflowNode::Seq(children)
        }
        other => WorkflowNode::Seq(vec![other, par_node]),
    }
}

/// Replace the `ActionConfig` of the Action node at flat position `pos` with `new_json`.
pub fn update_workflow_action(root: WorkflowNode, pos: usize, new_json: &str) -> WorkflowNode {
    let Ok(new_cfg) = serde_json::from_str::<ActionConfig>(new_json) else {
        return root;
    };
    let (result, _) = replace_action_rec(root, pos, new_cfg, &mut 0);
    result
}

fn replace_action_rec(
    node: WorkflowNode,
    target: usize,
    new_cfg: ActionConfig,
    counter: &mut usize,
) -> (WorkflowNode, bool) {
    let pos = *counter;
    *counter += 1;

    match node {
        WorkflowNode::Action(_) if pos == target => (WorkflowNode::Action(new_cfg), true),
        WorkflowNode::Seq(children) => {
            let (children, found) = replace_children(children, target, new_cfg, counter);
            (WorkflowNode::Seq(children), found)
        }
        WorkflowNode::Parallel(children) => {
            let (children, found) = replace_children(children, target, new_cfg, counter);
            (WorkflowNode::Parallel(children), found)
        }
        WorkflowNode::If { cond, then, otherwise } => {
            // count "If" header + "Then" label + recurse into then
            // (counter already incremented for If; Then and Else labels are counted in flatten)
            *counter += 1; // "Then" label row
            let (new_then, found) = replace_action_rec(*then, target, new_cfg.clone(), counter);
            let new_otherwise = match otherwise {
                Some(else_node) => {
                    *counter += 1; // "Else" label row
                    let (new_else, _) =
                        replace_action_rec(*else_node, target, new_cfg, counter);
                    Some(Box::new(new_else))
                }
                None => None,
            };
            (
                WorkflowNode::If {
                    cond,
                    then: Box::new(new_then),
                    otherwise: new_otherwise,
                },
                found,
            )
        }
        WorkflowNode::While { cond, body, max_iter } => {
            let (new_body, found) = replace_action_rec(*body, target, new_cfg, counter);
            (WorkflowNode::While { cond, body: Box::new(new_body), max_iter }, found)
        }
        WorkflowNode::Retry { body, times, backoff_ms } => {
            let (new_body, found) = replace_action_rec(*body, target, new_cfg, counter);
            (WorkflowNode::Retry { body: Box::new(new_body), times, backoff_ms }, found)
        }
        WorkflowNode::Timeout { body, millis } => {
            let (new_body, found) = replace_action_rec(*body, target, new_cfg, counter);
            (WorkflowNode::Timeout { body: Box::new(new_body), millis }, found)
        }
        other => (other, false),
    }
}

fn replace_children(
    children: Vec<WorkflowNode>,
    target: usize,
    new_cfg: ActionConfig,
    counter: &mut usize,
) -> (Vec<WorkflowNode>, bool) {
    let mut out = Vec::new();
    let mut found = false;
    for c in children {
        let (nc, f) = replace_action_rec(c, target, new_cfg.clone(), counter);
        out.push(nc);
        found |= f;
    }
    (out, found)
}

/// Delete the workflow node (and subtree) at flat position `pos`.
pub fn delete_workflow_at(root: WorkflowNode, pos: usize) -> WorkflowNode {
    let (result, _) = delete_workflow_rec(root, pos, &mut 0);
    result.unwrap_or(WorkflowNode::Seq(vec![]))
}

fn delete_workflow_rec(
    node: WorkflowNode,
    target: usize,
    counter: &mut usize,
) -> (Option<WorkflowNode>, bool) {
    let pos = *counter;
    *counter += 1;

    if pos == target {
        skip_workflow_count(&node, counter);
        return (None, true);
    }

    match node {
        WorkflowNode::Seq(children) => {
            let (children, found) = delete_workflow_children(children, target, counter);
            let result = match children.len() {
                0 => WorkflowNode::Seq(vec![]),
                1 => {
                    let mut v = children;
                    v.remove(0)
                }
                _ => WorkflowNode::Seq(children),
            };
            (Some(result), found)
        }
        WorkflowNode::Parallel(children) => {
            let (children, found) = delete_workflow_children(children, target, counter);
            (Some(WorkflowNode::Parallel(children)), found)
        }
        WorkflowNode::If { cond, then, otherwise } => {
            *counter += 1; // "Then" label
            let (new_then, found) = delete_workflow_rec(*then, target, counter);
            let new_then = new_then.unwrap_or(WorkflowNode::Seq(vec![]));
            let new_otherwise = match otherwise {
                Some(else_node) => {
                    *counter += 1; // "Else" label
                    let (new_else, _) = delete_workflow_rec(*else_node, target, counter);
                    new_else.map(Box::new)
                }
                None => None,
            };
            (
                Some(WorkflowNode::If {
                    cond,
                    then: Box::new(new_then),
                    otherwise: new_otherwise,
                }),
                found,
            )
        }
        WorkflowNode::While { cond, body, max_iter } => {
            let (new_body, found) = delete_workflow_rec(*body, target, counter);
            let new_body = new_body.unwrap_or(WorkflowNode::Seq(vec![]));
            (Some(WorkflowNode::While { cond, body: Box::new(new_body), max_iter }), found)
        }
        WorkflowNode::Retry { body, times, backoff_ms } => {
            let (new_body, found) = delete_workflow_rec(*body, target, counter);
            let new_body = new_body.unwrap_or(WorkflowNode::Seq(vec![]));
            (Some(WorkflowNode::Retry { body: Box::new(new_body), times, backoff_ms }), found)
        }
        WorkflowNode::Timeout { body, millis } => {
            let (new_body, found) = delete_workflow_rec(*body, target, counter);
            let new_body = new_body.unwrap_or(WorkflowNode::Seq(vec![]));
            (Some(WorkflowNode::Timeout { body: Box::new(new_body), millis }), found)
        }
        other => (Some(other), false),
    }
}

fn delete_workflow_children(
    children: Vec<WorkflowNode>,
    target: usize,
    counter: &mut usize,
) -> (Vec<WorkflowNode>, bool) {
    let mut out = Vec::new();
    let mut found = false;
    for c in children {
        let (opt, f) = delete_workflow_rec(c, target, counter);
        if let Some(nc) = opt {
            out.push(nc);
        }
        found |= f;
    }
    (out, found)
}

fn skip_workflow_count(node: &WorkflowNode, counter: &mut usize) {
    match node {
        WorkflowNode::Seq(children) | WorkflowNode::Parallel(children) => {
            for c in children {
                *counter += 1;
                skip_workflow_count(c, counter);
            }
        }
        WorkflowNode::If { then, otherwise, .. } => {
            *counter += 2; // "Then" + then body header
            skip_workflow_count(then, counter);
            if let Some(else_node) = otherwise {
                *counter += 2;
                skip_workflow_count(else_node, counter);
            }
        }
        WorkflowNode::While { body, .. }
        | WorkflowNode::Retry { body, .. }
        | WorkflowNode::Timeout { body, .. }
        | WorkflowNode::ForEach { body, .. } => {
            *counter += 1;
            skip_workflow_count(body, counter);
        }
        _ => {} // leaf nodes consume exactly 1 position (counted at call site)
    }
}

// ── Constraint wrap-at-position ───────────────────────────────────────────────

fn wrap_expr(expr: ConstraintExpr, kind: &str) -> ConstraintExpr {
    match kind {
        "And" => {
            if matches!(expr, ConstraintExpr::All { .. }) {
                expr
            } else {
                ConstraintExpr::All { exprs: vec![expr] }
            }
        }
        "Or" => {
            if matches!(expr, ConstraintExpr::Any { .. }) {
                expr
            } else {
                ConstraintExpr::Any { exprs: vec![expr] }
            }
        }
        "Not" => ConstraintExpr::Not { expr: Box::new(expr) },
        _ => expr,
    }
}

fn wrap_constraint_rec(
    expr: ConstraintExpr,
    target: usize,
    kind: &str,
    counter: &mut usize,
) -> (ConstraintExpr, bool) {
    let pos = *counter;
    *counter += 1;
    if pos == target {
        return (wrap_expr(expr, kind), true);
    }
    match expr {
        ConstraintExpr::Not { expr: inner } => {
            let (new_inner, found) = wrap_constraint_rec(*inner, target, kind, counter);
            (ConstraintExpr::Not { expr: Box::new(new_inner) }, found)
        }
        ConstraintExpr::All { exprs } => {
            let (new_exprs, found) = wrap_constraint_children_rec(exprs, target, kind, counter);
            (ConstraintExpr::All { exprs: new_exprs }, found)
        }
        ConstraintExpr::Any { exprs } => {
            let (new_exprs, found) = wrap_constraint_children_rec(exprs, target, kind, counter);
            (ConstraintExpr::Any { exprs: new_exprs }, found)
        }
        other => (other, false),
    }
}

fn wrap_constraint_children_rec(
    exprs: Vec<ConstraintExpr>,
    target: usize,
    kind: &str,
    counter: &mut usize,
) -> (Vec<ConstraintExpr>, bool) {
    let mut out = Vec::with_capacity(exprs.len());
    let mut found = false;
    for e in exprs {
        if found {
            out.push(e);
        } else {
            let (new_e, f) = wrap_constraint_rec(e, target, kind, counter);
            out.push(new_e);
            found = f;
        }
    }
    (out, found)
}

/// Wrap the node at flat position `target` with AND/OR/NOT.
/// Defaults to wrapping the root (position 0) when `target` is `None`.
pub fn wrap_constraint_at(root: ConstraintExpr, target: Option<usize>, kind: &str) -> ConstraintExpr {
    let pos = target.unwrap_or(0);
    let (result, _) = wrap_constraint_rec(root, pos, kind, &mut 0);
    result
}

// ── Constraint move-within-siblings ──────────────────────────────────────────

fn constraint_subtree_size(expr: &ConstraintExpr) -> usize {
    match expr {
        ConstraintExpr::Always | ConstraintExpr::Leaf { .. } => 1,
        ConstraintExpr::Not { expr: inner } => 1 + constraint_subtree_size(inner),
        ConstraintExpr::All { exprs } | ConstraintExpr::Any { exprs } => {
            1 + exprs.iter().map(constraint_subtree_size).sum::<usize>()
        }
    }
}

fn move_constraint_children(
    mut exprs: Vec<ConstraintExpr>,
    target: usize,
    up: bool,
    first_child_start: usize,
) -> (Vec<ConstraintExpr>, bool) {
    let mut child_starts = Vec::with_capacity(exprs.len());
    let mut cur = first_child_start;
    for e in &exprs {
        child_starts.push(cur);
        cur += constraint_subtree_size(e);
    }

    if let Some(i) = child_starts.iter().position(|&s| s == target) {
        let j = if up {
            if i > 0 { Some(i - 1) } else { None }
        } else if i + 1 < exprs.len() {
            Some(i + 1)
        } else {
            None
        };
        if let Some(j) = j {
            exprs.swap(i, j);
        }
        return (exprs, true);
    }

    for i in 0..exprs.len() {
        let cs = child_starts[i];
        let c_size = constraint_subtree_size(&exprs[i]);
        if cs <= target && target < cs + c_size {
            let child = exprs.remove(i);
            let (new_child, found) = try_move_constraint(child, target, up, cs);
            exprs.insert(i, new_child);
            return (exprs, found);
        }
    }

    (exprs, false)
}

fn try_move_constraint(
    expr: ConstraintExpr,
    target: usize,
    up: bool,
    start: usize,
) -> (ConstraintExpr, bool) {
    match expr {
        ConstraintExpr::All { exprs } => {
            let (new_exprs, found) = move_constraint_children(exprs, target, up, start + 1);
            (ConstraintExpr::All { exprs: new_exprs }, found)
        }
        ConstraintExpr::Any { exprs } => {
            let (new_exprs, found) = move_constraint_children(exprs, target, up, start + 1);
            (ConstraintExpr::Any { exprs: new_exprs }, found)
        }
        ConstraintExpr::Not { expr: inner } => {
            let (new_inner, found) = try_move_constraint(*inner, target, up, start + 1);
            (ConstraintExpr::Not { expr: Box::new(new_inner) }, found)
        }
        other => (other, false),
    }
}

/// Swap the node at flat position `target` with its previous (`up=true`) or next sibling.
pub fn move_constraint_node(root: ConstraintExpr, target: usize, up: bool) -> ConstraintExpr {
    let (result, _) = try_move_constraint(root, target, up, 0);
    result
}

// ── Workflow move-within-siblings ─────────────────────────────────────────────

fn workflow_subtree_size(node: &WorkflowNode) -> usize {
    match node {
        WorkflowNode::Action(_) | WorkflowNode::Wait { .. } => 1,
        WorkflowNode::Seq(ch) | WorkflowNode::Parallel(ch) => {
            1 + ch.iter().map(workflow_subtree_size).sum::<usize>()
        }
        WorkflowNode::If { then, otherwise, .. } => {
            // If header + Then label + then body + optional (Else label + else body)
            2 + workflow_subtree_size(then)
                + otherwise.as_ref().map(|e| 1 + workflow_subtree_size(e)).unwrap_or(0)
        }
        WorkflowNode::While { body, .. }
        | WorkflowNode::Retry { body, .. }
        | WorkflowNode::Timeout { body, .. }
        | WorkflowNode::ForEach { body, .. } => 1 + workflow_subtree_size(body),
    }
}

fn move_in_children(
    mut children: Vec<WorkflowNode>,
    target: usize,
    up: bool,
    first_child_start: usize,
) -> (Vec<WorkflowNode>, bool) {
    let mut child_starts = Vec::with_capacity(children.len());
    let mut cur = first_child_start;
    for c in &children {
        child_starts.push(cur);
        cur += workflow_subtree_size(c);
    }

    if let Some(i) = child_starts.iter().position(|&s| s == target) {
        let j = if up {
            if i > 0 { Some(i - 1) } else { None }
        } else if i + 1 < children.len() {
            Some(i + 1)
        } else {
            None
        };
        if let Some(j) = j {
            children.swap(i, j);
        }
        return (children, true);
    }

    for i in 0..children.len() {
        let cs = child_starts[i];
        let c_size = workflow_subtree_size(&children[i]);
        if cs <= target && target < cs + c_size {
            let child = children.remove(i);
            let (new_child, found) = try_move_in_seq(child, target, up, cs);
            children.insert(i, new_child);
            return (children, found);
        }
    }

    (children, false)
}

fn try_move_in_seq(
    node: WorkflowNode,
    target: usize,
    up: bool,
    start: usize,
) -> (WorkflowNode, bool) {
    match node {
        WorkflowNode::Seq(children) => {
            let (new_ch, found) = move_in_children(children, target, up, start + 1);
            (WorkflowNode::Seq(new_ch), found)
        }
        WorkflowNode::Parallel(children) => {
            let (new_ch, found) = move_in_children(children, target, up, start + 1);
            (WorkflowNode::Parallel(new_ch), found)
        }
        WorkflowNode::If { cond, then, otherwise } => {
            let then_start = start + 2;
            let then_size = workflow_subtree_size(&then);
            if then_start <= target && target < then_start + then_size {
                let (new_then, found) = try_move_in_seq(*then, target, up, then_start);
                return (WorkflowNode::If { cond, then: Box::new(new_then), otherwise }, found);
            }
            if let Some(else_node) = otherwise {
                let else_start = then_start + then_size + 1;
                let else_size = workflow_subtree_size(&else_node);
                if else_start <= target && target < else_start + else_size {
                    let (new_else, found) =
                        try_move_in_seq(*else_node, target, up, else_start);
                    return (
                        WorkflowNode::If {
                            cond,
                            then,
                            otherwise: Some(Box::new(new_else)),
                        },
                        found,
                    );
                }
                return (WorkflowNode::If { cond, then, otherwise: Some(else_node) }, false);
            }
            (WorkflowNode::If { cond, then, otherwise: None }, false)
        }
        WorkflowNode::While { cond, body, max_iter } => {
            let (new_body, found) = try_move_in_seq(*body, target, up, start + 1);
            (WorkflowNode::While { cond, body: Box::new(new_body), max_iter }, found)
        }
        WorkflowNode::Retry { body, times, backoff_ms } => {
            let (new_body, found) = try_move_in_seq(*body, target, up, start + 1);
            (WorkflowNode::Retry { body: Box::new(new_body), times, backoff_ms }, found)
        }
        WorkflowNode::Timeout { body, millis } => {
            let (new_body, found) = try_move_in_seq(*body, target, up, start + 1);
            (WorkflowNode::Timeout { body: Box::new(new_body), millis }, found)
        }
        WorkflowNode::ForEach { var, items, body } => {
            let (new_body, found) = try_move_in_seq(*body, target, up, start + 1);
            (WorkflowNode::ForEach { var, items, body: Box::new(new_body) }, found)
        }
        other => (other, false),
    }
}

/// Swap the workflow node at flat position `target` with its previous (`up=true`) or next sibling.
pub fn move_workflow_node(root: WorkflowNode, target: usize, up: bool) -> WorkflowNode {
    let (result, _) = try_move_in_seq(root, target, up, 0);
    result
}

// ── InputEvent helpers (needed for ActionConfig::SimulateInput default) ───────

#[allow(unused)]
fn _dummy_input() -> InputEvent {
    InputEvent::KeyPress { key: "a".into() }
}
