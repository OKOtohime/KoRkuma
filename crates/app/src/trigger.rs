use korkuma_core::domain::{KeyCombo, ProcessEvent, TriggerConfig};

use crate::{MainWindow, TriggerRow};

// Ordered key list mirroring PropertyInspector.slint's hotkey key DropDownMenu.
// Index 0 is "no key", indices 1-26 are A-Z, 27-38 are F1-F12, 39-48 are 0-9, then specials.
pub const HOTKEY_KEYS: &[&str] = &[
    "", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
    "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
    "Space", "Enter", "Tab", "Escape", "Delete", "Backspace", "Insert",
    "Home", "End", "PageUp", "PageDown", "Left", "Right", "Up", "Down",
];

// Ordered modifier list (index 0 = None).
pub const HOTKEY_MODS: &[&str] = &["None", "Ctrl", "Alt", "Shift", "Win", "Meta"];

// Cron expressions for schedule presets (index 7 = custom).
pub const CRON_PRESETS: &[&str] = &[
    "* * * * *",    // 0 every minute
    "0 * * * *",    // 1 every hour
    "0 0 * * *",    // 2 every day
    "0 0 * * 1-5",  // 3 weekdays
    "0 0 * * 0",    // 4 every week
    "0 0 1 * *",    // 5 every month
    "0 0 1 1 *",    // 6 every year
    "",             // 7 custom (use trigger-cron directly)
];

