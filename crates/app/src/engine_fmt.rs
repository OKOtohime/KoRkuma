use korkuma_core::engine::{EngineEvent, LogLevel};

/// A classified engine log line ready to be mapped onto the Slint `LogEntry`.
/// `level` is one of `INFO` | `WARN` | `ERROR` | `DEBUG` | `EVENT` and drives the
/// Logs / Events / Errors split in the bottom drawer.
pub struct LogLine {
    pub level: String,
    pub source: String,
    pub message: String,
}

pub fn format_engine_event(ev: &EngineEvent) -> LogLine {
    match ev {
        EngineEvent::MacroFired { name, id, .. } => LogLine {
            level: "EVENT".into(),
            source: "engine".into(),
            message: format!("fired \"{name}\" ({})", &id.to_string()[..8]),
        },
        EngineEvent::ActionLog { action, level, message, .. } => LogLine {
            level: match level {
                LogLevel::Error => "ERROR",
                LogLevel::Warn => "WARN",
                LogLevel::Info => "INFO",
                LogLevel::Debug => "DEBUG",
            }
            .into(),
            source: action.clone(),
            message: message.clone(),
        },
        EngineEvent::VariableChanged { key, value } => LogLine {
            level: "EVENT".into(),
            source: "state".into(),
            message: format!("{key} = {value:?}"),
        },
        EngineEvent::Error { macro_id, message } => LogLine {
            level: "ERROR".into(),
            source: match macro_id {
                Some(id) => id.to_string()[..8].to_string(),
                None => "engine".into(),
            },
            message: message.clone(),
        },
    }
}
