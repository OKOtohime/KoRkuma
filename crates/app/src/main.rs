slint::include_modules!();

mod tree_model;

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use notify::{EventKind, RecursiveMode, Watcher};
use slint::{Model, ModelRc, VecModel};

use korkuma_actions::register_all as register_actions;
use korkuma_core::{
    domain::{
        ActionConfig, ConstraintExpr, KeyCombo, Macro, ProcessEvent, TriggerConfig,
    },
    engine::{EngineCommand, EngineEvent, LogLevel},
    engine_loop::start_engine,
    permission::aggregate_from_configs,
    state::StateStore,
};
use korkuma_hooks::register_trigger_specs;
use korkuma_script::{
    register_actions as register_script_actions,
    register_constraints as register_script_constraints,
};
use korkuma_store::{InMemoryStateStore, load_macros, save_macros};

use tree_model::{
    ConstraintTreeRow, WorkflowTreeRow,
    add_constraint_leaf, default_constraint_config,
    delete_constraint_at, update_constraint_leaf,
    wrap_constraint_at, move_constraint_node,
    add_workflow_action, add_workflow_if, add_workflow_parallel,
    default_action_config, delete_workflow_at, flatten_constraint,
    flatten_workflow, update_workflow_action, move_workflow_node,
};

const MACROS_PATH: &str = "macros.json";

