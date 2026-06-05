use slint::{Global, Model, VecModel};

use korkuma_core::domain::Macro;
use korkuma_core::permission::Permission;

use crate::{
    ConstraintRow, LogEntry, LogTableAdapter, MacroItem, MainWindow, PermissionRow, TriggerRow,
    WorkflowRow, MACROS_PATH,
};
use crate::tree_model::{
    ConstraintTreeRow, WorkflowTreeRow, flatten_constraint, flatten_workflow,
};
use crate::trigger::to_slint_trigger_rows;

/// Insert `entry` at the front of the log model and bump the table version so
/// the `StandardTableView` re-evaluates its rows binding.
pub fn push_log(ui: &MainWindow, entry: LogEntry) {
    let logs_rc = ui.get_logs();
    if let Some(logs) = logs_rc.as_any().downcast_ref::<VecModel<LogEntry>>() {
        logs.insert(0, entry);
        while logs.row_count() > 500 {
            logs.remove(logs.row_count() - 1);
        }
    }
    let v = LogTableAdapter::get(ui).get_version();
    LogTableAdapter::get(ui).set_version(v + 1);
}

pub fn rebuild_model<T: Clone + 'static>(model: &VecModel<T>, items: Vec<T>) {
    while model.row_count() > 0 {
        model.remove(0);
    }
    for item in items {
        model.push(item);
    }
}

pub fn to_slint_constraint_rows(rows: &[ConstraintTreeRow]) -> Vec<ConstraintRow> {
    rows.iter()
        .map(|r| ConstraintRow {
            depth: r.depth,
            kind: r.kind.clone().into(),
            leaf_type: r.leaf_type.clone().into(),
            summary: r.summary.clone().into(),
            params_json: r.params_json.clone().into(),
        })
        .collect()
}

pub fn to_slint_workflow_rows(rows: &[WorkflowTreeRow]) -> Vec<WorkflowRow> {
    rows.iter()
        .map(|r| WorkflowRow {
            depth: r.depth,
            kind: r.kind.clone().into(),
            action_type: r.action_type.clone().into(),
            summary: r.summary.clone().into(),
            params_json: r.params_json.clone().into(),
            is_container: r.is_container,
        })
        .collect()
}

pub fn to_permission_rows(perms: &[Permission]) -> Vec<PermissionRow> {
    perms
        .iter()
        .map(|p| PermissionRow {
            label: p.describe().into(),
        })
        .collect()
}

/// Rebuilds the Permissions-tab list from a macro's granted permissions.
pub fn refresh_permission_rows(ui: &MainWindow, m: &Macro) {
    let rows = to_permission_rows(&m.granted_permissions.0);
    let rc = ui.get_permission_rows();
    if let Some(model) = rc.as_any().downcast_ref::<VecModel<PermissionRow>>() {
        rebuild_model(model, rows);
    }
}

pub fn refresh_editor(ui: &MainWindow, macros: &[Macro], idx: usize) {
    let Some(m) = macros.get(idx) else { return };

    ui.set_macro_name(m.name.clone().into());
    refresh_permission_rows(ui, m);

    let t_rows = to_slint_trigger_rows(&m.triggers);
    let t_rc = ui.get_trigger_rows();
    if let Some(model) = t_rc.as_any().downcast_ref::<VecModel<TriggerRow>>() {
        rebuild_model(model, t_rows);
    }
    ui.set_trigger_selected(-1);
    ui.set_trigger_kind("Manual".into());

    let c_rows = to_slint_constraint_rows(&flatten_constraint(&m.constraints));
    let c_rc = ui.get_constraint_rows();
    if let Some(model) = c_rc.as_any().downcast_ref::<VecModel<ConstraintRow>>() {
        rebuild_model(model, c_rows);
    }
    ui.set_constraint_selected(-1);
    ui.set_constraint_edit_json("".into());
    ui.set_constraint_leaf_type("".into());

    let root = m.workflow.clone().unwrap_or_else(|| m.root_workflow());
    let w_rows = to_slint_workflow_rows(&flatten_workflow(&root));
    let w_rc = ui.get_workflow_rows();
    if let Some(model) = w_rc.as_any().downcast_ref::<VecModel<WorkflowRow>>() {
        rebuild_model(model, w_rows);
    }
    ui.set_workflow_selected(-1);
    ui.set_workflow_edit_json("".into());
    ui.set_workflow_action_type("".into());
}

pub fn reload_ui_model(ui: &MainWindow, macros: &[Macro]) {
    let model_rc = ui.get_macros();
    if let Some(model) = model_rc.as_any().downcast_ref::<VecModel<MacroItem>>() {
        while model.row_count() > 0 {
            model.remove(0);
        }
        for m in macros {
            model.push(MacroItem {
                id: m.id.to_string().into(),
                name: m.name.clone().into(),
                enabled: m.enabled,
            });
        }
    }

    let sel = ui.get_selected_index();
    if sel >= macros.len() as i32 {
        ui.set_selected_index(-1);
        ui.set_macro_name("".into());
        ui.set_constraint_edit_json("".into());
        ui.set_constraint_leaf_type("".into());
        ui.set_workflow_edit_json("".into());
        ui.set_workflow_action_type("".into());
    } else if sel >= 0 {
        refresh_editor(ui, macros, sel as usize);
    }

    push_log(ui, LogEntry {
        level: "INFO".into(),
        source: "watcher".into(),
        message: format!("hot-reloaded {} macro(s) from {MACROS_PATH}", macros.len()).into(),
    });
}