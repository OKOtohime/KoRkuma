slint::include_modules!();

mod tree_model;

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use notify::{EventKind, RecursiveMode, Watcher};
use slint::{Model, ModelRc, VecModel};

use koakuma_actions::register_all as register_actions;
use koakuma_core::{
    domain::{ActionConfig, ConstraintExpr, Macro, TriggerConfig},
    engine::{EngineCommand, EngineEvent, LogLevel},
    engine_loop::start_engine,
    permission::aggregate_from_configs,
    state::StateStore,
};
use koakuma_hooks::register_trigger_specs;
use koakuma_script::{
    register_actions as register_script_actions,
    register_constraints as register_script_constraints,
};
use koakuma_store::{InMemoryStateStore, load_macros, save_macros};

use tree_model::{
    ConstraintTreeRow, WorkflowTreeRow,
    add_constraint_leaf, default_constraint_config,
    delete_constraint_at, update_constraint_leaf,
    wrap_constraint_and, wrap_constraint_not, wrap_constraint_or,
    add_workflow_action, add_workflow_if, add_workflow_parallel,
    default_action_config, delete_workflow_at, flatten_constraint,
    flatten_workflow, update_workflow_action,
};

const MACROS_PATH: &str = "macros.json";

fn main() -> Result<(), slint::PlatformError> {
    let backend = select_backend();

    println!("╔══════════════════════════════════════════╗");
    println!("║   Koakuma  —  Automation Engine (M2.4)   ║");
    println!("╚══════════════════════════════════════════╝");
    println!("[koakuma] renderer: {backend}");

    // ── 1. Registry ───────────────────────────────────────────────────────────
    let mut registry = koakuma_core::registry::Registry::with_builtins();
    register_trigger_specs(&mut registry);
    register_actions(&mut registry);
    register_script_actions(&mut registry);
    register_script_constraints(&mut registry);
    let registry = Arc::new(registry);

    // ── 2. State store ────────────────────────────────────────────────────────
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());

    // ── 3. UI & models ────────────────────────────────────────────────────────
    let ui = match MainWindow::new() {
        Ok(w) => w,
        Err(e) if backend == "winit-femtovg" => {
            eprintln!("[koakuma] femtovg init failed ({e}); falling back to software renderer");
            // SAFETY: engine thread not yet spawned; Slint platform not yet committed.
            unsafe {
                std::env::set_var("SLINT_BACKEND", "winit-software");
            }
            MainWindow::new()?
        }
        Err(e) => return Err(e),
    };
    let ui_weak = ui.as_weak();

    let macros_model: Rc<VecModel<MacroItem>> = Rc::new(VecModel::default());
    let logs_model: Rc<VecModel<LogEntry>> = Rc::new(VecModel::default());
    let constraint_model: Rc<VecModel<ConstraintRow>> = Rc::new(VecModel::default());
    let workflow_model: Rc<VecModel<WorkflowRow>> = Rc::new(VecModel::default());

    ui.set_macros(ModelRc::from(macros_model.clone()));
    ui.set_logs(ModelRc::from(logs_model.clone()));
    ui.set_constraint_rows(ModelRc::from(constraint_model.clone()));
    ui.set_workflow_rows(ModelRc::from(workflow_model.clone()));

    let local_macros: Arc<Mutex<Vec<Macro>>> = Arc::new(Mutex::new(Vec::new()));

    // ── 4. Start engine ───────────────────────────────────────────────────────
    let (engine, _event_sink) = start_engine(Arc::clone(&registry), Arc::clone(&store), {
        let ui_weak = ui_weak.clone();
        move |ev| {
            let msg = format_engine_event(&ev);
            let _ = ui_weak.upgrade_in_event_loop(move |ui| {
                let model_rc = ui.get_logs();
                if let Some(model) = model_rc.as_any().downcast_ref::<VecModel<LogEntry>>() {
                    model.insert(0, LogEntry { message: msg.into() });
                    while model.row_count() > 500 {
                        model.remove(model.row_count() - 1);
                    }
                }
            });
        }
    });

    let engine_sender = engine.clone_sender();

    // ── 5. Start hooks (Windows only) ─────────────────────────────────────────
    #[cfg(target_os = "windows")]
    let _providers = start_hooks(_event_sink);

    // ── 6. Load macros ────────────────────────────────────────────────────────
    match load_macros(std::path::Path::new(MACROS_PATH)) {
        Ok(loaded) => {
            println!(
                "[koakuma] loaded {} macro(s) from {MACROS_PATH}",
                loaded.len()
            );
            let mut guard = local_macros.lock().unwrap();
            for m in loaded {
                macros_model.push(MacroItem {
                    id: m.id.to_string().into(),
                    name: m.name.clone().into(),
                    enabled: m.enabled,
                });
                engine.send(EngineCommand::AddMacro(m.clone()));
                guard.push(m);
            }
        }
        Err(e) => {
            eprintln!("[koakuma] could not load {MACROS_PATH}: {e}");
        }
    }

    // ── 7. Hot-reload watcher ─────────────────────────────────────────────────
    let suppress_reload = Arc::new(AtomicBool::new(false));
    let _watcher = spawn_file_watcher(
        Arc::clone(&local_macros),
        engine_sender.clone(),
        ui_weak.clone(),
        Arc::clone(&suppress_reload),
    );

    // ── 8. Wire callbacks ─────────────────────────────────────────────────────

    // add-macro
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

    // delete-macro
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
                        ui.set_selected_triggers("".into());
                        ui.set_constraint_edit_json("".into());
                        ui.set_workflow_edit_json("".into());
                    } else if sel > idx as i32 {
                        ui.set_selected_index(sel - 1);
                    }
                }
                persist(&local_macros.lock().unwrap(), &suppress_reload);
            }
        });
    }

    // toggle-enabled
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

    // trigger-macro
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

    // dry-run-macro
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

    // macro-selected
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

    // ── Constraint tree callbacks ─────────────────────────────────────────────

    // constraint-node-selected: populate edit-json with the selected row's params
    {
        let ui_weak = ui_weak.clone();
        ui.on_constraint_node_selected(move |idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let rows_rc = ui.get_constraint_rows();
            if let Some(row) = rows_rc.row_data(idx as usize) {
                ui.set_constraint_edit_json(row.params_json.clone());
            }
        });
    }

    // constraint-update-node
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
            let sel = sel as usize;
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel) {
                m.constraints = update_constraint_leaf(
                    m.constraints.clone(),
                    idx as usize,
                    new_json.as_str(),
                );
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

    // constraint-delete-node
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
            let sel = sel as usize;
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel) {
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
            }
            persist(&list, &suppress_reload);
        });
    }

    // constraint-add-leaf
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
            let sel = sel as usize;
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel) {
                let leaf = default_constraint_config(leaf_type.as_str());
                m.constraints =
                    add_constraint_leaf(m.constraints.clone(), leaf);
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

    // constraint-wrap
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
            let sel = sel as usize;
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel) {
                m.constraints = match kind.as_str() {
                    "And" => wrap_constraint_and(m.constraints.clone()),
                    "Or" => wrap_constraint_or(m.constraints.clone()),
                    "Not" => wrap_constraint_not(m.constraints.clone()),
                    _ => m.constraints.clone(),
                };
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

    // ── Workflow tree callbacks ───────────────────────────────────────────────

    // workflow-node-selected
    {
        let ui_weak = ui_weak.clone();
        ui.on_workflow_node_selected(move |idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let rows_rc = ui.get_workflow_rows();
            if let Some(row) = rows_rc.row_data(idx as usize) {
                ui.set_workflow_edit_json(row.params_json.clone());
                // Populate target selector if this is an Interact action
                if row.action_type == "Interact" {
                    if let Ok(cfg) =
                        serde_json::from_str::<ActionConfig>(row.params_json.as_str())
                    {
                        if let ActionConfig::Interact { target, on_no_background, .. } = &cfg {
                            use koakuma_core::domain::{OnNoBackground, TargetSelector};
                            let (ttype, tpat) = match target {
                                TargetSelector::Foreground => ("Foreground", String::new()),
                                TargetSelector::Window { title_pattern, .. } => {
                                    ("Window", title_pattern.clone())
                                }
                                TargetSelector::Process { name } => ("Process", name.clone()),
                                TargetSelector::BrowserTab { url_pattern } => {
                                    ("BrowserTab", url_pattern.clone())
                                }
                                TargetSelector::Custom { provider, .. } => {
                                    ("Foreground", provider.clone())
                                }
                            };
                            let nobg = match on_no_background {
                                OnNoBackground::Degrade => "Degrade",
                                OnNoBackground::Fail => "Fail",
                                OnNoBackground::Queue => "Queue",
                            };
                            ui.set_target_type(ttype.into());
                            ui.set_target_pattern(tpat.into());
                            ui.set_on_no_bg(nobg.into());
                        }
                    }
                }
            }
        });
    }

    // workflow-update-node
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
            let sel = sel as usize;
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel) {
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

    // workflow-delete-node
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
            let sel = sel as usize;
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel) {
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
            }
            persist(&list, &suppress_reload);
        });
    }

    // workflow-add-action
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
            let sel = sel as usize;
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel) {
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

    // workflow-add-if
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
            let sel = sel as usize;
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel) {
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

    // workflow-add-parallel
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
            let sel = sel as usize;
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel) {
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

    // ── 9. Run UI ─────────────────────────────────────────────────────────────
    ui.run()?;

    // ── 10. Graceful shutdown ─────────────────────────────────────────────────
    #[cfg(target_os = "windows")]
    {
        let mut providers = _providers;
        for p in &mut providers {
            p.stop();
        }
    }
    drop(engine);

    Ok(())
}

