use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use slint::{Model, ModelRc, VecModel};

use korkuma_core::{
    domain::{
        ActionConfig, CompareOp, ConstraintConfig, ConstraintExpr, Macro, OnNoBackground,
        ScriptLang, TargetSelector, TriggerConfig, UiOp,
    },
    engine::EngineCommand,
    permission::{aggregate_from_configs, aggregate_from_workflow},
    value::Value,
};
use korkuma_store::save_macros;

use crate::{
    ConstraintRow, MacroItem, MainWindow, TriggerRow, WorkflowRow, MACROS_PATH,
};
use crate::model::{
    rebuild_model, refresh_editor, refresh_permission_rows, to_slint_constraint_rows,
    to_slint_workflow_rows,
};
use crate::trigger::{
    build_trigger_from_ui, default_trigger_config, populate_trigger_fields,
    to_slint_trigger_rows,
};
use crate::tree_model::{
    add_constraint_leaf, add_workflow_action, add_workflow_if, add_workflow_parallel,
    default_action_config, default_constraint_config, delete_constraint_at, delete_workflow_at,
    flatten_constraint, flatten_workflow, move_constraint_node, move_workflow_node,
    update_constraint_leaf, update_workflow_action, wrap_constraint_at,
};

pub fn persist(macros: &[Macro], suppress_reload: &AtomicBool) {
    suppress_reload.store(true, Ordering::Relaxed);
    if let Err(e) = save_macros(std::path::Path::new(MACROS_PATH), macros) {
        eprintln!("[korkuma] failed to save {MACROS_PATH}: {e}");
        suppress_reload.store(false, Ordering::Relaxed);
    }
}

pub fn create_default_macro() -> Macro {
    let actions = vec![ActionConfig::Notify {
        title: "KoRkuma".to_string(),
        body: "Macro fired!".to_string(),
    }];
    let granted_permissions = aggregate_from_configs(&actions);
    Macro {
        id: uuid::Uuid::new_v4(),
        name: "New Macro".to_string(),
        description: String::new(),
        enabled: true,
        category: None,
        triggers: vec![TriggerConfig::Manual],
        constraints: ConstraintExpr::Always,
        actions,
        workflow: None,
        granted_permissions,
        priority: 0,
        concurrency: Default::default(),
    }
}

