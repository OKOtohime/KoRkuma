slint::include_modules!();

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use slint::{Model, ModelRc, VecModel};

use koakuma_core::{
    domain::{ActionConfig, ConstraintExpr, Macro, TriggerConfig},
    engine::{EngineCommand, EngineEvent, LogLevel},
    engine_loop::start_engine,
    permission::PermissionSet,
    state::StateStore,
};
use koakuma_store::{load_macros, save_macros, InMemoryStateStore};
use koakuma_hooks::register_trigger_specs;
use koakuma_actions::register_all as register_actions;

const MACROS_PATH: &str = "macros.json";

fn main() -> Result<(), slint::PlatformError> {
    // Default to software renderer so the app works on VMs without a GPU driver.
    // Override with SLINT_BACKEND=winit-femtovg to enable hardware acceleration.
    if std::env::var("SLINT_BACKEND").is_err() {
        // SAFETY: called before any threads or Slint initialisation.
        unsafe { std::env::set_var("SLINT_BACKEND", "winit-software"); }
    }

    println!("╔══════════════════════════════════════════╗");
    println!("║   Koakuma  —  Automation Engine (M1.3)   ║");
    println!("╚══════════════════════════════════════════╝");

    // ── 1. Registry ───────────────────────────────────────────────────────────
    let mut registry = koakuma_core::registry::Registry::with_builtins();
    register_trigger_specs(&mut registry);
    register_actions(&mut registry);
    let registry = Arc::new(registry);

    // ── 2. State store ────────────────────────────────────────────────────────
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());

    // ── 3. UI & models ────────────────────────────────────────────────────────
    let ui = MainWindow::new()?;
    let ui_weak = ui.as_weak();

    let macros_model: Rc<VecModel<MacroItem>> = Rc::new(VecModel::default());
    let logs_model: Rc<VecModel<LogEntry>> = Rc::new(VecModel::default());

    ui.set_macros(ModelRc::from(macros_model.clone()));
    ui.set_logs(ModelRc::from(logs_model.clone()));

    // Main-thread-only macro list; shared across UI callbacks via Rc<RefCell>.
    let local_macros: Rc<RefCell<Vec<Macro>>> = Rc::new(RefCell::new(Vec::new()));

    // ── 4. Start engine with UI bridge ────────────────────────────────────────
    let (engine, _event_sink) = start_engine(
        Arc::clone(&registry),
        Arc::clone(&store),
        {
            let ui_weak = ui_weak.clone();
            move |ev| {
                let msg = format_engine_event(&ev);
                // Cross-thread: schedule a model update on the main (UI) thread.
                let _ = ui_weak.upgrade_in_event_loop(move |ui| {
                    let model_rc = ui.get_logs();
                    if let Some(model) =
                        model_rc.as_any().downcast_ref::<VecModel<LogEntry>>()
                    {
                        // Prepend so newest entries appear at the top.
                        model.insert(0, LogEntry { message: msg.into() });
                        // Bound the log to 500 entries.
                        while model.row_count() > 500 {
                            model.remove(model.row_count() - 1);
                        }
                    }
                });
            }
        },
    );

    let engine_sender = engine.clone_sender();

    // ── 5. Start hook providers (Windows only) ────────────────────────────────
    #[cfg(target_os = "windows")]
    let _providers = start_hooks(_event_sink);

    // ── 6. Load macros from macros.json ───────────────────────────────────────
    match load_macros(std::path::Path::new(MACROS_PATH)) {
        Ok(loaded) => {
            println!("[koakuma] loaded {} macro(s) from {MACROS_PATH}", loaded.len());
            for m in loaded {
                macros_model.push(MacroItem {
                    id: m.id.to_string().into(),
                    name: m.name.clone().into(),
                    enabled: m.enabled,
                });
                engine.send(EngineCommand::AddMacro(m.clone()));
                local_macros.borrow_mut().push(m);
            }
        }
        Err(e) => {
            eprintln!("[koakuma] could not load {MACROS_PATH}: {e}");
            println!("[koakuma] starting with no macros");
        }
    }

    // ── 7. Wire callbacks ─────────────────────────────────────────────────────

    // add-macro: create a default macro, register with engine, append to models
    {
        let local_macros = local_macros.clone();
        let macros_model = macros_model.clone();
        let engine_sender = engine_sender.clone();
        let ui_weak = ui_weak.clone();
        ui.on_add_macro(move || {
            let m = create_default_macro();
            let new_idx = local_macros.borrow().len() as i32;
            macros_model.push(MacroItem {
                id: m.id.to_string().into(),
                name: m.name.clone().into(),
                enabled: m.enabled,
            });
            engine_sender.send(EngineCommand::AddMacro(m.clone())).ok();
            local_macros.borrow_mut().push(m);
            // Auto-select the new macro and populate the editor.
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_selected_index(new_idx);
                let list = local_macros.borrow();
                refresh_editor(&ui, &list, new_idx as usize);
            }
            persist(&local_macros.borrow());
        });
    }

    // delete-macro: remove by index, update engine and models
    {
        let local_macros = local_macros.clone();
        let macros_model = macros_model.clone();
        let engine_sender = engine_sender.clone();
        let ui_weak = ui_weak.clone();
        ui.on_delete_macro(move |idx| {
            let idx = idx as usize;
            let macro_id = local_macros.borrow().get(idx).map(|m| m.id);
            if let Some(id) = macro_id {
                engine_sender.send(EngineCommand::DeleteMacro(id)).ok();
                macros_model.remove(idx);
                local_macros.borrow_mut().remove(idx);
                if let Some(ui) = ui_weak.upgrade() {
                    let sel = ui.get_selected_index();
                    if sel == idx as i32 {
                        // Deleted the selected row — clear editor.
                        ui.set_selected_index(-1);
                        ui.set_selected_triggers("".into());
                        ui.set_selected_constraints("".into());
                        ui.set_selected_actions("".into());
                    } else if sel > idx as i32 {
                        // Rows above shifted; keep pointing at the same macro.
                        ui.set_selected_index(sel - 1);
                    }
                }
                persist(&local_macros.borrow());
            }
        });
    }

    // toggle-enabled: flip enabled flag, notify engine, update model row
    {
        let local_macros = local_macros.clone();
        let macros_model = macros_model.clone();
        let engine_sender = engine_sender.clone();
        ui.on_toggle_enabled(move |idx, enabled| {
            let idx = idx as usize;
            let macro_id = {
                let mut list = local_macros.borrow_mut();
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
                persist(&local_macros.borrow());
            }
        });
    }

    // trigger-macro: dispatch manual trigger for the selected macro
    {
        let local_macros = local_macros.clone();
        let engine_sender = engine_sender.clone();
        let ui_weak = ui_weak.clone();
        ui.on_trigger_macro(move |idx| {
            let idx = idx as usize;
            let macro_id = local_macros.borrow().get(idx).map(|m| m.id);
            if let Some(id) = macro_id {
                engine_sender.send(EngineCommand::TriggerManually(id)).ok();
            }
            if let Some(ui) = ui_weak.upgrade() {
                let list = local_macros.borrow();
                refresh_editor(&ui, &list, idx);
            }
        });
    }

    // macro-selected: populate the 3-tab editor for the clicked row
    {
        let local_macros = local_macros.clone();
        let ui_weak = ui_weak.clone();
        ui.on_macro_selected(move |idx| {
            if idx < 0 { return; }
            if let Some(ui) = ui_weak.upgrade() {
                let list = local_macros.borrow();
                refresh_editor(&ui, &list, idx as usize);
            }
        });
    }

    // ── 8. Run UI ─────────────────────────────────────────────────────────────
    ui.run()?;

    // ── 9. Graceful shutdown ──────────────────────────────────────────────────
    #[cfg(target_os = "windows")]
    {
        let mut providers = _providers;
        for p in &mut providers {
            p.stop();
        }
    }
    // EngineHandle::drop sends Shutdown and joins the engine thread.
    drop(engine);

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Serialises the macro list to [`MACROS_PATH`] atomically; logs errors to stderr.
fn persist(macros: &[Macro]) {
    if let Err(e) = save_macros(std::path::Path::new(MACROS_PATH), macros) {
        eprintln!("[koakuma] failed to save {MACROS_PATH}: {e}");
    }
}

