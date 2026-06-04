use async_trait::async_trait;
use koakuma_core::domain::{TargetSelector, UiOp};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
    IUIAutomationValuePattern, TreeScope_Descendants, UIA_AutomationIdPropertyId,
    UIA_InvokePatternId, UIA_NamePropertyId, UIA_ValuePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowTextW, IsWindowVisible,
};
use windows::core::{BSTR, VARIANT};

use crate::backend::{InteractionBackend, ResolvedTarget, TargetInner, Tier, UiNode};
use crate::error::BackendError;

/// Windows UI Automation backend.
///
/// Uses `IUIAutomation` (COM) to interact with windows without stealing focus
/// (`Tier::Background`).  Requires the window's application to be accessible
/// via UIA — native Win32, WPF, WinForms and most UWP apps qualify; fully
/// custom-rendered apps may not.
pub struct UiaBackend;

impl UiaBackend {
    pub fn new() -> Self {
        Self
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn window_title(hwnd: HWND) -> String {
    let mut buf = vec![0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

struct EnumData {
    pattern: String,
    results: Vec<isize>,
}

// SAFETY: EnumData outlives the EnumWindows call in `enumerate_windows_by_title`.
unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let data = &mut *(lparam.0 as *mut EnumData);
    if unsafe { IsWindowVisible(hwnd) }.as_bool() {
        let title = window_title(hwnd);
        if title.contains(&data.pattern) {
            data.results.push(hwnd.0 as isize);
        }
    }
    BOOL(1)
}

fn enumerate_windows_by_title(pattern: &str) -> Vec<isize> {
    let mut data = EnumData { pattern: pattern.to_string(), results: Vec::new() };
    // SAFETY: data is alive for the duration of EnumWindows
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&raw mut data as isize));
    }
    data.results
}

fn make_target(hwnd: isize) -> ResolvedTarget {
    let title = window_title(HWND(hwnd as _));
    ResolvedTarget {
        backend_id: "windows_uia",
        display_name: if title.is_empty() { format!("HWND(0x{hwnd:x})") } else { title },
        inner: TargetInner::WindowHandle { hwnd },
    }
}

// ── UIA element finding ───────────────────────────────────────────────────────

unsafe fn element_from_hwnd(
    uia: &IUIAutomation,
    hwnd: isize,
) -> Result<IUIAutomationElement, BackendError> {
    uia.ElementFromHandle(HWND(hwnd as _))
        .map_err(|e| BackendError::NotFound(format!("UIA element from HWND: {e}")))
}

/// Find a descendant by UiPath.
/// - `"name:X"` → search by accessible name
/// - `"id:X"` → search by AutomationId
/// - empty → return root element
unsafe fn find_element(
    uia: &IUIAutomation,
    root: &IUIAutomationElement,
    path: &str,
) -> Result<IUIAutomationElement, BackendError> {
    if path.is_empty() {
        return Ok(root.clone());
    }
    let (prop_id, value) = if let Some(v) = path.strip_prefix("name:") {
        (UIA_NamePropertyId, v)
    } else if let Some(v) = path.strip_prefix("id:") {
        (UIA_AutomationIdPropertyId, v)
    } else {
        (UIA_NamePropertyId, path)
    };

    let variant = VARIANT::from(BSTR::from(value));

    let cond = uia
        .CreatePropertyCondition(prop_id, &variant)
        .map_err(|e| BackendError::Internal(format!("CreatePropertyCondition: {e}")))?;

    root.FindFirst(TreeScope_Descendants, &cond)
        .map_err(|_| BackendError::NotFound(format!("element not found: {path}")))
}

// ── blocking operations ───────────────────────────────────────────────────────

fn uia_invoke_sync(hwnd: isize, op: UiOp) -> Result<(), BackendError> {
    unsafe {
        // SAFETY: COM per-thread init; no-op if already initialised
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let uia: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| BackendError::NotAvailable(format!("UIA init: {e}")))?;

        let root = element_from_hwnd(&uia, hwnd)?;

        match &op {
            UiOp::Focus { node } => {
                let elem = match node {
                    Some(p) => find_element(&uia, &root, p)?,
                    None => root,
                };
                elem.SetFocus()
                    .map_err(|e| BackendError::Internal(format!("SetFocus: {e}")))?;
            }
            UiOp::Click { node } => {
                let elem = find_element(&uia, &root, node)?;
                let pat: IUIAutomationInvokePattern = elem
                    .GetCurrentPatternAs(UIA_InvokePatternId)
                    .map_err(|e| BackendError::NotSupported(format!("InvokePattern: {e}")))?;
                pat.Invoke()
                    .map_err(|e| BackendError::Internal(format!("Invoke: {e}")))?;
            }
            UiOp::SetText { node, text } => {
                let elem = find_element(&uia, &root, node)?;
                let pat: IUIAutomationValuePattern = elem
                    .GetCurrentPatternAs(UIA_ValuePatternId)
                    .map_err(|e| BackendError::NotSupported(format!("ValuePattern: {e}")))?;
                pat.SetValue(&BSTR::from(text.as_str()))
                    .map_err(|e| BackendError::Internal(format!("SetValue: {e}")))?;
            }
            UiOp::ReadValue { node } => {
                let elem = find_element(&uia, &root, node)?;
                let pat: IUIAutomationValuePattern = elem
                    .GetCurrentPatternAs(UIA_ValuePatternId)
                    .map_err(|e| BackendError::NotSupported(format!("ValuePattern: {e}")))?;
                let _val = pat
                    .CurrentValue()
                    .map_err(|e| BackendError::Internal(format!("GetValue: {e}")))?;
            }
            UiOp::SendKeys { .. } => {
                return Err(BackendError::NotSupported(
                    "SendKeys requires ForegroundSynthetic tier".into(),
                ));
            }
        }
        Ok(())
    }
}