// ── Model helpers ─────────────────────────────────────────────────────────────

fn to_slint_constraint_rows(rows: &[ConstraintTreeRow]) -> Vec<ConstraintRow> {
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

fn to_slint_workflow_rows(rows: &[WorkflowTreeRow]) -> Vec<WorkflowRow> {
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

fn rebuild_model<T: Clone + 'static>(model: &VecModel<T>, items: Vec<T>) {
    while model.row_count() > 0 {
        model.remove(0);
    }
    for item in items {
        model.push(item);
    }
}

// ── Editor refresh ────────────────────────────────────────────────────────────

fn refresh_editor(ui: &MainWindow, macros: &[Macro], idx: usize) {
    let Some(m) = macros.get(idx) else { return };

    // Triggers: JSON preview
    ui.set_selected_triggers(
        serde_json::to_string_pretty(&m.triggers)
            .unwrap_or_default()
            .into(),
    );

    // Constraints: flat tree model
    let c_rows = to_slint_constraint_rows(&flatten_constraint(&m.constraints));
    let c_rc = ui.get_constraint_rows();
    if let Some(model) = c_rc.as_any().downcast_ref::<VecModel<ConstraintRow>>() {
        rebuild_model(model, c_rows);
    }
    ui.set_constraint_selected(-1);
    ui.set_constraint_edit_json("".into());

    // Workflow: flat tree model
    let root = m.workflow.clone().unwrap_or_else(|| m.root_workflow());
    let w_rows = to_slint_workflow_rows(&flatten_workflow(&root));
    let w_rc = ui.get_workflow_rows();
    if let Some(model) = w_rc.as_any().downcast_ref::<VecModel<WorkflowRow>>() {
        rebuild_model(model, w_rows);
    }
    ui.set_workflow_selected(-1);
    ui.set_workflow_edit_json("".into());
}

// ── File watcher ──────────────────────────────────────────────────────────────

fn spawn_file_watcher(
    local_macros: Arc<Mutex<Vec<Macro>>>,
    engine_sender: crossbeam_channel::Sender<EngineCommand>,
    ui_weak: slint::Weak<MainWindow>,
    suppress_reload: Arc<AtomicBool>,
) -> notify::RecommendedWatcher {
    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(tx).expect("failed to create file watcher");
    watcher
        .watch(std::path::Path::new("."), RecursiveMode::NonRecursive)
        .expect("failed to watch current directory");

    std::thread::spawn(move || {
        for res in rx {
            let event = match res {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("[koakuma] watcher error: {e}");
                    continue;
                }
            };

            let is_macros_json = event
                .paths
                .iter()
                .any(|p| p.file_name().and_then(|n| n.to_str()) == Some(MACROS_PATH));
            if !is_macros_json {
                continue;
            }

            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => {}
                _ => continue,
            }

            if suppress_reload.swap(false, Ordering::Relaxed) {
                continue;
            }

            match load_macros(std::path::Path::new(MACROS_PATH)) {
                Ok(new_macros) => {
                    {
                        let mut current = local_macros.lock().unwrap();
                        let old_ids: std::collections::HashSet<_> =
                            current.iter().map(|m| m.id).collect();
                        let new_ids: std::collections::HashSet<_> =
                            new_macros.iter().map(|m| m.id).collect();

                        for m in &new_macros {
                            if old_ids.contains(&m.id) {
                                engine_sender.send(EngineCommand::UpdateMacro(m.clone())).ok();
                            } else {
                                engine_sender.send(EngineCommand::AddMacro(m.clone())).ok();
                            }
                        }
                        for id in old_ids.difference(&new_ids) {
                            engine_sender.send(EngineCommand::DeleteMacro(*id)).ok();
                        }
                        *current = new_macros;
                    }

                    let local_macros = Arc::clone(&local_macros);
                    let _ = ui_weak.upgrade_in_event_loop(move |ui| {
                        let macros = local_macros.lock().unwrap();
                        reload_ui_model(&ui, &macros);
                    });

                    println!("[koakuma] hot-reloaded {MACROS_PATH}");
                }
                Err(e) => {
                    eprintln!("[koakuma] hot-reload failed: {e}; keeping current macros");
                    let msg = format!("[WRN] hot-reload failed: {e}");
                    let _ = ui_weak.upgrade_in_event_loop(move |ui| {
                        let logs_rc = ui.get_logs();
                        if let Some(logs) = logs_rc.as_any().downcast_ref::<VecModel<LogEntry>>() {
                            logs.insert(0, LogEntry { message: msg.into() });
                        }
                    });
                }
            }
        }
    });

    watcher
}