pub fn wire_callbacks(
    ui: &MainWindow,
    local_macros: Arc<Mutex<Vec<Macro>>>,
    macros_model: Rc<VecModel<MacroItem>>,
    engine_sender: crossbeam_channel::Sender<EngineCommand>,
    ui_weak: slint::Weak<MainWindow>,
    suppress_reload: Arc<AtomicBool>,
) {
    // ── add-macro ─────────────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let macros_model = macros_model.clone();
        let engine_sender = engine_sender.clone();
        let ui_weak = ui_weak.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_add_macro(move || {
            let m = create_default_macro();
            let new_idx = local_macros.lock().unwrap().len() as i32;
            macros_model.push(MacroItem {
                id: m.id.to_string().into(),
                name: m.name.clone().into(),
                enabled: m.enabled,
            });
            engine_sender.send(EngineCommand::AddMacro(m.clone())).ok();
            local_macros.lock().unwrap().push(m);
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_selected_index(new_idx);
                let list = local_macros.lock().unwrap();
                refresh_editor(&ui, &list, new_idx as usize);
            }
            persist(&local_macros.lock().unwrap(), &suppress_reload);
        });
    }

    // ── delete-macro ──────────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let macros_model = macros_model.clone();
        let engine_sender = engine_sender.clone();
        let ui_weak = ui_weak.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_delete_macro(move |idx| {
            let idx = idx as usize;
            let macro_id = local_macros.lock().unwrap().get(idx).map(|m| m.id);
            if let Some(id) = macro_id {
                engine_sender.send(EngineCommand::DeleteMacro(id)).ok();
                macros_model.remove(idx);
                local_macros.lock().unwrap().remove(idx);
                if let Some(ui) = ui_weak.upgrade() {
                    let sel = ui.get_selected_index();
                    if sel == idx as i32 {
                        ui.set_selected_index(-1);
                        ui.set_macro_name("".into());
                        ui.set_constraint_edit_json("".into());
                        ui.set_constraint_leaf_type("".into());
                        ui.set_workflow_edit_json("".into());
                        ui.set_workflow_action_type("".into());
                    } else if sel > idx as i32 {
                        ui.set_selected_index(sel - 1);
                    }
                }
                persist(&local_macros.lock().unwrap(), &suppress_reload);
            }
        });
    }

    // ── toggle-enabled ────────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let macros_model = macros_model.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_toggle_enabled(move |idx, enabled| {
            let idx = idx as usize;
            let macro_id = {
                let mut list = local_macros.lock().unwrap();
                if let Some(m) = list.get_mut(idx) {
                    m.enabled = enabled;
                    Some(m.id)
                } else {
                    None
                }
            };
            if let Some(id) = macro_id {
                engine_sender.send(EngineCommand::SetEnabled(id, enabled)).ok();
                if let Some(row) = macros_model.row_data(idx) {
                    macros_model.set_row_data(idx, MacroItem { enabled, ..row });
                }
                persist(&local_macros.lock().unwrap(), &suppress_reload);
            }
        });
    }

    // ── trigger-macro ─────────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let engine_sender = engine_sender.clone();
        let ui_weak = ui_weak.clone();
        ui.on_trigger_macro(move |idx| {
            let macro_id = local_macros.lock().unwrap().get(idx as usize).map(|m| m.id);
            if let Some(id) = macro_id {
                engine_sender.send(EngineCommand::TriggerManually(id)).ok();
            }
            if let Some(ui) = ui_weak.upgrade() {
                let list = local_macros.lock().unwrap();
                refresh_editor(&ui, &list, idx as usize);
            }
        });
    }

    // ── dry-run-macro ─────────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let engine_sender = engine_sender.clone();
        ui.on_dry_run_macro(move |idx| {
            let macro_id = local_macros.lock().unwrap().get(idx as usize).map(|m| m.id);
            if let Some(id) = macro_id {
                engine_sender.send(EngineCommand::DryRunMacro(id)).ok();
            }
        });
    }

    // ── macro-selected ────────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        ui.on_macro_selected(move |idx| {
            if idx < 0 {
                return;
            }
            if let Some(ui) = ui_weak.upgrade() {
                let list = local_macros.lock().unwrap();
                refresh_editor(&ui, &list, idx as usize);
            }
        });
    }

    // ── rename-macro ──────────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let macros_model = macros_model.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_rename_macro(move |idx, new_name| {
            let idx = idx as usize;
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(idx) {
                m.name = new_name.to_string();
                if let Some(row) = macros_model.row_data(idx) {
                    macros_model.set_row_data(idx, MacroItem { name: new_name, ..row });
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── move-macro-up ─────────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let macros_model = macros_model.clone();
        let ui_weak = ui_weak.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_move_macro_up(move |idx| {
            let idx = idx as usize;
            if idx == 0 {
                return;
            }
            {
                let mut list = local_macros.lock().unwrap();
                list.swap(idx, idx - 1);
            }
            let row_a = macros_model.row_data(idx - 1).unwrap();
            let row_b = macros_model.row_data(idx).unwrap();
            macros_model.set_row_data(idx - 1, row_b);
            macros_model.set_row_data(idx, row_a);
            if let Some(ui) = ui_weak.upgrade() {
                let sel = ui.get_selected_index();
                if sel == idx as i32 {
                    ui.set_selected_index(sel - 1);
                } else if sel == idx as i32 - 1 {
                    ui.set_selected_index(sel + 1);
                }
            }
            persist(&local_macros.lock().unwrap(), &suppress_reload);
        });
    }

    // ── move-macro-down ───────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let macros_model = macros_model.clone();
        let ui_weak = ui_weak.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_move_macro_down(move |idx| {
            let idx = idx as usize;
            let len = local_macros.lock().unwrap().len();
            if idx + 1 >= len {
                return;
            }
            {
                let mut list = local_macros.lock().unwrap();
                list.swap(idx, idx + 1);
            }
            let row_a = macros_model.row_data(idx).unwrap();
            let row_b = macros_model.row_data(idx + 1).unwrap();
            macros_model.set_row_data(idx, row_b);
            macros_model.set_row_data(idx + 1, row_a);
            if let Some(ui) = ui_weak.upgrade() {
                let sel = ui.get_selected_index();
                if sel == idx as i32 {
                    ui.set_selected_index(sel + 1);
                } else if sel == idx as i32 + 1 {
                    ui.set_selected_index(sel - 1);
                }
            }
            persist(&local_macros.lock().unwrap(), &suppress_reload);
        });
    }

    // ── constraint-node-selected ──────────────────────────────────────────────
    {
        let ui_weak = ui_weak.clone();
        ui.on_constraint_node_selected(move |idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let rows_rc = ui.get_constraint_rows();
            if let Some(row) = rows_rc.row_data(idx as usize) {
                ui.set_constraint_edit_json(row.params_json.clone());
                ui.set_constraint_leaf_type(row.leaf_type.clone());
                if let Ok(cfg) = serde_json::from_str::<ConstraintConfig>(row.params_json.as_str()) {
                    match &cfg {
                        ConstraintConfig::ActiveWindow { title_pattern, regex } => {
                            ui.set_c_aw_title(title_pattern.clone().into());
                            ui.set_c_aw_regex(*regex);
                        }
                        ConstraintConfig::TimeRange { from, to } => {
                            ui.set_c_tr_from(from.clone().into());
                            ui.set_c_tr_to(to.clone().into());
                        }
                        ConstraintConfig::VarCompare { key, op, value } => {
                            ui.set_c_vc_key(key.clone().into());
                            ui.set_c_vc_op_idx(match op {
                                CompareOp::Eq => 0,
                                CompareOp::Ne => 1,
                                CompareOp::Lt => 2,
                                CompareOp::Gt => 3,
                                CompareOp::Le => 4,
                                CompareOp::Ge => 5,
                            });
                            ui.set_c_vc_value(match value {
                                Value::Int(i) => i.to_string(),
                                Value::Float(f) => f.to_string(),
                                Value::Bool(b) => b.to_string(),
                                Value::Str(s) => s.clone(),
                                _ => String::new(),
                            }.into());
                        }
                        ConstraintConfig::Expression { dsl } => {
                            ui.set_c_expr_dsl(dsl.clone().into());
                        }
                        _ => {}
                    }
                }
            }
        });
    }

    // ── constraint-update-node ────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_constraint_update_node(move |idx, new_json| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 {
                return;
            }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                m.constraints =
                    update_constraint_leaf(m.constraints.clone(), idx as usize, new_json.as_str());
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                let rows = to_slint_constraint_rows(&flatten_constraint(&m.constraints));
                let rc = ui.get_constraint_rows();
                if let Some(model) = rc.as_any().downcast_ref::<VecModel<ConstraintRow>>() {
                    rebuild_model(model, rows);
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── constraint-update (typed UI) ──────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_constraint_update(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            let node_idx = ui.get_constraint_selected();
            if sel < 0 || node_idx < 0 { return; }

            let leaf_type = ui.get_constraint_leaf_type().to_string();
            let new_cfg = match leaf_type.as_str() {
                "ActiveWindow" => ConstraintConfig::ActiveWindow {
                    title_pattern: ui.get_c_aw_title().to_string(),
                    regex: ui.get_c_aw_regex(),
                },
                "TimeRange" => ConstraintConfig::TimeRange {
                    from: ui.get_c_tr_from().to_string(),
                    to: ui.get_c_tr_to().to_string(),
                },
                "VarCompare" => {
                    let op = match ui.get_c_vc_op_idx() {
                        1 => CompareOp::Ne,
                        2 => CompareOp::Lt,
                        3 => CompareOp::Gt,
                        4 => CompareOp::Le,
                        5 => CompareOp::Ge,
                        _ => CompareOp::Eq,
                    };
                    let vs = ui.get_c_vc_value().to_string();
                    let value = if let Ok(i) = vs.parse::<i64>() {
                        Value::Int(i)
                    } else if let Ok(f) = vs.parse::<f64>() {
                        Value::Float(f)
                    } else if vs == "true" {
                        Value::Bool(true)
                    } else if vs == "false" {
                        Value::Bool(false)
                    } else {
                        Value::Str(vs)
                    };
                    ConstraintConfig::VarCompare { key: ui.get_c_vc_key().to_string(), op, value }
                }
                "Expression" => ConstraintConfig::Expression {
                    dsl: ui.get_c_expr_dsl().to_string(),
                },
                _ => return,
            };
            let new_json = serde_json::to_string(&new_cfg).unwrap_or_default();
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                m.constraints = update_constraint_leaf(m.constraints.clone(), node_idx as usize, &new_json);
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                let rows = to_slint_constraint_rows(&flatten_constraint(&m.constraints));
                let rc = ui.get_constraint_rows();
                if let Some(model) = rc.as_any().downcast_ref::<VecModel<ConstraintRow>>() {
                    rebuild_model(model, rows);
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── constraint-delete-node ────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_constraint_delete_node(move |idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 {
                return;
            }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                m.constraints =
                    delete_constraint_at(m.constraints.clone(), idx as usize);
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                let rows = to_slint_constraint_rows(&flatten_constraint(&m.constraints));
                let rc = ui.get_constraint_rows();
                if let Some(model) = rc.as_any().downcast_ref::<VecModel<ConstraintRow>>() {
                    rebuild_model(model, rows);
                }
                ui.set_constraint_selected(-1);
                ui.set_constraint_edit_json("".into());
                ui.set_constraint_leaf_type("".into());
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── constraint-add-leaf ───────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_constraint_add_leaf(move |leaf_type| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 {
                return;
            }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                let leaf = default_constraint_config(leaf_type.as_str());
                m.constraints = add_constraint_leaf(m.constraints.clone(), leaf);
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                let rows = to_slint_constraint_rows(&flatten_constraint(&m.constraints));
                let rc = ui.get_constraint_rows();
                if let Some(model) = rc.as_any().downcast_ref::<VecModel<ConstraintRow>>() {
                    rebuild_model(model, rows);
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── constraint-wrap ───────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_constraint_wrap(move |kind| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 {
                return;
            }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                let node_sel = ui.get_constraint_selected();
                let pos = if node_sel >= 0 { Some(node_sel as usize) } else { None };
                m.constraints =
                    wrap_constraint_at(m.constraints.clone(), pos, kind.as_str());
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                let rows = to_slint_constraint_rows(&flatten_constraint(&m.constraints));
                let rc = ui.get_constraint_rows();
                if let Some(model) = rc.as_any().downcast_ref::<VecModel<ConstraintRow>>() {
                    rebuild_model(model, rows);
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── constraint-move-up ────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_constraint_move_up(move |idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 {
                return;
            }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                m.constraints =
                    move_constraint_node(m.constraints.clone(), idx as usize, true);
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                let rows = to_slint_constraint_rows(&flatten_constraint(&m.constraints));
                let rc = ui.get_constraint_rows();
                if let Some(model) = rc.as_any().downcast_ref::<VecModel<ConstraintRow>>() {
                    rebuild_model(model, rows);
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── constraint-move-down ──────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_constraint_move_down(move |idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 {
                return;
            }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                m.constraints =
                    move_constraint_node(m.constraints.clone(), idx as usize, false);
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                let rows = to_slint_constraint_rows(&flatten_constraint(&m.constraints));
                let rc = ui.get_constraint_rows();
                if let Some(model) = rc.as_any().downcast_ref::<VecModel<ConstraintRow>>() {
                    rebuild_model(model, rows);
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── workflow-node-selected ────────────────────────────────────────────────
    {
        let ui_weak = ui_weak.clone();
        ui.on_workflow_node_selected(move |idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let rows_rc = ui.get_workflow_rows();
            if let Some(row) = rows_rc.row_data(idx as usize) {
                ui.set_workflow_action_type(row.action_type.clone());
                if let Ok(cfg) = serde_json::from_str::<ActionConfig>(row.params_json.as_str()) {
                    match &cfg {
                        ActionConfig::Notify { title, body } => {
                            ui.set_wf_notify_title(title.clone().into());
                            ui.set_wf_notify_body(body.clone().into());
                        }
                        ActionConfig::RunCommand { program, args, capture } => {
                            ui.set_wf_run_program(program.clone().into());
                            ui.set_wf_run_args(args.join(" ").into());
                            ui.set_wf_run_capture(*capture);
                        }
                        ActionConfig::RunScript { source, .. } => {
                            ui.set_wf_script_lang_idx(0); // only Rhai
                            ui.set_wf_script_source(source.clone().into());
                        }
                        ActionConfig::Delay { millis } => {
                            ui.set_wf_delay_ms(millis.to_string().into());
                        }
                        ActionConfig::Interact { target, on_no_background, op } => {
                            let (ttype_idx, tpat) = match target {
                                TargetSelector::Foreground => (0i32, String::new()),
                                TargetSelector::Window { title_pattern, .. } => (1, title_pattern.clone()),
                                TargetSelector::Process { name } => (2, name.clone()),
                                TargetSelector::BrowserTab { url_pattern } => (3, url_pattern.clone()),
                                TargetSelector::Custom { provider, .. } => (0, provider.clone()),
                            };
                            let nobg_idx = match on_no_background {
                                OnNoBackground::Degrade => 0i32,
                                OnNoBackground::Fail => 1,
                                OnNoBackground::Queue => 2,
                            };
                            ui.set_target_type_idx(ttype_idx);
                            ui.set_target_pattern(tpat.into());
                            ui.set_on_no_bg_idx(nobg_idx);
                            ui.set_workflow_edit_json(
                                serde_json::to_string_pretty(op).unwrap_or_default().into()
                            );
                        }
                        _ => {
                            ui.set_workflow_edit_json(row.params_json.clone());
                        }
                    }
                }
            }
        });
    }

    // ── workflow-update-node ──────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_workflow_update_node(move |idx, new_json| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 {
                return;
            }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                let root = m.workflow.clone().unwrap_or_else(|| m.root_workflow());
                let new_root =
                    update_workflow_action(root, idx as usize, new_json.as_str());
                m.workflow = Some(new_root.clone());
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                let rows = to_slint_workflow_rows(&flatten_workflow(&new_root));
                let rc = ui.get_workflow_rows();
                if let Some(model) = rc.as_any().downcast_ref::<VecModel<WorkflowRow>>() {
                    rebuild_model(model, rows);
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── workflow-update (typed UI) ────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_workflow_update(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            let node_idx = ui.get_workflow_selected();
            if sel < 0 || node_idx < 0 { return; }

            let action_type = ui.get_workflow_action_type().to_string();
            let new_cfg = match action_type.as_str() {
                "Notify" => ActionConfig::Notify {
                    title: ui.get_wf_notify_title().to_string(),
                    body: ui.get_wf_notify_body().to_string(),
                },
                "RunCommand" => ActionConfig::RunCommand {
                    program: ui.get_wf_run_program().to_string(),
                    args: ui.get_wf_run_args()
                        .split_whitespace()
                        .map(String::from)
                        .collect(),
                    capture: ui.get_wf_run_capture(),
                },
                "RunScript" => ActionConfig::RunScript {
                    lang: ScriptLang::Rhai,
                    source: ui.get_wf_script_source().to_string(),
                },
                "Delay" => {
                    let ms = ui.get_wf_delay_ms().parse::<u64>().unwrap_or(1000);
                    ActionConfig::Delay { millis: ms }
                }
                "Interact" => {
                    let target = match ui.get_target_type_idx() {
                        1 => TargetSelector::Window {
                            title_pattern: ui.get_target_pattern().to_string(),
                            regex: false,
                        },
                        2 => TargetSelector::Process { name: ui.get_target_pattern().to_string() },
                        3 => TargetSelector::BrowserTab { url_pattern: ui.get_target_pattern().to_string() },
                        _ => TargetSelector::Foreground,
                    };
                    let on_no_background = match ui.get_on_no_bg_idx() {
                        1 => OnNoBackground::Fail,
                        2 => OnNoBackground::Queue,
                        _ => OnNoBackground::Degrade,
                    };
                    let op = serde_json::from_str::<UiOp>(ui.get_workflow_edit_json().as_str())
                        .unwrap_or(UiOp::Click { node: String::new() });
                    ActionConfig::Interact { target, op, on_no_background }
                }
                _ => {
                    let Ok(cfg) = serde_json::from_str::<ActionConfig>(ui.get_workflow_edit_json().as_str()) else { return };
                    cfg
                }
            };
            let new_json = serde_json::to_string(&new_cfg).unwrap_or_default();
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                let root = m.workflow.clone().unwrap_or_else(|| m.root_workflow());
                let new_root = update_workflow_action(root, node_idx as usize, &new_json);
                m.workflow = Some(new_root.clone());
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                let rows = to_slint_workflow_rows(&flatten_workflow(&new_root));
                let rc = ui.get_workflow_rows();
                if let Some(model) = rc.as_any().downcast_ref::<VecModel<WorkflowRow>>() {
                    rebuild_model(model, rows);
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── workflow-delete-node ──────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_workflow_delete_node(move |idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 {
                return;
            }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                let root = m.workflow.clone().unwrap_or_else(|| m.root_workflow());
                let new_root = delete_workflow_at(root, idx as usize);
                m.workflow = Some(new_root.clone());
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                let rows = to_slint_workflow_rows(&flatten_workflow(&new_root));
                let rc = ui.get_workflow_rows();
                if let Some(model) = rc.as_any().downcast_ref::<VecModel<WorkflowRow>>() {
                    rebuild_model(model, rows);
                }
                ui.set_workflow_selected(-1);
                ui.set_workflow_edit_json("".into());
                ui.set_workflow_action_type("".into());
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── workflow-add-action ───────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_workflow_add_action(move |action_type| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 {
                return;
            }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                let root = m.workflow.clone().unwrap_or_else(|| m.root_workflow());
                let cfg = default_action_config(action_type.as_str());
                let new_root = add_workflow_action(root, cfg);
                m.workflow = Some(new_root.clone());
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                let rows = to_slint_workflow_rows(&flatten_workflow(&new_root));
                let rc = ui.get_workflow_rows();
                if let Some(model) = rc.as_any().downcast_ref::<VecModel<WorkflowRow>>() {
                    rebuild_model(model, rows);
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── workflow-add-if ───────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_workflow_add_if(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 {
                return;
            }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                let root = m.workflow.clone().unwrap_or_else(|| m.root_workflow());
                let new_root = add_workflow_if(root);
                m.workflow = Some(new_root.clone());
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                let rows = to_slint_workflow_rows(&flatten_workflow(&new_root));
                let rc = ui.get_workflow_rows();
                if let Some(model) = rc.as_any().downcast_ref::<VecModel<WorkflowRow>>() {
                    rebuild_model(model, rows);
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── workflow-add-parallel ─────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_workflow_add_parallel(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 {
                return;
            }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                let root = m.workflow.clone().unwrap_or_else(|| m.root_workflow());
                let new_root = add_workflow_parallel(root);
                m.workflow = Some(new_root.clone());
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                let rows = to_slint_workflow_rows(&flatten_workflow(&new_root));
                let rc = ui.get_workflow_rows();
                if let Some(model) = rc.as_any().downcast_ref::<VecModel<WorkflowRow>>() {
                    rebuild_model(model, rows);
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── workflow-move-up ──────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_workflow_move_up(move |idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 {
                return;
            }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                let root = m.workflow.clone().unwrap_or_else(|| m.root_workflow());
                let new_root = move_workflow_node(root, idx as usize, true);
                m.workflow = Some(new_root.clone());
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                let rows = to_slint_workflow_rows(&flatten_workflow(&new_root));
                let rc = ui.get_workflow_rows();
                if let Some(model) = rc.as_any().downcast_ref::<VecModel<WorkflowRow>>() {
                    rebuild_model(model, rows);
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── workflow-move-down ────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_workflow_move_down(move |idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 {
                return;
            }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                let root = m.workflow.clone().unwrap_or_else(|| m.root_workflow());
                let new_root = move_workflow_node(root, idx as usize, false);
                m.workflow = Some(new_root.clone());
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                let rows = to_slint_workflow_rows(&flatten_workflow(&new_root));
                let rc = ui.get_workflow_rows();
                if let Some(model) = rc.as_any().downcast_ref::<VecModel<WorkflowRow>>() {
                    rebuild_model(model, rows);
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── trigger-select ────────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        ui.on_trigger_select(move |tidx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let midx = ui.get_selected_index();
            if midx < 0 {
                return;
            }
            let list = local_macros.lock().unwrap();
            let Some(m) = list.get(midx as usize) else { return };
            let Some(trigger) = m.triggers.get(tidx as usize) else { return };
            populate_trigger_fields(&ui, trigger);
        });
    }

    // ── trigger-add ───────────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_trigger_add(move |kind| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let midx = ui.get_selected_index();
            if midx < 0 {
                return;
            }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(midx as usize) {
                m.triggers.push(default_trigger_config(kind.as_str()));
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                let rows = to_slint_trigger_rows(&m.triggers);
                let rc = ui.get_trigger_rows();
                if let Some(model) = rc.as_any().downcast_ref::<VecModel<TriggerRow>>() {
                    rebuild_model(model, rows);
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── trigger-delete ────────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_trigger_delete(move |tidx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let midx = ui.get_selected_index();
            if midx < 0 {
                return;
            }
            let tidx = tidx as usize;
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(midx as usize) {
                if tidx < m.triggers.len() {
                    m.triggers.remove(tidx);
                    engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                    let rows = to_slint_trigger_rows(&m.triggers);
                    let rc = ui.get_trigger_rows();
                    if let Some(model) = rc.as_any().downcast_ref::<VecModel<TriggerRow>>() {
                        rebuild_model(model, rows);
                    }
                    ui.set_trigger_selected(-1);
                    ui.set_trigger_kind("Manual".into());
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── trigger-apply ─────────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_trigger_apply(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let midx = ui.get_selected_index();
            let tidx = ui.get_trigger_selected();
            if midx < 0 || tidx < 0 {
                return;
            }
            let kind = ui.get_trigger_kind().to_string();
            let new_trigger = build_trigger_from_ui(&ui, &kind);
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(midx as usize) {
                if let Some(t) = m.triggers.get_mut(tidx as usize) {
                    *t = new_trigger;
                    engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                    let rows = to_slint_trigger_rows(&m.triggers);
                    let rc = ui.get_trigger_rows();
                    if let Some(model) = rc.as_any().downcast_ref::<VecModel<TriggerRow>>() {
                        rebuild_model(model, rows);
                    }
                }
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── request-save (open permission approval dialog) ────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        ui.on_request_save(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 {
                return;
            }
            let list = local_macros.lock().unwrap();
            if let Some(m) = list.get(sel as usize) {
                let root = m.workflow.clone().unwrap_or_else(|| m.root_workflow());
                let labels: Vec<slint::SharedString> = aggregate_from_workflow(&root)
                    .0
                    .iter()
                    .map(|p| p.describe().into())
                    .collect();
                ui.set_pending_permissions(ModelRc::from(Rc::new(VecModel::from(labels))));
                ui.set_show_permission_dialog(true);
            }
        });
    }

    // ── approve-permissions (grant aggregated set) ────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_approve_permissions(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            ui.set_show_permission_dialog(false);
            let sel = ui.get_selected_index();
            if sel < 0 {
                return;
            }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                let root = m.workflow.clone().unwrap_or_else(|| m.root_workflow());
                m.granted_permissions = aggregate_from_workflow(&root);
                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                refresh_permission_rows(&ui, m);
            }
            persist(&list, &suppress_reload);
        });
    }

    // ── cancel-permissions ────────────────────────────────────────────────────
    {
        let ui_weak = ui_weak.clone();
        ui.on_cancel_permissions(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_show_permission_dialog(false);
            }
        });
    }

    // ── revoke-permission (per-macro) ─────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_revoke_permission(move |pidx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 {
                return;
            }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                let pidx = pidx as usize;
                if pidx < m.granted_permissions.0.len() {
                    m.granted_permissions.0.remove(pidx);
                    engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                    refresh_permission_rows(&ui, m);
                }
            }
            persist(&list, &suppress_reload);
        });
    }
}