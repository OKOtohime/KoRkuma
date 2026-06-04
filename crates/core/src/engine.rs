use crate::domain::{Macro, MacroId};
use crate::value::Value;
use std::time::SystemTime;

/// Log severity level for action execution messages emitted via [`EngineEvent::ActionLog`].
///
/// # Examples
///
/// ```rust
/// use koakuma_core::engine::LogLevel;
///
/// let level = LogLevel::Warn;
/// assert!(matches!(level, LogLevel::Warn));
/// assert_ne!(level, LogLevel::Error);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// Commands sent from the UI thread to the engine thread.
pub enum EngineCommand {
    AddMacro(Macro),
    UpdateMacro(Macro),
    DeleteMacro(MacroId),
    SetEnabled(MacroId, bool),
    TriggerManually(MacroId),
    /// Like `TriggerManually` but executes with `dry_run = true` — logs what
    /// each action *would* do without performing side-effecting operations.
    DryRunMacro(MacroId),
    QuerySnapshot(crossbeam_channel::Sender<EngineSnapshot>),
    Shutdown,
}

/// Events sent from the engine thread back to the UI thread via
/// `slint::invoke_from_event_loop` / `Weak::upgrade_in_event_loop`.
#[derive(Debug)]
pub enum EngineEvent {
    MacroFired {
        id: MacroId,
        name: String,
        at: SystemTime,
    },
    ActionLog {
        macro_id: MacroId,
        action: String,
        level: LogLevel,
        message: String,
    },
    VariableChanged {
        key: String,
        value: Value,
    },
    Error {
        macro_id: Option<MacroId>,
        message: String,
    },
}

/// A point-in-time snapshot of the engine state, returned by [`EngineCommand::QuerySnapshot`].
///
/// # Examples
///
/// ```rust
/// use koakuma_core::engine::EngineSnapshot;
///
/// let snap = EngineSnapshot { macros: vec![] };
/// assert!(snap.macros.is_empty());
/// ```
pub struct EngineSnapshot {
    pub macros: Vec<Macro>,
}
