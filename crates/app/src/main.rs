slint::include_modules!();

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

const MACROS_PATH: &str = "macros.json";

fn main() -> Result<(), slint::PlatformError> {
    // Auto-detect rendering backend before any Slint initialisation.
    let backend = select_backend();

    println!("╔══════════════════════════════════════════╗");
    println!("║   Koakuma  —  Automation Engine (M1.4)   ║");
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
    // If femtovg fails before set_platform commits, silently retry with software.
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

    ui.set_macros(ModelRc::from(macros_model.clone()));
    ui.set_logs(ModelRc::from(logs_model.clone()));

    // Arc<Mutex> so the hot-reload watcher thread can share this list.
    let local_macros: Arc<Mutex<Vec<Macro>>> = Arc::new(Mutex::new(Vec::new()));

    // ── 4. Start engine ───────────────────────────────────────────────────────
    let (engine, _event_sink) = start_engine(Arc::clone(&registry), Arc::clone(&store), {
        let ui_weak = ui_weak.clone();
        move |ev| {
            let msg = format_engine_event(&ev);
            // Cross-thread: schedule a model update on the main (UI) thread.
            let _ = ui_weak.upgrade_in_event_loop(move |ui| {
                let model_rc = ui.get_logs();
                if let Some(model) = model_rc.as_any().downcast_ref::<VecModel<LogEntry>>() {
                    model.insert(
                        0,
                        LogEntry {
                            message: msg.into(),
                        },
                    );
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

    // ── 6. Load macros from macros.json ───────────────────────────────────────
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
            println!("[koakuma] starting with no macros");
        }
    }

    // ── 7. Hot-reload watcher ─────────────────────────────────────────────────
    // suppress_reload is set true before every UI-triggered persist() call so the
    // resulting rename event is silently ignored by the watcher thread.
    let suppress_reload = Arc::new(AtomicBool::new(false));
    let _watcher = spawn_file_watcher(
        Arc::clone(&local_macros),
        engine_sender.clone(),
        ui_weak.clone(),
        Arc::clone(&suppress_reload),
    );

    // ── 8. Wire callbacks ─────────────────────────────────────────────────────

    // add-macro: create a default macro, register with engine, append to models
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

    // delete-macro: remove by index, update engine and models
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
                        ui.set_selected_constraints("".into());
                        ui.set_selected_actions("".into());
                    } else if sel > idx as i32 {
                        ui.set_selected_index(sel - 1);
                    }
                }
                persist(&local_macros.lock().unwrap(), &suppress_reload);
            }
        });
    }

    // toggle-enabled: flip enabled flag, notify engine, update model row
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
                engine_sender
                    .send(EngineCommand::SetEnabled(id, enabled))
                    .ok();
                if let Some(row) = macros_model.row_data(idx) {
                    macros_model.set_row_data(idx, MacroItem { enabled, ..row });
                }
                persist(&local_macros.lock().unwrap(), &suppress_reload);
            }
        });
    }

    // trigger-macro: dispatch manual trigger for the selected macro
    {
        let local_macros = Arc::clone(&local_macros);
        let engine_sender = engine_sender.clone();
        let ui_weak = ui_weak.clone();
        ui.on_trigger_macro(move |idx| {
            let idx = idx as usize;
            let macro_id = local_macros.lock().unwrap().get(idx).map(|m| m.id);
            if let Some(id) = macro_id {
                engine_sender.send(EngineCommand::TriggerManually(id)).ok();
            }
            if let Some(ui) = ui_weak.upgrade() {
                let list = local_macros.lock().unwrap();
                refresh_editor(&ui, &list, idx);
            }
        });
    }

    // macro-selected: populate the 3-tab editor for the clicked row
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
    // EngineHandle::drop sends Shutdown and joins the engine thread.
    drop(engine);

    Ok(())
}

// ── File watcher ──────────────────────────────────────────────────────────────

/// Watches the current directory for changes to [`MACROS_PATH`] and hot-reloads
/// macros into the engine and UI on external edits.
///
/// Returns the watcher handle; must remain live for the duration of the UI loop.
/// Drops automatically on shutdown, closing the internal channel and causing the
/// watcher thread to exit cleanly.
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

            // Only react to events whose path is exactly macros.json (not .tmp).
            let is_macros_json = event
                .paths
                .iter()
                .any(|p| p.file_name().and_then(|n| n.to_str()) == Some(MACROS_PATH));
            if !is_macros_json {
                continue;
            }

            // Only react to create/modify events (including rename-to from atomic write).
            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => {}
                _ => continue,
            }

            // Skip if this event was triggered by our own persist() call.
            if suppress_reload.swap(false, Ordering::Relaxed) {
                continue;
            }

            match load_macros(std::path::Path::new(MACROS_PATH)) {
                Ok(new_macros) => {
                    // Diff against current list and send engine commands.
                    {
                        let mut current = local_macros.lock().unwrap();
                        let old_ids: std::collections::HashSet<_> =
                            current.iter().map(|m| m.id).collect();
                        let new_ids: std::collections::HashSet<_> =
                            new_macros.iter().map(|m| m.id).collect();

                        for m in &new_macros {
                            if old_ids.contains(&m.id) {
                                engine_sender
                                    .send(EngineCommand::UpdateMacro(m.clone()))
                                    .ok();
                            } else {
                                engine_sender.send(EngineCommand::AddMacro(m.clone())).ok();
                            }
                        }
                        for id in old_ids.difference(&new_ids) {
                            engine_sender.send(EngineCommand::DeleteMacro(*id)).ok();
                        }
                        *current = new_macros;
                    }

                    // Rebuild UI model on the main thread.
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
                            logs.insert(
                                0,
                                LogEntry {
                                    message: msg.into(),
                                },
                            );
                        }
                    });
                }
            }
        }
    });

    watcher
}