fn main() -> Result<(), slint::PlatformError> {
    // Slint's text layout engine reads LANG via sys_locale at first text-render time and
    // passes the language tag to ICU4X for line-break segmentation.  For CJK languages
    // (notably Japanese) the required ML model is not bundled in Slint's ICU4X data, so
    // ICU4X prints "No segmentation model for language: ja" on startup.  Since this app
    // ships English-only UI with no CJK translations, normalise LANG to en_US before any
    // Slint/ICU4X initialisation so that the English segmenter (always available) is used.
    //
    // SAFETY: called before Slint initialises its platform and before any threads are spawned.
    normalize_lang_for_slint();

    let backend = select_backend();

    println!("╔════════════════════╗");
    println!("║   KoRkuma (M2.4)   ║");
    println!("╚════════════════════╝");
    println!("[korkuma] renderer: {backend}");

    // ── 1. Registry ───────────────────────────────────────────────────────────
    let mut registry = korkuma_core::registry::Registry::with_builtins();
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
            eprintln!("[korkuma] femtovg init failed ({e}); falling back to software renderer");
            // SAFETY: engine thread not yet spawned; Slint platform not yet committed.
            unsafe {
                std::env::set_var("SLINT_BACKEND", "winit-software");
            }
            MainWindow::new()?
        }
        Err(e) => return Err(e),
    };
    {
        let locale = system_locale();
        if let Err(e) = slint::select_bundled_translation(&locale) {
            eprintln!("[korkuma] i18n: no bundled translation for '{locale}': {e}");
        }
    }
    let ui_weak = ui.as_weak();

    let macros_model: Rc<VecModel<MacroItem>> = Rc::new(VecModel::default());
    let logs_model: Rc<VecModel<LogEntry>> = Rc::new(VecModel::default());
    let trigger_model: Rc<VecModel<TriggerRow>> = Rc::new(VecModel::default());
    let constraint_model: Rc<VecModel<ConstraintRow>> = Rc::new(VecModel::default());
    let workflow_model: Rc<VecModel<WorkflowRow>> = Rc::new(VecModel::default());

    ui.set_macros(ModelRc::from(macros_model.clone()));
    ui.set_logs(ModelRc::from(logs_model.clone()));
    ui.set_trigger_rows(ModelRc::from(trigger_model.clone()));
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
                "[korkuma] loaded {} macro(s) from {MACROS_PATH}",
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
            eprintln!("[korkuma] could not load {MACROS_PATH}: {e}");
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
                        ui.set_macro_name("".into());
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
                let node_sel = ui.get_constraint_selected();
                let pos = if node_sel >= 0 { Some(node_sel as usize) } else { None };
                m.constraints = wrap_constraint_at(m.constraints.clone(), pos, kind.as_str());
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
                            use korkuma_core::domain::{OnNoBackground, TargetSelector};
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

    // ── move-macro-up / move-macro-down ───────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let macros_model = macros_model.clone();
        let ui_weak = ui_weak.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_move_macro_up(move |idx| {
            let idx = idx as usize;
            if idx == 0 { return; }
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
                if sel == idx as i32 { ui.set_selected_index(sel - 1); }
                else if sel == idx as i32 - 1 { ui.set_selected_index(sel + 1); }
            }
            persist(&local_macros.lock().unwrap(), &suppress_reload);
        });
    }
    {
        let local_macros = Arc::clone(&local_macros);
        let macros_model = macros_model.clone();
        let ui_weak = ui_weak.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_move_macro_down(move |idx| {
            let idx = idx as usize;
            let len = local_macros.lock().unwrap().len();
            if idx + 1 >= len { return; }
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
                if sel == idx as i32 { ui.set_selected_index(sel + 1); }
                else if sel == idx as i32 + 1 { ui.set_selected_index(sel - 1); }
            }
            persist(&local_macros.lock().unwrap(), &suppress_reload);
        });
    }

    // ── trigger-select ────────────────────────────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        ui.on_trigger_select(move |tidx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let midx = ui.get_selected_index();
            if midx < 0 { return; }
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
            if midx < 0 { return; }
            let midx = midx as usize;
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(midx) {
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
            if midx < 0 { return; }
            let midx = midx as usize;
            let tidx = tidx as usize;
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(midx) {
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
            if midx < 0 || tidx < 0 { return; }
            let midx = midx as usize;
            let tidx = tidx as usize;
            let kind = ui.get_trigger_kind().to_string();
            let new_trigger = build_trigger_from_ui(&ui, &kind);
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(midx) {
                if let Some(t) = m.triggers.get_mut(tidx) {
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

    // ── constraint-move-up / constraint-move-down ─────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_constraint_move_up(move |idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 { return; }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                m.constraints = move_constraint_node(m.constraints.clone(), idx as usize, true);
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
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_constraint_move_down(move |idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 { return; }
            let mut list = local_macros.lock().unwrap();
            if let Some(m) = list.get_mut(sel as usize) {
                m.constraints = move_constraint_node(m.constraints.clone(), idx as usize, false);
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

    // ── workflow-move-up / workflow-move-down ─────────────────────────────────
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_workflow_move_up(move |idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 { return; }
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
    {
        let local_macros = Arc::clone(&local_macros);
        let ui_weak = ui_weak.clone();
        let engine_sender = engine_sender.clone();
        let suppress_reload = Arc::clone(&suppress_reload);
        ui.on_workflow_move_down(move |idx| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let sel = ui.get_selected_index();
            if sel < 0 { return; }
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

// ── Trigger model helpers ─────────────────────────────────────────────────────

fn format_hotkey(keys: &[KeyCombo]) -> String {
    if keys.is_empty() {
        return "—".into();
    }
    keys.iter()
        .map(|k| {
            let mut parts = k.modifiers.clone();
            parts.push(k.key.clone());
            parts.join("+")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn parse_hotkey_str(s: &str) -> Vec<KeyCombo> {
    let s = s.trim();
    if s.is_empty() {
        return vec![];
    }
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    let key = parts.last().unwrap_or(&s).to_string();
    let modifiers: Vec<String> = parts[..parts.len().saturating_sub(1)]
        .iter()
        .map(|&m| m.to_string())
        .collect();
    vec![KeyCombo { modifiers, key }]
}

fn describe_trigger(t: &TriggerConfig) -> (String, String) {
    match t {
        TriggerConfig::Manual => ("Manual".into(), "manual trigger".into()),
        TriggerConfig::Schedule { cron } => ("Schedule".into(), format!("cron: {cron}")),
        TriggerConfig::Hotkey { keys } => ("Hotkey".into(), format_hotkey(keys)),
        TriggerConfig::WindowFocus { title_pattern, regex } => (
            "WindowFocus".into(),
            format!(
                "window ~ \"{}\"{}",
                title_pattern,
                if *regex { " (regex)" } else { "" }
            ),
        ),
        TriggerConfig::Process { name, event } => (
            "Process".into(),
            format!(
                "{name} {}",
                match event {
                    ProcessEvent::Started => "started",
                    ProcessEvent::Stopped => "stopped",
                }
            ),
        ),
        TriggerConfig::FileChange { path, kind } => (
            "FileChange".into(),
            format!("{} {:?}", path.display(), kind),
        ),
        TriggerConfig::Custom { provider, .. } => {
            ("Custom".into(), format!("custom:{provider}"))
        }
    }
}

fn to_slint_trigger_rows(triggers: &[TriggerConfig]) -> Vec<TriggerRow> {
    triggers
        .iter()
        .map(|t| {
            let (kind, summary) = describe_trigger(t);
            TriggerRow { kind: kind.into(), summary: summary.into() }
        })
        .collect()
}

fn default_trigger_config(kind: &str) -> TriggerConfig {
    match kind {
        "Schedule" => TriggerConfig::Schedule { cron: "* * * * *".into() },
        "Hotkey" => TriggerConfig::Hotkey { keys: vec![] },
        "WindowFocus" => TriggerConfig::WindowFocus { title_pattern: String::new(), regex: false },
        "Process" => TriggerConfig::Process { name: String::new(), event: ProcessEvent::Started },
        _ => TriggerConfig::Manual,
    }
}

fn populate_trigger_fields(ui: &MainWindow, trigger: &TriggerConfig) {
    match trigger {
        TriggerConfig::Manual => {
            ui.set_trigger_kind("Manual".into());
        }
        TriggerConfig::Schedule { cron } => {
            ui.set_trigger_kind("Schedule".into());
            ui.set_trigger_cron(cron.clone().into());
        }
        TriggerConfig::Hotkey { keys } => {
            ui.set_trigger_kind("Hotkey".into());
            ui.set_trigger_hotkey(format_hotkey(keys).into());
        }
        TriggerConfig::WindowFocus { title_pattern, regex } => {
            ui.set_trigger_kind("WindowFocus".into());
            ui.set_trigger_title_pat(title_pattern.clone().into());
            ui.set_trigger_use_regex(*regex);
        }
        TriggerConfig::Process { name, event } => {
            ui.set_trigger_kind("Process".into());
            ui.set_trigger_proc_name(name.clone().into());
            ui.set_trigger_proc_event(
                match event {
                    ProcessEvent::Started => "Started",
                    ProcessEvent::Stopped => "Stopped",
                }
                .into(),
            );
        }
        TriggerConfig::FileChange { path, .. } => {
            ui.set_trigger_kind("FileChange".into());
            ui.set_trigger_title_pat(path.to_string_lossy().as_ref().into());
        }
        TriggerConfig::Custom { provider, .. } => {
            ui.set_trigger_kind("Custom".into());
            ui.set_trigger_title_pat(provider.clone().into());
        }
    }
}

fn build_trigger_from_ui(ui: &MainWindow, kind: &str) -> TriggerConfig {
    match kind {
        "Schedule" => TriggerConfig::Schedule { cron: ui.get_trigger_cron().to_string() },
        "Hotkey" => TriggerConfig::Hotkey {
            keys: parse_hotkey_str(ui.get_trigger_hotkey().as_str()),
        },
        "WindowFocus" => TriggerConfig::WindowFocus {
            title_pattern: ui.get_trigger_title_pat().to_string(),
            regex: ui.get_trigger_use_regex(),
        },
        "Process" => TriggerConfig::Process {
            name: ui.get_trigger_proc_name().to_string(),
            event: if ui.get_trigger_proc_event().as_str() == "Stopped" {
                ProcessEvent::Stopped
            } else {
                ProcessEvent::Started
            },
        },
        _ => TriggerConfig::Manual,
    }
}

// ── Editor refresh ────────────────────────────────────────────────────────────

fn refresh_editor(ui: &MainWindow, macros: &[Macro], idx: usize) {
    let Some(m) = macros.get(idx) else { return };

    ui.set_macro_name(m.name.clone().into());

    // Triggers: flat list model
    let t_rows = to_slint_trigger_rows(&m.triggers);
    let t_rc = ui.get_trigger_rows();
    if let Some(model) = t_rc.as_any().downcast_ref::<VecModel<TriggerRow>>() {
        rebuild_model(model, t_rows);
    }
    ui.set_trigger_selected(-1);
    ui.set_trigger_kind("Manual".into());

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
                    eprintln!("[korkuma] watcher error: {e}");
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

                    println!("[korkuma] hot-reloaded {MACROS_PATH}");
                }
                Err(e) => {
                    eprintln!("[korkuma] hot-reload failed: {e}; keeping current macros");
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
        ui.set_macro_name("".into());
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
        eprintln!("[korkuma] failed to save {MACROS_PATH}: {e}");
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

// ── Locale detection ─────────────────────────────────────────────────────────

/// Normalise `LANG` to `en_US.UTF-8` when the detected language has no bundled ICU4X
/// segmentation model in Slint (e.g. Japanese, Chinese, Korean).  Must be called before
/// Slint or ICU4X initialise so the override takes effect on first text layout.
fn normalize_lang_for_slint() {
    let lang = std::env::var("LANG").unwrap_or_default();
    let tag = lang.split('.').next().unwrap_or("").to_ascii_lowercase();
    let lang_code = tag.split('_').next().unwrap_or("");
    // ICU4X bundles segmentation models for Latin-script languages; CJK and others that rely
    // on ML-based word-breaking (ja, zh, th, km, …) are not included in Slint's data bundle.
    let needs_ml_segmenter = matches!(lang_code, "ja" | "zh" | "th" | "km" | "lo" | "my");
    if needs_ml_segmenter {
        // SAFETY: callers guarantee this runs before any threads are spawned
        unsafe { std::env::set_var("LANG", "en_US.UTF-8"); }
    }
}

fn system_locale() -> String {
    // "zh_CN.UTF-8" → "zh-CN"
    let raw = std::env::var("LANG")
        .or_else(|_| std::env::var("LANGUAGE"))
        .or_else(|_| std::env::var("LC_ALL"))
        .unwrap_or_else(|_| "en".to_string());
    raw.split('.').next().unwrap_or(&raw).replace('_', "-")
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
    event_sink: korkuma_core::traits::EventSink,
) -> Vec<Box<dyn korkuma_core::traits::HookProvider>> {
    use korkuma_core::traits::HookProvider;
    use korkuma_hooks::{HotkeyProvider, ProcessProvider, WindowFocusProvider};

    let mut providers: Vec<Box<dyn HookProvider>> = Vec::new();

    let mut hotkey = HotkeyProvider::new();
    if let Err(e) = hotkey.start(event_sink.clone()) {
        eprintln!("[korkuma] hotkey hook failed to start: {e}");
    } else {
        providers.push(Box::new(hotkey));
    }

    let mut window_focus = WindowFocusProvider::new();
    if let Err(e) = window_focus.start(event_sink.clone()) {
        eprintln!("[korkuma] window_focus hook failed to start: {e}");
    } else {
        providers.push(Box::new(window_focus));
    }

    let mut process = ProcessProvider::new();
    if let Err(e) = process.start(event_sink) {
        eprintln!("[korkuma] process hook failed to start: {e}");
    } else {
        providers.push(Box::new(process));
    }

    providers
}
