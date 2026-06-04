use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use notify::{EventKind, RecursiveMode, Watcher};
use slint::{Model, VecModel};

use korkuma_core::{domain::Macro, engine::EngineCommand};
use korkuma_store::load_macros;

use crate::{LogEntry, MainWindow, MACROS_PATH};
use crate::model::reload_ui_model;

pub fn spawn_file_watcher(
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
                        if let Some(logs) =
                            logs_rc.as_any().downcast_ref::<VecModel<LogEntry>>()
                        {
                            logs.insert(0, LogEntry { message: msg.into() });
                        }
                    });
                }
            }
        }
    });

    watcher
}