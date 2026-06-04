use korkuma_core::domain::{KeyCombo, ProcessEvent, TriggerConfig};

use crate::{MainWindow, TriggerRow};

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

pub fn parse_hotkey_str(s: &str) -> Vec<KeyCombo> {
    let s = s.trim();
    if s.is_empty() {
        return vec![];
    }
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    let key = parts.last().unwrap_or(&s).to_string();
    let modifiers: Vec<String> = parts[..parts.len().saturating_sub(1)]
        .iter()
        .map(|&m| m.to_string())
        .collect();
    vec![KeyCombo { modifiers, key }]
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
        }
        TriggerConfig::Hotkey { keys } => {
            ui.set_trigger_kind("Hotkey".into());
            ui.set_trigger_hotkey(format_hotkey(keys).into());
        }
        TriggerConfig::WindowFocus { title_pattern, regex } => {
            ui.set_trigger_kind("WindowFocus".into());
            ui.set_trigger_title_pat(title_pattern.clone().into());
            ui.set_trigger_use_regex(*regex);
        }
        TriggerConfig::Process { name, event } => {
            ui.set_trigger_kind("Process".into());
            ui.set_trigger_proc_name(name.clone().into());
            ui.set_trigger_proc_event(
                match event {
                    ProcessEvent::Started => "Started",
                    ProcessEvent::Stopped => "Stopped",
                }
                .into(),
            );
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
        "Schedule" => TriggerConfig::Schedule { cron: ui.get_trigger_cron().to_string() },
        "Hotkey" => TriggerConfig::Hotkey {
            keys: parse_hotkey_str(ui.get_trigger_hotkey().as_str()),
        },
        "WindowFocus" => TriggerConfig::WindowFocus {
            title_pattern: ui.get_trigger_title_pat().to_string(),
            regex: ui.get_trigger_use_regex(),
        },
        "Process" => TriggerConfig::Process {
            name: ui.get_trigger_proc_name().to_string(),
            event: if ui.get_trigger_proc_event().as_str() == "Stopped" {
                ProcessEvent::Stopped
            } else {
                ProcessEvent::Started
            },
        },
        _ => TriggerConfig::Manual,
    }
}