use async_trait::async_trait;
use korkuma_core::domain::{TargetSelector, UiOp};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT, SendInput,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

use crate::backend::{InteractionBackend, ResolvedTarget, TargetInner, Tier, UiNode};
use crate::error::BackendError;

/// Windows SendInput foreground-synthetic fallback.
///
/// Brings the target window to the foreground first (`SetForegroundWindow`), then
/// injects input events via `SendInput`.  This *will* steal focus from the user
/// and requires the `ForegroundTakeover` permission — it is only used by the
/// negotiator when the `Degrade` policy is active and no `Background`-tier backend
/// is available.
pub struct SendInputBackend;

impl SendInputBackend {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl InteractionBackend for SendInputBackend {
    fn id(&self) -> &'static str {
        "windows_sendinput"
    }

    async fn resolve(&self, sel: &TargetSelector) -> Result<Vec<ResolvedTarget>, BackendError> {
        use super::uia::UiaBackend;
        UiaBackend::new().resolve(sel).await.map(|mut targets| {
            for t in &mut targets {
                t.backend_id = "windows_sendinput";
            }
            targets
        })
    }

    fn capability(&self, t: &ResolvedTarget) -> Tier {
        match &t.inner {
            TargetInner::WindowHandle { .. } => Tier::ForegroundSynthetic,
            _ => Tier::Unsupported,
        }
    }

    async fn invoke(&self, t: &ResolvedTarget, op: &UiOp) -> Result<(), BackendError> {
        let hwnd_val = match &t.inner {
            TargetInner::WindowHandle { hwnd } => *hwnd,
            _ => return Err(BackendError::NotSupported("not a window handle".into())),
        };
        let op = op.clone();
        tokio::task::spawn_blocking(move || sendinput_invoke_sync(hwnd_val, &op))
            .await
            .map_err(|e| BackendError::Internal(format!("spawn_blocking: {e}")))?
    }

    async fn enumerate(&self, _t: &ResolvedTarget) -> Result<Vec<UiNode>, BackendError> {
        Ok(vec![])
    }
}

fn sendinput_invoke_sync(hwnd_val: isize, op: &UiOp) -> Result<(), BackendError> {
    unsafe {
        // Bring window to foreground before injecting input
        SetForegroundWindow(HWND(hwnd_val as _));
        // Small delay for focus to settle
        std::thread::sleep(std::time::Duration::from_millis(50));

        match op {
            UiOp::Click { .. } => {
                let inputs = [
                    mouse_input(MOUSEEVENTF_LEFTDOWN),
                    mouse_input(MOUSEEVENTF_LEFTUP),
                ];
                send_inputs(&inputs)?;
            }
            UiOp::SendKeys { keys } => {
                for combo in keys {
                    // Press modifiers
                    let mut events: Vec<INPUT> = combo
                        .modifiers
                        .iter()
                        .filter_map(|m| modifier_vk(m))
                        .map(|vk| key_input(vk, false))
                        .collect();
                    // Press main key via unicode
                    for ch in combo.key.chars() {
                        events.push(unicode_input(ch as u16, false));
                        events.push(unicode_input(ch as u16, true));
                    }
                    // Release modifiers
                    for m in combo.modifiers.iter().rev() {
                        if let Some(vk) = modifier_vk(m) {
                            events.push(key_input(vk, true));
                        }
                    }
                    send_inputs(&events)?;
                }
            }
            UiOp::SetText { text, .. } => {
                // Type each character via Unicode injection
                let events: Vec<INPUT> = text
                    .encode_utf16()
                    .flat_map(|ch| [unicode_input(ch, false), unicode_input(ch, true)])
                    .collect();
                send_inputs(&events)?;
            }
            _ => {
                return Err(BackendError::NotSupported(
                    "SendInput backend only supports Click, SendKeys, SetText".into(),
                ))
            }
        }
        Ok(())
    }
}

unsafe fn send_inputs(inputs: &[INPUT]) -> Result<(), BackendError> {
    let sent = SendInput(inputs, std::mem::size_of::<INPUT>() as i32);
    if sent != inputs.len() as u32 {
        Err(BackendError::Internal(format!(
            "SendInput: only {sent}/{} events sent",
            inputs.len()
        )))
    } else {
        Ok(())
    }
}

fn mouse_input(flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

fn key_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: if key_up { KEYEVENTF_KEYUP } else { Default::default() },
                ..Default::default()
            },
        },
    }
}

fn unicode_input(ch: u16, key_up: bool) -> INPUT {
    let mut flags = KEYEVENTF_UNICODE;
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wScan: ch,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

fn modifier_vk(name: &str) -> Option<VIRTUAL_KEY> {
    match name.to_uppercase().as_str() {
        "CTRL" | "CONTROL" => Some(windows::Win32::UI::Input::KeyboardAndMouse::VK_CONTROL),
        "SHIFT" => Some(windows::Win32::UI::Input::KeyboardAndMouse::VK_SHIFT),
        "ALT" => Some(windows::Win32::UI::Input::KeyboardAndMouse::VK_MENU),
        "WIN" | "META" => Some(windows::Win32::UI::Input::KeyboardAndMouse::VK_LWIN),
        _ => None,
    }
}
