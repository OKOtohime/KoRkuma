slint::include_modules!();

mod callbacks;
mod engine_fmt;
mod model;
mod setup;
mod tree_model;
mod trigger;
mod watcher;

use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use slint::{Global, Model, ModelRc, StandardListViewItem, VecModel};

use korkuma_actions::register_all as register_actions;
use korkuma_core::{
    domain::Macro,
    engine::EngineCommand,
    engine_loop::start_engine,
    state::StateStore,
};
use korkuma_hooks::register_trigger_specs;
use korkuma_script::{
    register_actions as register_script_actions,
    register_constraints as register_script_constraints,
};
use korkuma_store::{InMemoryStateStore, load_macros};

pub const MACROS_PATH: &str = "macros.json";

/// Value kind shown in the variable inspector's Type column.
fn value_type(v: &korkuma_core::value::Value) -> &'static str {
    use korkuma_core::value::Value;
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Str(_) => "string",
        Value::List(_) => "list",
        Value::Map(_) => "map",
    }
}

fn main() -> Result<(), slint::PlatformError> {
    setup::normalize_lang_for_slint();
    let backend = setup::select_backend();

    println!("╔═════════════╗");
    println!("║   KoRkuma   ║");
    println!("╚═════════════╝");
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
            unsafe { std::env::set_var("SLINT_BACKEND", "winit-software"); }
            MainWindow::new()?
        }
        Err(e) => return Err(e),
    };
    {
        let locale = setup::system_locale();
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
    let permission_model: Rc<VecModel<PermissionRow>> = Rc::new(VecModel::default());
    let var_model: Rc<VecModel<VarRow>> = Rc::new(VecModel::default());

    ui.set_macros(ModelRc::from(macros_model.clone()));
    ui.set_logs(ModelRc::from(logs_model.clone()));
    ui.set_trigger_rows(ModelRc::from(trigger_model.clone()));
    ui.set_constraint_rows(ModelRc::from(constraint_model.clone()));
    ui.set_workflow_rows(ModelRc::from(workflow_model.clone()));
    ui.set_permission_rows(ModelRc::from(permission_model.clone()));
    ui.set_var_rows(ModelRc::from(var_model.clone()));

    LogTableAdapter::get(&ui).on_convert(|entries, category, _version| {
        let rows: Vec<ModelRc<StandardListViewItem>> = entries
            .iter()
            .filter(|e| match category.as_str() {
                "event"   => e.level == "EVENT",
                "problem" => e.level == "ERROR" || e.level == "WARN",
                _         => true,
            })
            .map(|e| ModelRc::new(VecModel::from(vec![
                StandardListViewItem::from(e.level.clone()),
                StandardListViewItem::from(e.source.clone()),
                StandardListViewItem::from(e.message.clone()),
            ])))
            .collect();
        ModelRc::new(VecModel::from(rows))
    });

    let local_macros: Arc<Mutex<Vec<Macro>>> = Arc::new(Mutex::new(Vec::new()));

    // ── 4. Start engine ───────────────────────────────────────────────────────
    let (engine, _event_sink) = start_engine(Arc::clone(&registry), Arc::clone(&store), {
        let ui_weak = ui_weak.clone();
        move |ev| {
            let line = engine_fmt::format_engine_event(&ev);
            let _ = ui_weak.upgrade_in_event_loop(move |ui| {
                model::push_log(&ui, LogEntry {
                    level: line.level.into(),
                    source: line.source.into(),
                    message: line.message.into(),
                });
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
            println!("[korkuma] loaded {} macro(s) from {MACROS_PATH}", loaded.len());
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
        Err(e) => eprintln!("[korkuma] could not load {MACROS_PATH}: {e}"),
    }

    // ── 7. Hot-reload watcher ─────────────────────────────────────────────────
    let suppress_reload = Arc::new(AtomicBool::new(false));
    let _watcher = watcher::spawn_file_watcher(
        Arc::clone(&local_macros),
        engine_sender.clone(),
        ui_weak.clone(),
        Arc::clone(&suppress_reload),
    );

    // ── 8. Wire callbacks ─────────────────────────────────────────────────────
    callbacks::wire_callbacks(
        &ui,
        Arc::clone(&local_macros),
        macros_model,
        engine_sender,
        ui_weak,
        Arc::clone(&suppress_reload),
    );

    // ── 9. Variable monitor: poll StateStore snapshot every second ────────────
    let var_timer = slint::Timer::default();
    {
        let store = Arc::clone(&store);
        let var_model = var_model.clone();
        let ui_weak = ui.as_weak();
        var_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(1000),
            move || {
                let filter = ui_weak
                    .upgrade()
                    .map(|ui| ui.get_var_filter().to_string().to_lowercase())
                    .unwrap_or_default();
                let rows: Vec<VarRow> = store
                    .snapshot()
                    .iter()
                    .filter(|(k, _)| filter.is_empty() || k.to_lowercase().contains(&filter))
                    .map(|(k, v)| VarRow {
                        key: k.clone().into(),
                        value: serde_json::to_string(v).unwrap_or_default().into(),
                        value_type: value_type(v).into(),
                    })
                    .collect();
                model::rebuild_model(&var_model, rows);
            },
        );
    }

    // ── 10. Run UI ────────────────────────────────────────────────────────────
    ui.run()?;
    drop(var_timer);

    // ── 11. Graceful shutdown ─────────────────────────────────────────────────
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