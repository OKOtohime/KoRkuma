use korkuma_core::engine::{EngineEvent, LogLevel};

pub fn format_engine_event(ev: &EngineEvent) -> String {
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