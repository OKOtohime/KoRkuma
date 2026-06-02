slint::include_modules!();

use std::sync::Arc;

use koakuma_core::{
    engine::{EngineCommand, EngineEvent, LogLevel},
    engine_loop::start_engine,
    state::StateStore,
};
use koakuma_store::InMemoryStateStore;
use koakuma_hooks::register_trigger_specs;
use koakuma_actions::register_all as register_actions;

fn main() -> Result<(), slint::PlatformError> {
    // Default to the software renderer so the app works on VMs and machines
    // without a GPU/OpenGL driver. Override with SLINT_BACKEND=winit-femtovg
    // (or winit-skia) to use hardware acceleration on capable machines.
    if std::env::var("SLINT_BACKEND").is_err() {
        // SAFETY: called before any threads or Slint initialization.
        unsafe { std::env::set_var("SLINT_BACKEND", "winit-software"); }
    }

    println!("╔══════════════════════════════════════════╗");
    println!("║   Koakuma  —  Automation Engine (M1.2)   ║");
    println!("╚══════════════════════════════════════════╝");

    // ── 1. Build Registry ────────────────────────────────────────────────────
    let mut registry = koakuma_core::registry::Registry::with_builtins();
    register_trigger_specs(&mut registry);
    register_actions(&mut registry);
    let registry = Arc::new(registry);

    // ── 2. State store ────────────────────────────────────────────────────────
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());

    // ── 3. Start engine ───────────────────────────────────────────────────────
    let (engine, _event_sink) = start_engine(
        Arc::clone(&registry),
        Arc::clone(&store),
        // M1.2: log to stdout. M1.3 will replace this with invoke_from_event_loop.
        |ev| match ev {
            EngineEvent::MacroFired { name, id, .. } => {
                println!("[koakuma] FIRED  \"{name}\" ({id})");
            }
            EngineEvent::ActionLog { action, level, message, .. } => {
                let prefix = match level {
                    LogLevel::Error => "ERROR",
                    LogLevel::Warn  => "WARN ",
                    LogLevel::Info  => "INFO ",
                    LogLevel::Debug => "DEBUG",
                };
                println!("[koakuma] {prefix} [{action}] {message}");
            }
            EngineEvent::VariableChanged { key, value } => {
                println!("[koakuma] VAR    {key} = {value:?}");
            }
            EngineEvent::Error { macro_id, message } => {
                eprintln!("[koakuma] ERROR  (macro: {macro_id:?}): {message}");
            }
        },
    );

    // ── 4. Load macros from macros.json (if present) ─────────────────────────
    load_macros_json(&engine);

    // ── 5. Start hook providers (Windows only) ────────────────────────────────
    // Each provider gets its own clone of event_sink so it can push events
    // into the engine's channel independently.
    #[cfg(target_os = "windows")]
    let _providers = start_hooks(_event_sink);

    // ── 6. Run Slint UI ───────────────────────────────────────────────────────
    let ui = MainWindow::new()?;
    ui.run()?;

    // ── 7. Graceful shutdown ──────────────────────────────────────────────────
    // Providers stop when _providers is dropped (they don't impl Drop, so the
    // engine Shutdown is sent by EngineHandle::drop at end of scope).
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

/// Loads `macros.json` from the current working directory and registers each
/// macro with the engine. Silently skips if the file is missing or malformed.
///
/// Format: a JSON array of serialized [`Macro`](koakuma_core::domain::Macro) objects.
/// See `DESIGN.md §8` for the full schema; `macros.json.example` in the repo root
/// shows a minimal working configuration.
fn load_macros_json(engine: &koakuma_core::engine_loop::EngineHandle) {
    let path = std::path::Path::new("macros.json");
    if !path.exists() {
        println!("[koakuma] macros.json not found — running with no macros");
        return;
    }
    let data = match std::fs::read_to_string(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[koakuma] failed to read macros.json: {e}");
            return;
        }
    };
    let macros: Vec<koakuma_core::domain::Macro> = match serde_json::from_str(&data) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[koakuma] failed to parse macros.json: {e}");
            return;
        }
    };
    println!("[koakuma] loaded {} macro(s) from macros.json:", macros.len());
    for m in macros {
        let trigger_summary = m.triggers.iter()
            .map(trigger_label)
            .collect::<Vec<_>>()
            .join(" | ");
        let status = if m.enabled { "ON " } else { "OFF" };
        println!("  [{status}] \"{}\"  triggers: {trigger_summary}  actions: {}",
            m.name, m.actions.len());
        engine.send(EngineCommand::AddMacro(m));
    }
}

fn trigger_label(tc: &koakuma_core::domain::TriggerConfig) -> String {
    use koakuma_core::domain::TriggerConfig;
    match tc {
        TriggerConfig::Hotkey { keys } => {
            let combos: Vec<String> = keys.iter().map(|k| {
                if k.modifiers.is_empty() {
                    k.key.clone()
                } else {
                    format!("{}+{}", k.modifiers.join("+"), k.key)
                }
            }).collect();
            format!("Hotkey({})", combos.join("/"))
        }
        TriggerConfig::WindowFocus { title_pattern, .. } => format!("WindowFocus({title_pattern})"),
        TriggerConfig::Process { name, event } => format!("Process({name} {event:?})"),
        TriggerConfig::Schedule { cron } => format!("Schedule({cron})"),
        TriggerConfig::FileChange { path, .. } => format!("FileChange({})", path.display()),
        TriggerConfig::Manual => "Manual".to_string(),
        TriggerConfig::Custom { provider, .. } => format!("Custom({provider})"),
    }
}

/// Starts all Windows hook providers and returns them so they can be stopped
/// before the engine shuts down.
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
        eprintln!("[koakuma] window focus hook failed to start: {e}");
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
