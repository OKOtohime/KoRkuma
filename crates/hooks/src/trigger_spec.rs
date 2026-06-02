//! Cross-platform [`TriggerSpec`] implementations for the three hook event kinds.
//!
//! These specs match against the event payloads produced by the platform
//! hook providers. The payload schemas are:
//!
//! | Event kind | Payload fields |
//! |---|---|
//! | `Hotkey` | `key: Str`, `modifiers: List<Str>`, `vk_code: Int` |
//! | `WindowFocus` | `title: Str`, `exe: Str` |
//! | `Process` | `name: Str`, `pid: Int`, `event: Str("started"\|"stopped")` |

use koakuma_core::domain::{KeyCombo, ProcessEvent, TriggerConfig};
use koakuma_core::event::{Event, EventKind};
use koakuma_core::traits::TriggerSpec;
use koakuma_core::value::Value;

// ── Hotkey ────────────────────────────────────────────────────────────────────

/// Matches [`EventKind::Hotkey`] events against a list of [`KeyCombo`]s (OR semantics).
///
/// Key and modifier names are compared case-insensitively. Modifier order is ignored
/// (set comparison).
///
/// # Examples
///
/// ```rust,no_run
/// use koakuma_hooks::trigger_spec::HotkeyTriggerSpec;
/// use koakuma_core::domain::KeyCombo;
/// use koakuma_core::traits::TriggerSpec;
///
/// let spec = HotkeyTriggerSpec::new(vec![
///     KeyCombo { modifiers: vec!["Ctrl".to_string()], key: "S".to_string() },
/// ]);
/// ```
pub struct HotkeyTriggerSpec {
    keys: Vec<KeyCombo>,
}

impl HotkeyTriggerSpec {
    /// Creates a spec that fires when any of `keys` is matched.
    pub fn new(keys: Vec<KeyCombo>) -> Self {
        Self { keys }
    }
}

impl TriggerSpec for HotkeyTriggerSpec {
    fn subscribed_kinds(&self) -> &[EventKind] {
        &[EventKind::Hotkey]
    }

    fn matches(&self, event: &Event) -> bool {
        if event.kind != EventKind::Hotkey {
            return false;
        }
        let Value::Map(ref payload) = event.payload else {
            return false;
        };
        let Some(Value::Str(key)) = payload.get("key") else {
            return false;
        };
        let event_mods: Vec<String> = match payload.get("modifiers") {
            Some(Value::List(list)) => list
                .iter()
                .filter_map(|v| {
                    if let Value::Str(s) = v {
                        Some(s.to_lowercase())
                    } else {
                        None
                    }
                })
                .collect(),
            _ => vec![],
        };

        self.keys.iter().any(|combo| {
            combo.key.eq_ignore_ascii_case(key)
                && combo.modifiers.len() == event_mods.len()
                && combo
                    .modifiers
                    .iter()
                    .all(|m| event_mods.contains(&m.to_lowercase()))
        })
    }
}

// ── Window focus ──────────────────────────────────────────────────────────────

/// Matches [`EventKind::WindowFocus`] events by window title pattern.
///
/// Matching is a case-insensitive substring search. Full regex support
/// is planned for M1.4 when the script engine is available.
///
/// # Examples
///
/// ```rust,no_run
/// use koakuma_hooks::trigger_spec::WindowFocusTriggerSpec;
/// use koakuma_core::traits::TriggerSpec;
///
/// let spec = WindowFocusTriggerSpec::new("Visual Studio Code".to_string(), false);
/// ```
pub struct WindowFocusTriggerSpec {
    title_pattern: String,
    /// When `true`, treat `title_pattern` as a regex. M1.2 falls back to substring match.
    regex: bool,
}

impl WindowFocusTriggerSpec {
    /// Creates a spec that fires when the foreground window title matches `title_pattern`.
    pub fn new(title_pattern: String, regex: bool) -> Self {
        Self {
            title_pattern,
            regex,
        }
    }
}

impl TriggerSpec for WindowFocusTriggerSpec {
    fn subscribed_kinds(&self) -> &[EventKind] {
        &[EventKind::WindowFocus]
    }

    fn matches(&self, event: &Event) -> bool {
        if event.kind != EventKind::WindowFocus {
            return false;
        }
        let Value::Map(ref payload) = event.payload else {
            return false;
        };
        let Some(Value::Str(title)) = payload.get("title") else {
            return false;
        };
        // regex deferred to M1.4; both branches use substring match for now
        let _ = self.regex;
        title
            .to_lowercase()
            .contains(&self.title_pattern.to_lowercase())
    }
}

// ── Process ───────────────────────────────────────────────────────────────────

/// Matches [`EventKind::Process`] events by executable name and event type.
///
/// Name matching is a case-insensitive substring search, so `"notepad"` matches
/// `"notepad.exe"`.
///
/// # Examples
///
/// ```rust,no_run
/// use koakuma_hooks::trigger_spec::ProcessTriggerSpec;
/// use koakuma_core::domain::ProcessEvent;
/// use koakuma_core::traits::TriggerSpec;
///
/// let spec = ProcessTriggerSpec::new("notepad".to_string(), ProcessEvent::Started);
/// ```
pub struct ProcessTriggerSpec {
    name: String,
    event: ProcessEvent,
}

impl ProcessTriggerSpec {
    /// Creates a spec that fires when a process named `name` transitions via `event`.
    pub fn new(name: String, event: ProcessEvent) -> Self {
        Self { name, event }
    }
}

impl TriggerSpec for ProcessTriggerSpec {
    fn subscribed_kinds(&self) -> &[EventKind] {
        &[EventKind::Process]
    }

    fn matches(&self, event: &Event) -> bool {
        if event.kind != EventKind::Process {
            return false;
        }
        let Value::Map(ref payload) = event.payload else {
            return false;
        };
        let Some(Value::Str(name)) = payload.get("name") else {
            return false;
        };
        let Some(Value::Str(ev_type)) = payload.get("event") else {
            return false;
        };
        let name_match = name.to_lowercase().contains(&self.name.to_lowercase());
        let event_match = match self.event {
            ProcessEvent::Started => ev_type == "started",
            ProcessEvent::Stopped => ev_type == "stopped",
        };
        name_match && event_match
    }
}

// ── Factory functions ─────────────────────────────────────────────────────────

/// Builds a [`HotkeyTriggerSpec`] from [`TriggerConfig::Hotkey`].
pub fn build_hotkey_spec(c: &TriggerConfig) -> Option<Box<dyn TriggerSpec>> {
    if let TriggerConfig::Hotkey { keys } = c {
        Some(Box::new(HotkeyTriggerSpec::new(keys.clone())))
    } else {
        None
    }
}

/// Builds a [`WindowFocusTriggerSpec`] from [`TriggerConfig::WindowFocus`].
pub fn build_window_focus_spec(c: &TriggerConfig) -> Option<Box<dyn TriggerSpec>> {
    if let TriggerConfig::WindowFocus {
        title_pattern,
        regex,
    } = c
    {
        Some(Box::new(WindowFocusTriggerSpec::new(
            title_pattern.clone(),
            *regex,
        )))
    } else {
        None
    }
}

/// Builds a [`ProcessTriggerSpec`] from [`TriggerConfig::Process`].
pub fn build_process_spec(c: &TriggerConfig) -> Option<Box<dyn TriggerSpec>> {
    if let TriggerConfig::Process { name, event } = c {
        Some(Box::new(ProcessTriggerSpec::new(name.clone(), *event)))
    } else {
        None
    }
}