fn reload_ui_model(ui: &MainWindow, macros: &[Macro]) {
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
        ui.set_selected_triggers("".into());
        ui.set_constraint_edit_json("".into());
        ui.set_workflow_edit_json("".into());
    } else if sel >= 0 {
        refresh_editor(ui, macros, sel as usize);
    }

    let logs_rc = ui.get_logs();
    if let Some(logs) = logs_rc.as_any().downcast_ref::<VecModel<LogEntry>>() {
        let msg = format!(
            "[INF] hot-reloaded {} macro(s) from {MACROS_PATH}",
            macros.len()
        );
        logs.insert(0, LogEntry { message: msg.into() });
        while logs.row_count() > 500 {
            logs.remove(logs.row_count() - 1);
        }
    }
}

// ── Persist ───────────────────────────────────────────────────────────────────

fn persist(macros: &[Macro], suppress_reload: &AtomicBool) {
    suppress_reload.store(true, Ordering::Relaxed);
    if let Err(e) = save_macros(std::path::Path::new(MACROS_PATH), macros) {
        eprintln!("[koakuma] failed to save {MACROS_PATH}: {e}");
        suppress_reload.store(false, Ordering::Relaxed);
    }
}

// ── Engine event formatting ───────────────────────────────────────────────────

fn format_engine_event(ev: &EngineEvent) -> String {
    match ev {
        EngineEvent::MacroFired { name, id, .. } => {
            format!("[FIRED] \"{name}\" ({})", &id.to_string()[..8])
        }
        EngineEvent::ActionLog { action, level, message, .. } => {
            let prefix = match level {
                LogLevel::Error => "ERR",
                LogLevel::Warn => "WRN",
                LogLevel::Info => "INF",
                LogLevel::Debug => "DBG",
            };
            format!("[{prefix}] [{action}] {message}")
        }
        EngineEvent::VariableChanged { key, value } => {
            format!("[VAR] {key} = {value:?}")
        }
        EngineEvent::Error { macro_id, message } => match macro_id {
            Some(id) => format!("[ERR] ({}) {message}", &id.to_string()[..8]),
            None => format!("[ERR] {message}"),
        },
    }
}

