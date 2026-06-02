//! [`SimulateInputAction`] — synthesizes keyboard and mouse input.
//!
//! The action struct is cross-platform. The actual input injection in
//! `platform_impl` is Windows-only; on other targets the action returns
//! [`ActionError::Failed`] with a clear message.

use async_trait::async_trait;
use koakuma_core::context::ExecContext;
use koakuma_core::domain::{ActionConfig, InputEvent};
use koakuma_core::error::ActionError;
use koakuma_core::permission::{Permission, PermissionSet};
use koakuma_core::traits::{Action, Outcome};

/// Injects a sequence of keyboard and mouse input events into the OS input stream.
///
/// Requires [`Permission::InputSimulation`]. Each [`InputEvent`] in `sequence`
/// is sent in order with no additional delay between events; insert
/// [`ActionConfig::Delay`] actions between sequences if pacing is needed.
///
/// **Config**: [`ActionConfig::SimulateInput`]
///
/// **Platform support**: Windows only in V1. Linux support planned for M2.1.
pub struct SimulateInputAction {
    sequence: Vec<InputEvent>,
}

#[async_trait]
impl Action for SimulateInputAction {
    fn id(&self) -> &'static str {
        "simulate_input"
    }

    fn required_permissions(&self) -> PermissionSet {
        PermissionSet(vec![Permission::InputSimulation])
    }

    async fn execute(&self, ctx: &mut ExecContext) -> Result<Outcome, ActionError> {
        if !ctx.permissions.allows(&Permission::InputSimulation) {
            return Err(ActionError::PermissionDenied("InputSimulation".to_string()));
        }
        platform_impl::send_sequence(&self.sequence).map_err(ActionError::Failed)?;
        Ok(Outcome::Continue)
    }
}

/// Factory: builds [`SimulateInputAction`] from [`ActionConfig::SimulateInput`].
pub fn build(c: &ActionConfig) -> Option<Box<dyn Action>> {
    if let ActionConfig::SimulateInput { sequence } = c {
        Some(Box::new(SimulateInputAction {
            sequence: sequence.clone(),
        }))
    } else {
        None
    }
}

// ── Windows platform implementation ──────────────────────────────────────────

#[cfg(target_os = "windows")]
mod platform_impl {
    use koakuma_core::domain::InputEvent;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
        KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE,
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
        MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEINPUT, SendInput,
        VIRTUAL_KEY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    pub fn send_sequence(sequence: &[InputEvent]) -> Result<(), String> {
        for event in sequence {
            send_one(event)?;
        }
        Ok(())
    }

    fn send_one(event: &InputEvent) -> Result<(), String> {
        let inputs: Vec<INPUT> = match event {
            InputEvent::KeyPress { key } => {
                let vk = name_to_vk(key);
                vec![key_input(vk, KEYBD_EVENT_FLAGS(0))]
            }
            InputEvent::KeyRelease { key } => {
                let vk = name_to_vk(key);
                vec![key_input(vk, KEYEVENTF_KEYUP)]
            }
            InputEvent::Text { text } => {
                // Use KEYEVENTF_UNICODE — no VK lookup needed, works for any Unicode char.
                text.encode_utf16()
                    .flat_map(|scan| {
                        [
                            unicode_input(scan, KEYBD_EVENT_FLAGS(0)),
                            unicode_input(scan, KEYEVENTF_KEYUP),
                        ]
                    })
                    .collect()
            }
            InputEvent::MouseMove { x, y } => {
                let (w, h) = screen_size();
                // Normalize to [0, 65535] as required by MOUSEEVENTF_ABSOLUTE.
                let dx = (x * 65535.0 / w as f64).round() as i32;
                let dy = (y * 65535.0 / h as f64).round() as i32;
                vec![mouse_input(
                    dx,
                    dy,
                    0,
                    MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                )]
            }
            InputEvent::MouseClick { button } => {
                let (down, up) = match button.to_lowercase().as_str() {
                    "right" => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
                    "middle" => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
                    _ => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
                };
                vec![mouse_input(0, 0, 0, down), mouse_input(0, 0, 0, up)]
            }
        };

        if inputs.is_empty() {
            return Ok(());
        }
        let sent = unsafe { SendInput(inputs.as_slice(), std::mem::size_of::<INPUT>() as i32) };
        if sent != inputs.len() as u32 {
            return Err(format!(
                "SendInput: {sent}/{} events injected (UIPI block?)",
                inputs.len()
            ));
        }
        Ok(())
    }

    fn key_input(vk: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn unicode_input(scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: scan,
                    dwFlags: KEYEVENTF_UNICODE | flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn mouse_input(dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn screen_size() -> (i32, i32) {
        unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
    }

    /// Maps a human-readable key name to its Windows Virtual Key code.
    ///
    /// Single ASCII letters and digits are mapped directly via their code point.
    /// Returns 0 for unknown names (input will be a no-op).
    fn name_to_vk(key: &str) -> u16 {
        let up = key.to_ascii_uppercase();
        match up.as_str() {
            "BACKSPACE" | "BS" => 0x08,
            "TAB" => 0x09,
            "ENTER" | "RETURN" => 0x0D,
            "ESCAPE" | "ESC" => 0x1B,
            "SPACE" => 0x20,
            "PAGEUP" => 0x21,
            "PAGEDOWN" => 0x22,
            "END" => 0x23,
            "HOME" => 0x24,
            "LEFT" => 0x25,
            "UP" => 0x26,
            "RIGHT" => 0x27,
            "DOWN" => 0x28,
            "PRINTSCREEN" => 0x2C,
            "INSERT" => 0x2D,
            "DELETE" | "DEL" => 0x2E,
            "F1" => 0x70,
            "F2" => 0x71,
            "F3" => 0x72,
            "F4" => 0x73,
            "F5" => 0x74,
            "F6" => 0x75,
            "F7" => 0x76,
            "F8" => 0x77,
            "F9" => 0x78,
            "F10" => 0x79,
            "F11" => 0x7A,
            "F12" => 0x7B,
            "CTRL" | "CONTROL" | "LCTRL" => 0x11,
            "SHIFT" | "LSHIFT" => 0x10,
            "ALT" | "LALT" => 0x12,
            "WIN" | "LWIN" => 0x5B,
            "RWIN" => 0x5C,
            "RCTRL" => 0xA3,
            "RSHIFT" => 0xA1,
            "RALT" => 0xA5,
            // Single ASCII character (A-Z, 0-9): VK code equals ASCII code point.
            s if s.len() == 1 => s.chars().next().unwrap() as u16,
            _ => 0,
        }
    }
}

// ── Non-Windows stub ──────────────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
mod platform_impl {
    use koakuma_core::domain::InputEvent;

    pub fn send_sequence(_sequence: &[InputEvent]) -> Result<(), String> {
        Err("SimulateInput is not supported on this platform (Windows only in V1; Linux planned for M2.1)".to_string())
    }
}