/// Rebuilds the macros `VecModel`, adjusts the editor selection, and appends a
/// hot-reload log entry.  Must be called on the Slint main thread.
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
        ui.set_selected_constraints("".into());
        ui.set_selected_actions("".into());
    } else if sel >= 0 {
        refresh_editor(ui, macros, sel as usize);
    }

    let logs_rc = ui.get_logs();
    if let Some(logs) = logs_rc.as_any().downcast_ref::<VecModel<LogEntry>>() {
        let msg = format!(
            "[INF] hot-reloaded {} macro(s) from {MACROS_PATH}",
            macros.len()
        );
        logs.insert(
            0,
            LogEntry {
                message: msg.into(),
            },
        );
        while logs.row_count() > 500 {
            logs.remove(logs.row_count() - 1);
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Serialises the macro list to [`MACROS_PATH`] atomically.
///
/// Sets `suppress_reload` before saving so the resulting rename event is
/// ignored by the hot-reload watcher, avoiding a redundant UI refresh.
fn persist(macros: &[Macro], suppress_reload: &AtomicBool) {
    suppress_reload.store(true, Ordering::Relaxed);
    if let Err(e) = save_macros(std::path::Path::new(MACROS_PATH), macros) {
        eprintln!("[koakuma] failed to save {MACROS_PATH}: {e}");
        suppress_reload.store(false, Ordering::Relaxed);
    }
}

/// Updates the 3-tab editor properties for `macros[idx]`.
fn refresh_editor(ui: &MainWindow, macros: &[Macro], idx: usize) {
    if let Some(m) = macros.get(idx) {
        ui.set_selected_triggers(
            serde_json::to_string_pretty(&m.triggers)
                .unwrap_or_default()
                .into(),
        );
        ui.set_selected_constraints(
            serde_json::to_string_pretty(&m.constraints)
                .unwrap_or_default()
                .into(),
        );
        ui.set_selected_actions(
            serde_json::to_string_pretty(&m.actions)
                .unwrap_or_default()
                .into(),
        );
    }
}

/// Formats an [`EngineEvent`] into a short human-readable log line.
fn format_engine_event(ev: &EngineEvent) -> String {
    match ev {
        EngineEvent::MacroFired { name, id, .. } => {
            format!("[FIRED] \"{name}\" ({})", &id.to_string()[..8])
        }
        EngineEvent::ActionLog {
            action,
            level,
            message,
            ..
        } => {
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

/// Builds a ready-to-use default [`Macro`] with a Manual trigger and a Notify action.
///
/// `granted_permissions` is auto-populated from the action list so the macro
/// can execute immediately without a separate authorization step.
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
    }
}

// ── Renderer detection ────────────────────────────────────────────────────────

/// Probes hardware OpenGL availability and sets `SLINT_BACKEND` accordingly.
///
/// Must be called before any Slint initialisation (before `MainWindow::new()`).
/// Returns the chosen backend name for logging.
fn select_backend() -> &'static str {
    // Respect an explicit user override — don't touch it.
    if std::env::var("SLINT_BACKEND").is_ok() {
        return "custom (SLINT_BACKEND env)";
    }
    let backend = if hardware_gl_available() {
        "winit-femtovg"
    } else {
        "winit-software"
    };
    // SAFETY: called before Slint initialisation and before spawning any threads
    // that could concurrently read the environment.
    unsafe {
        std::env::set_var("SLINT_BACKEND", backend);
    }
    backend
}

/// Returns `true` if hardware-accelerated OpenGL is likely available.
///
/// Checks cross-platform Mesa/Gallium env vars first, then delegates to a
/// platform-specific DRI/display-server probe.  Errs on the side of `false`
/// (software fallback) when the outcome is uncertain.
fn hardware_gl_available() -> bool {
    // Explicit software-rendering overrides (CI, Mesa, Gallium, user request).
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

/// Linux probe: a DRM render node must be present *and* a display server must
/// be reachable (winit needs one regardless of the renderer).
#[cfg(target_os = "linux")]
fn platform_has_hw_gl() -> bool {
    let has_gpu = std::path::Path::new("/dev/dri/renderD128").exists()
        || std::path::Path::new("/dev/dri/card0").exists();
    has_gpu && (std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok())
}

/// Windows probe: optimistic default; software env-var overrides (checked above)
/// cover the common VM/RDP cases.
#[cfg(target_os = "windows")]
fn platform_has_hw_gl() -> bool {
    true
}

/// Fallback for other platforms (macOS, etc.): assume hardware is available.
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn platform_has_hw_gl() -> bool {
    true
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