// ── Default macro factory ─────────────────────────────────────────────────────

fn create_default_macro() -> Macro {
    let actions = vec![ActionConfig::Notify {
        title: "Koakuma".to_string(),
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

// ── Renderer detection ────────────────────────────────────────────────────────

fn select_backend() -> &'static str {
    if std::env::var("SLINT_BACKEND").is_ok() {
        return "custom (SLINT_BACKEND env)";
    }
    let backend = if hardware_gl_available() { "winit-femtovg" } else { "winit-software" };
    // SAFETY: called before Slint initialisation and before spawning any threads
    unsafe {
        std::env::set_var("SLINT_BACKEND", backend);
    }
    backend
}

fn hardware_gl_available() -> bool {
    if std::env::var("LIBGL_ALWAYS_SOFTWARE").ok().as_deref() == Some("1") {
        return false;
    }
    if std::env::var("GALLIUM_DRIVER")
        .ok()
        .map(|d| matches!(d.as_str(), "llvmpipe" | "softpipe" | "swr"))
        .unwrap_or(false)
    {
        return false;
    }
    platform_has_hw_gl()
}

#[cfg(target_os = "linux")]
fn platform_has_hw_gl() -> bool {
    let has_gpu = std::path::Path::new("/dev/dri/renderD128").exists()
        || std::path::Path::new("/dev/dri/card0").exists();
    has_gpu && (std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok())
}

#[cfg(target_os = "windows")]
fn platform_has_hw_gl() -> bool {
    true
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn platform_has_hw_gl() -> bool {
    true
}

#[cfg(target_os = "windows")]
fn start_hooks(
    event_sink: koakuma_core::traits::EventSink,
) -> Vec<Box<dyn koakuma_core::traits::HookProvider>> {
    use koakuma_core::traits::HookProvider;
    use koakuma_hooks::{HotkeyProvider, ProcessProvider, WindowFocusProvider};

    let mut providers: Vec<Box<dyn HookProvider>> = Vec::new();

    let mut hotkey = HotkeyProvider::new();
    if let Err(e) = hotkey.start(event_sink.clone()) {
        eprintln!("[koakuma] hotkey hook failed to start: {e}");
    } else {
        providers.push(Box::new(hotkey));
    }

    let mut window_focus = WindowFocusProvider::new();
    if let Err(e) = window_focus.start(event_sink.clone()) {
        eprintln!("[koakuma] window_focus hook failed to start: {e}");
    } else {
        providers.push(Box::new(window_focus));
    }

    let mut process = ProcessProvider::new();
    if let Err(e) = process.start(event_sink) {
        eprintln!("[koakuma] process hook failed to start: {e}");
    } else {
        providers.push(Box::new(process));
    }

    providers
}
