use async_trait::async_trait;
use korkuma_core::domain::{TargetSelector, UiOp};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    PostMessageW, WM_LBUTTONDOWN, WM_LBUTTONUP,
};

use crate::backend::{InteractionBackend, ResolvedTarget, TargetInner, Tier, UiNode};
use crate::error::BackendError;

/// Windows PostMessage backend (Win32 legacy fallback).
///
/// Posts `WM_LBUTTONDOWN`/`WM_LBUTTONUP` messages to a target window without
/// activating it (`Tier::Background`).  This is reliable for classic Win32
/// controls; modern frameworks (WPF, WinForms, UWP) are better served by
/// [`UiaBackend`](super::uia::UiaBackend).
pub struct WinMsgBackend;

impl WinMsgBackend {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl InteractionBackend for WinMsgBackend {
    fn id(&self) -> &'static str {
        "windows_winmsg"
    }

    async fn resolve(&self, sel: &TargetSelector) -> Result<Vec<ResolvedTarget>, BackendError> {
        // Delegate window enumeration to the same logic as UiaBackend (import helper).
        // For MVP: only matches Window/Foreground selectors.
        use super::uia::UiaBackend;
        UiaBackend::new().resolve(sel).await.map(|mut targets| {
            // Re-tag targets as belonging to this backend
            for t in &mut targets {
                t.backend_id = "windows_winmsg";
            }
            targets
        })
    }

    fn capability(&self, t: &ResolvedTarget) -> Tier {
        match &t.inner {
            TargetInner::WindowHandle { .. } => Tier::Background,
            _ => Tier::Unsupported,
        }
    }

    async fn invoke(&self, t: &ResolvedTarget, op: &UiOp) -> Result<(), BackendError> {
        let hwnd_val = match &t.inner {
            TargetInner::WindowHandle { hwnd } => *hwnd,
            _ => return Err(BackendError::NotSupported("not a window handle".into())),
        };
        let op = op.clone();
        tokio::task::spawn_blocking(move || winmsg_invoke_sync(hwnd_val, &op))
            .await
            .map_err(|e| BackendError::Internal(format!("spawn_blocking: {e}")))?
    }

    async fn enumerate(&self, _t: &ResolvedTarget) -> Result<Vec<UiNode>, BackendError> {
        // PostMessage backend has no element enumeration capability.
        Ok(vec![])
    }
}

fn winmsg_invoke_sync(hwnd_val: isize, op: &UiOp) -> Result<(), BackendError> {
    unsafe {
        match op {
            UiOp::Click { .. } => {
                // Post click at (0,0) relative to client area
                let pos = LPARAM(0);
                PostMessageW(HWND(hwnd_val as _), WM_LBUTTONDOWN, WPARAM(1), pos)
                    .map_err(|e| BackendError::Internal(format!("WM_LBUTTONDOWN: {e}")))?;
                PostMessageW(HWND(hwnd_val as _), WM_LBUTTONUP, WPARAM(0), pos)
                    .map_err(|e| BackendError::Internal(format!("WM_LBUTTONUP: {e}")))?;
                Ok(())
            }
            UiOp::Focus { .. } => {
                // WM_SETFOCUS posts a focus notification to the window
                PostMessageW(
                    HWND(hwnd_val as _),
                    windows::Win32::UI::WindowsAndMessaging::WM_SETFOCUS,
                    WPARAM(0),
                    LPARAM(0),
                )
                .map_err(|e| BackendError::Internal(format!("WM_SETFOCUS: {e}")))?;
                Ok(())
            }
            _ => Err(BackendError::NotSupported(
                "win-msg only supports Click and Focus".into(),
            )),
        }
    }
}