pub fn format_hotkey(keys: &[KeyCombo]) -> String {
    if keys.is_empty() {
        return "—".into();
    }
    keys.iter()
        .map(|k| {
            let mut parts = k.modifiers.clone();
            parts.push(k.key.clone());
            parts.join("+")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn describe_trigger(t: &TriggerConfig) -> (String, String) {
    match t {
        TriggerConfig::Manual => ("Manual".into(), "manual trigger".into()),
        TriggerConfig::Schedule { cron } => ("Schedule".into(), format!("cron: {cron}")),
        TriggerConfig::Hotkey { keys } => ("Hotkey".into(), format_hotkey(keys)),
        TriggerConfig::WindowFocus { title_pattern, regex } => (
            "WindowFocus".into(),
            format!(
                "window ~ \"{}\"{}",
                title_pattern,
                if *regex { " (regex)" } else { "" }
            ),
        ),
        TriggerConfig::Process { name, event } => (
            "Process".into(),
            format!(
                "{name} {}",
                match event {
                    ProcessEvent::Started => "started",
                    ProcessEvent::Stopped => "stopped",
                }
            ),
        ),
        TriggerConfig::FileChange { path, kind } => {
            ("FileChange".into(), format!("{} {:?}", path.display(), kind))
        }
        TriggerConfig::Custom { provider, .. } => {
            ("Custom".into(), format!("custom:{provider}"))
        }
    }
}

pub fn to_slint_trigger_rows(triggers: &[TriggerConfig]) -> Vec<TriggerRow> {
    triggers
        .iter()
        .map(|t| {
            let (kind, summary) = describe_trigger(t);
            TriggerRow { kind: kind.into(), summary: summary.into() }
        })
        .collect()
}

pub fn default_trigger_config(kind: &str) -> TriggerConfig {
    match kind {
        "Schedule" => TriggerConfig::Schedule { cron: "* * * * *".into() },
        "Hotkey" => TriggerConfig::Hotkey { keys: vec![] },
        "WindowFocus" => TriggerConfig::WindowFocus {
            title_pattern: String::new(),
            regex: false,
        },
        "Process" => TriggerConfig::Process {
            name: String::new(),
            event: ProcessEvent::Started,
        },
        _ => TriggerConfig::Manual,
    }
}

pub fn populate_trigger_fields(ui: &MainWindow, trigger: &TriggerConfig) {
    match trigger {
        TriggerConfig::Manual => {
            ui.set_trigger_kind("Manual".into());
        }
        TriggerConfig::Schedule { cron } => {
            ui.set_trigger_kind("Schedule".into());
            ui.set_trigger_cron(cron.clone().into());
            let preset_idx = CRON_PRESETS[..7]
                .iter()
                .position(|&p| p == cron.as_str())
                .unwrap_or(7) as i32;
            ui.set_trigger_cron_preset_idx(preset_idx);
        }
        TriggerConfig::Hotkey { keys } => {
            ui.set_trigger_kind("Hotkey".into());
            if let Some(combo) = keys.first() {
                let mod1 = combo.modifiers.first().map(String::as_str).unwrap_or("None");
                let mod2 = combo.modifiers.get(1).map(String::as_str).unwrap_or("None");
                let mod1_idx = HOTKEY_MODS.iter().position(|&m| m == mod1).unwrap_or(0) as i32;
                let mod2_idx = HOTKEY_MODS.iter().position(|&m| m == mod2).unwrap_or(0) as i32;
                let key_idx = HOTKEY_KEYS.iter().position(|&k| k == combo.key.as_str()).unwrap_or(0) as i32;
                ui.set_trigger_hotkey_mod1_idx(mod1_idx);
                ui.set_trigger_hotkey_mod2_idx(mod2_idx);
                ui.set_trigger_hotkey_key_idx(key_idx);
            } else {
                ui.set_trigger_hotkey_mod1_idx(0);
                ui.set_trigger_hotkey_mod2_idx(0);
                ui.set_trigger_hotkey_key_idx(0);
            }
        }
        TriggerConfig::WindowFocus { title_pattern, regex } => {
            ui.set_trigger_kind("WindowFocus".into());
            ui.set_trigger_title_pat(title_pattern.clone().into());
            ui.set_trigger_use_regex(*regex);
        }
        TriggerConfig::Process { name, event } => {
            ui.set_trigger_kind("Process".into());
            ui.set_trigger_proc_name(name.clone().into());
            ui.set_trigger_proc_event_idx(match event {
                ProcessEvent::Started => 0,
                ProcessEvent::Stopped => 1,
            });
        }
        TriggerConfig::FileChange { path, .. } => {
            ui.set_trigger_kind("FileChange".into());
            ui.set_trigger_title_pat(path.to_string_lossy().as_ref().into());
        }
        TriggerConfig::Custom { provider, .. } => {
            ui.set_trigger_kind("Custom".into());
            ui.set_trigger_title_pat(provider.clone().into());
        }
    }
}

pub fn build_trigger_from_ui(ui: &MainWindow, kind: &str) -> TriggerConfig {
    match kind {
        "Schedule" => {
            let preset_idx = ui.get_trigger_cron_preset_idx() as usize;
            let cron = if preset_idx < 7 {
                CRON_PRESETS[preset_idx].to_string()
            } else {
                ui.get_trigger_cron().to_string()
            };
            TriggerConfig::Schedule { cron }
        }
        "Hotkey" => {
            let mod1_idx = ui.get_trigger_hotkey_mod1_idx() as usize;
            let mod2_idx = ui.get_trigger_hotkey_mod2_idx() as usize;
            let key_idx = ui.get_trigger_hotkey_key_idx() as usize;
            let key = HOTKEY_KEYS.get(key_idx).map(|&k| k.to_string()).unwrap_or_default();
            if key.is_empty() {
                return TriggerConfig::Hotkey { keys: vec![] };
            }
            let mut mods: Vec<String> = vec![];
            if mod1_idx != 0 {
                mods.push(HOTKEY_MODS[mod1_idx].to_string());
            }
            // deduplicate: only add mod2 if different from mod1
            if mod2_idx != 0 {
                let mod2_str = HOTKEY_MODS[mod2_idx].to_string();
                if !mods.contains(&mod2_str) {
                    mods.push(mod2_str);
                }
            }
            TriggerConfig::Hotkey { keys: vec![KeyCombo { modifiers: mods, key }] }
        }
        "WindowFocus" => TriggerConfig::WindowFocus {
            title_pattern: ui.get_trigger_title_pat().to_string(),
            regex: ui.get_trigger_use_regex(),
        },
        "Process" => TriggerConfig::Process {
            name: ui.get_trigger_proc_name().to_string(),
            event: if ui.get_trigger_proc_event_idx() == 1 {
                ProcessEvent::Stopped
            } else {
                ProcessEvent::Started
            },
        },
        _ => TriggerConfig::Manual,
    }
}
