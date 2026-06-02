use std::time::SystemTime;
use crate::value::Value;

/// Coarse-grained event category used as the primary routing key in [`router::EventRouter`].
///
/// `EventRouter` indexes macros by `EventKind` so each incoming event visits only
/// macros that subscribe to that kind — O(1) lookup instead of a full broadcast.
///
/// # Examples
///
/// ```rust
/// use koakuma_core::event::EventKind;
///
/// let kind = EventKind::Hotkey;
/// assert_eq!(kind, EventKind::Hotkey);
/// assert_ne!(kind, EventKind::Timer);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventKind {
    Hotkey,
    WindowFocus,
    Process,
    Timer,
    FileChange,
    Manual,
    Custom,
}

/// A single event produced by a [`traits::HookProvider`] and routed through the pipeline.
///
/// # Examples
///
/// ```rust
/// use koakuma_core::event::{Event, EventKind};
/// use koakuma_core::value::Value;
/// use std::time::SystemTime;
///
/// let event = Event {
///     kind: EventKind::Manual,
///     source: "ui".to_string(),
///     timestamp: SystemTime::UNIX_EPOCH,
///     payload: Value::Null,
/// };
/// assert_eq!(event.kind, EventKind::Manual);
/// assert_eq!(event.source, "ui");
/// ```
#[derive(Clone, Debug)]
pub struct Event {
    pub kind: EventKind,
    pub source: String,
    pub timestamp: SystemTime,
    pub payload: Value,
}