fn uia_enumerate_sync(hwnd: isize) -> Result<Vec<UiNode>, BackendError> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let uia: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| BackendError::NotAvailable(format!("UIA init: {e}")))?;

        let root = element_from_hwnd(&uia, hwnd)?;
        let true_cond = uia
            .CreateTrueCondition()
            .map_err(|e| BackendError::Internal(format!("TrueCondition: {e}")))?;
        let arr = root
            .FindAll(TreeScope_Descendants, &true_cond)
            .map_err(|e| BackendError::Internal(format!("FindAll: {e}")))?;
        let count =
            arr.Length().map_err(|e| BackendError::Internal(format!("array len: {e}")))?;

        let mut nodes = Vec::new();
        for i in 0..count {
            if let Ok(elem) = arr.GetElement(i) {
                let name = elem.CurrentName().map(|b| b.to_string()).unwrap_or_default();
                let ctrl = elem
                    .CurrentLocalizedControlType()
                    .map(|b| b.to_string())
                    .unwrap_or_default();
                let aid = elem
                    .CurrentAutomationId()
                    .map(|b| b.to_string())
                    .unwrap_or_default();

                let path = if !aid.is_empty() {
                    format!("id:{aid}")
                } else if !name.is_empty() {
                    format!("name:{name}")
                } else {
                    continue;
                };

                nodes.push(UiNode { path, name, control_type: ctrl });
            }
        }
        Ok(nodes)
    }
}

// ── trait impl ────────────────────────────────────────────────────────────────

#[async_trait]
impl InteractionBackend for UiaBackend {
    fn id(&self) -> &'static str {
        "windows_uia"
    }

    async fn resolve(&self, sel: &TargetSelector) -> Result<Vec<ResolvedTarget>, BackendError> {
        let sel = sel.clone();
        tokio::task::spawn_blocking(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
            match &sel {
                TargetSelector::Foreground => {
                    let hwnd = unsafe { GetForegroundWindow() };
                    if hwnd.0.is_null() {
                        return Ok(vec![]);
                    }
                    Ok(vec![make_target(hwnd.0 as isize)])
                }
                TargetSelector::Window { title_pattern, .. } => {
                    Ok(enumerate_windows_by_title(title_pattern)
                        .into_iter()
                        .map(make_target)
                        .collect())
                }
                TargetSelector::Process { name } => {
                    // MVP: find windows whose title contains the process name
                    Ok(enumerate_windows_by_title(name)
                        .into_iter()
                        .map(make_target)
                        .collect())
                }
                _ => Ok(vec![]),
            }
        })
        .await
        .map_err(|e| BackendError::Internal(format!("spawn_blocking: {e}")))?
    }

    fn capability(&self, t: &ResolvedTarget) -> Tier {
        // Optimistically report Background for any valid HWND; invoke() will fail
        // gracefully if UIA isn't supported by that window.
        match &t.inner {
            TargetInner::WindowHandle { .. } => Tier::Background,
            _ => Tier::Unsupported,
        }
    }

    async fn invoke(&self, t: &ResolvedTarget, op: &UiOp) -> Result<(), BackendError> {
        let hwnd = match &t.inner {
            TargetInner::WindowHandle { hwnd } => *hwnd,
            _ => return Err(BackendError::NotSupported("not a window handle".into())),
        };
        let op = op.clone();
        tokio::task::spawn_blocking(move || uia_invoke_sync(hwnd, op))
            .await
            .map_err(|e| BackendError::Internal(format!("spawn_blocking: {e}")))?
    }

    async fn enumerate(&self, t: &ResolvedTarget) -> Result<Vec<UiNode>, BackendError> {
        let hwnd = match &t.inner {
            TargetInner::WindowHandle { hwnd } => *hwnd,
            _ => return Err(BackendError::NotSupported("not a window handle".into())),
        };
        tokio::task::spawn_blocking(move || uia_enumerate_sync(hwnd))
            .await
            .map_err(|e| BackendError::Internal(format!("spawn_blocking: {e}")))?
    }
}