/// Updates the 3-tab editor properties for `macros[idx]`.
fn refresh_editor(ui: &MainWindow, macros: &[Macro], idx: usize) {
    if let Some(m) = macros.get(idx) {
        ui.set_selected_triggers(
            serde_json::to_string_pretty(&m.triggers).unwrap_or_default().into(),
        );
        ui.set_selected_constraints(
            serde_json::to_string_pretty(&m.constraints).unwrap_or_default().into(),
        );
        ui.set_selected_actions(
            serde_json::to_string_pretty(&m.actions).unwrap_or_default().into(),
        );
    }
}

/// Formats an [`EngineEvent`] into a short human-readable log line.
fn format_engine_event(ev: &EngineEvent) -> String {
    match ev {
        EngineEvent::MacroFired { name, id, .. } => {
            format!("[FIRED] \"{name}\" ({})", &id.to_string()[..8])
        }
        EngineEvent::ActionLog { action, level, message, .. } => {
            let prefix = match level {
                LogLevel::Error => "ERR",
                LogLevel::Warn  => "WRN",
                LogLevel::Info  => "INF",
                LogLevel::Debug => "DBG",
            };
            format!("[{prefix}] [{action}] {message}")
        }
        EngineEvent::VariableChanged { key, value } => {
            format!("[VAR] {key} = {value:?}")
        }
        EngineEvent::Error { macro_id, message } => match macro_id {
            Some(id) => format!("[ERR] ({}) {message}", &id.to_string()[..8]),
            None     => format!("[ERR] {message}"),
        },
    }
}

/// Builds a ready-to-use default [`Macro`] with a Manual trigger and a Notify action.
fn create_default_macro() -> Macro {
    Macro {
        id: uuid::Uuid::new_v4(),
        name: "New Macro".to_string(),
        description: String::new(),
        enabled: true,
        category: None,
        triggers: vec![TriggerConfig::Manual],
        constraints: ConstraintExpr::Always,
        actions: vec![ActionConfig::Notify {
            title: "Koakuma".to_string(),
            body: "Macro fired!".to_string(),
        }],
        granted_permissions: PermissionSet::default(),
    }
}

/// Starts all Windows hook providers and returns them for clean shutdown.
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