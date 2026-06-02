//! Windows platform HookProvider implementations.
//!
//! Three providers are implemented using Win32 APIs:
//!
//! | Provider | API | Threading |
//! |---|---|---|
//! | [`HotkeyProvider`] | `SetWindowsHookEx(WH_KEYBOARD_LL)` | Dedicated message-pump thread |
//! | [`WindowFocusProvider`] | `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` | Dedicated message-pump thread |
//! | [`ProcessProvider`] | `CreateToolhelp32Snapshot` polling | Dedicated polling thread, 500 ms interval |
//!
//! Both hook-based providers install their hook on a dedicated thread that then
//! runs a `GetMessage` loop. The hook callbacks are invoked on that same thread
//! (`WINEVENT_OUTOFCONTEXT` / low-level hook semantics). `stop()` posts `WM_QUIT`
//! to the hook thread and joins it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use windows::Win32::Foundation::{CloseHandle, HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
    TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, GetWindowTextW, PostThreadMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, EVENT_SYSTEM_FOREGROUND, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
};

use koakuma_core::error::HookError;
use koakuma_core::event::{Event, EventKind};
use koakuma_core::traits::{EventSink, HookProvider};
use koakuma_core::value::Value;

// ── Keyboard hook ─────────────────────────────────────────────────────────────

thread_local! {
    static KB_SINK: RefCell<Option<EventSink>> = RefCell::new(None);
}

unsafe extern "system" fn keyboard_hook_proc(
    ncode: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if ncode == HC_ACTION as i32 {
        let msg = wparam.0 as u32;
        if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
            let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };

            // Collect active modifier keys via GetKeyState.
            // VK_CONTROL=0x11, VK_SHIFT=0x10, VK_MENU=0x12, VK_LWIN=0x5B, VK_RWIN=0x5C
            let mut modifiers = Vec::new();
            unsafe {
                if (GetKeyState(0x11) as u16) & 0x8000 != 0 {
                    modifiers.push(Value::Str("Ctrl".to_string()));
                }
                if (GetKeyState(0x10) as u16) & 0x8000 != 0 {
                    modifiers.push(Value::Str("Shift".to_string()));
                }
                if (GetKeyState(0x12) as u16) & 0x8000 != 0 {
                    modifiers.push(Value::Str("Alt".to_string()));
                }
                if ((GetKeyState(0x5B) as u16) | (GetKeyState(0x5C) as u16)) & 0x8000 != 0 {
                    modifiers.push(Value::Str("Win".to_string()));
                }
            }

            let payload = Value::Map(
                [
                    ("key".to_string(), Value::Str(vk_to_name(info.vkCode))),
                    ("modifiers".to_string(), Value::List(modifiers)),
                    ("vk_code".to_string(), Value::Int(info.vkCode as i64)),
                ]
                .into_iter()
                .collect(),
            );

            KB_SINK.with(|cell| {
                if let Some(sink) = cell.borrow().as_ref() {
                    let _ = sink.send(Event {
                        kind: EventKind::Hotkey,
                        source: "hotkey_win32".to_string(),
                        timestamp: SystemTime::now(),
                        payload,
                    });
                }
            });
        }
    }
    // Always pass the event to the next hook in the chain.
    unsafe { CallNextHookEx(HHOOK::default(), ncode, wparam, lparam) }
}

/// Windows system-wide keyboard hook provider using `WH_KEYBOARD_LL`.
///
/// Runs a dedicated message-pump thread. Every key-down event (including
/// system-key combinations involving Alt) fires an [`EventKind::Hotkey`] event.
///
/// **Payload schema:**
///
/// | Field | Type | Description |
/// |---|---|---|
/// | `key` | `Str` | Key name (`"A"`, `"F5"`, `"Enter"`, …) |
/// | `modifiers` | `List<Str>` | Active modifiers: any subset of `["Ctrl","Shift","Alt","Win"]` |
/// | `vk_code` | `Int` | Raw Windows Virtual Key code |
pub struct HotkeyProvider {
    thread_id: Arc<AtomicU32>,
    thread: Option<JoinHandle<()>>,
}

impl HotkeyProvider {
    /// Creates a new, stopped provider.
    pub fn new() -> Self {
        Self {
            thread_id: Arc::new(AtomicU32::new(0)),
            thread: None,
        }
    }
}

impl Default for HotkeyProvider {
    fn default() -> Self { Self::new() }
}

impl HookProvider for HotkeyProvider {
    fn id(&self) -> &'static str { "hotkey_win32" }

    fn produces(&self) -> &'static [EventKind] { &[EventKind::Hotkey] }

    fn start(&mut self, sink: EventSink) -> Result<(), HookError> {
        if self.thread.is_some() {
            return Err(HookError::AlreadyRunning);
        }
        let thread_id = Arc::clone(&self.thread_id);
        let handle = thread::Builder::new()
            .name("koakuma-hotkey".to_string())
            .spawn(move || {
                KB_SINK.with(|c| *c.borrow_mut() = Some(sink));
                thread_id.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);

                let hook = match unsafe {
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0)
                } {
                    Ok(h) => h,
                    Err(_) => {
                        thread_id.store(0, Ordering::SeqCst);
                        return;
                    }
                };

                // Message loop — keeps the thread alive so the hook can fire.
                let mut msg = MSG::default();
                loop {
                    // GetMessageW returns 0 for WM_QUIT, -1 on error.
                    let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
                    if r.0 <= 0 { break; }
                }

                unsafe { let _ = UnhookWindowsHookEx(hook); }
                KB_SINK.with(|c| *c.borrow_mut() = None);
                thread_id.store(0, Ordering::SeqCst);
            })
            .map_err(|e| HookError::InitFailed(e.to_string()))?;

        self.thread = Some(handle);
        Ok(())
    }

    fn stop(&mut self) {
        let tid = self.thread_id.load(Ordering::SeqCst);
        if tid != 0 {
            // Post WM_QUIT to break the hook thread's GetMessage loop.
            unsafe { let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0)); }
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ── Window focus hook ─────────────────────────────────────────────────────────

thread_local! {
    static WF_SINK: RefCell<Option<EventSink>> = RefCell::new(None);
}

unsafe extern "system" fn winevent_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    // Read the window title into a fixed-size buffer (512 UTF-16 chars).
    let mut buf = [0u16; 512];
    let written = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if written <= 0 {
        return;
    }
    let title = String::from_utf16_lossy(&buf[..written as usize]);

    let payload = Value::Map(
        [
            ("title".to_string(), Value::Str(title)),
            // exe field: full path available via QueryFullProcessImageNameW
            // in a future iteration; empty string for M1.2.
            ("exe".to_string(), Value::Str(String::new())),
        ]
        .into_iter()
        .collect(),
    );

    WF_SINK.with(|cell| {
        if let Some(sink) = cell.borrow().as_ref() {
            let _ = sink.send(Event {
                kind: EventKind::WindowFocus,
                source: "window_focus_win32".to_string(),
                timestamp: SystemTime::now(),
                payload,
            });
        }
    });
}

/// Windows foreground-window change provider using `SetWinEventHook`.
///
/// Fires an [`EventKind::WindowFocus`] event every time the user switches to
/// a different top-level window.
///
/// **Payload schema:**
///
/// | Field | Type | Description |
/// |---|---|---|
/// | `title` | `Str` | Window title text |
/// | `exe` | `Str` | Executable filename (empty in M1.2; populated in M1.3) |
pub struct WindowFocusProvider {
    thread_id: Arc<AtomicU32>,
    thread: Option<JoinHandle<()>>,
}

impl WindowFocusProvider {
    /// Creates a new, stopped provider.
    pub fn new() -> Self {
        Self {
            thread_id: Arc::new(AtomicU32::new(0)),
            thread: None,
        }
    }
}

impl Default for WindowFocusProvider {
    fn default() -> Self { Self::new() }
}

impl HookProvider for WindowFocusProvider {
    fn id(&self) -> &'static str { "window_focus_win32" }

    fn produces(&self) -> &'static [EventKind] { &[EventKind::WindowFocus] }

    fn start(&mut self, sink: EventSink) -> Result<(), HookError> {
        if self.thread.is_some() {
            return Err(HookError::AlreadyRunning);
        }
        let thread_id = Arc::clone(&self.thread_id);
        let handle = thread::Builder::new()
            .name("koakuma-window-focus".to_string())
            .spawn(move || {
                WF_SINK.with(|c| *c.borrow_mut() = Some(sink));
                thread_id.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);

                // WINEVENT_OUTOFCONTEXT (0x0000): callback runs on the calling thread,
                // which requires a message loop. idProcess=0, idThread=0 = all processes.
                let hook = unsafe {
                    SetWinEventHook(
                        EVENT_SYSTEM_FOREGROUND,
                        EVENT_SYSTEM_FOREGROUND,
                        HMODULE::default(),
                        Some(winevent_proc),
                        0,
                        0,
                        0x0000, // WINEVENT_OUTOFCONTEXT
                    )
                };
                if hook.0.is_null() {
                    thread_id.store(0, Ordering::SeqCst);
                    return;
                }

                let mut msg = MSG::default();
                loop {
                    let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
                    if r.0 <= 0 { break; }
                }

                unsafe { let _ = UnhookWinEvent(hook); }
                WF_SINK.with(|c| *c.borrow_mut() = None);
                thread_id.store(0, Ordering::SeqCst);
            })
            .map_err(|e| HookError::InitFailed(e.to_string()))?;

        self.thread = Some(handle);
        Ok(())
    }

    fn stop(&mut self) {
        let tid = self.thread_id.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe { let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0)); }
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ── Process monitor ───────────────────────────────────────────────────────────

/// Snapshots all running processes using `CreateToolhelp32Snapshot`.
///
/// Returns a map of `pid → exe_name`.
fn snapshot_processes() -> HashMap<u32, String> {
    let mut map = HashMap::new();
    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return map;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    if unsafe { Process32FirstW(snap, &mut entry) }.is_ok() {
        loop {
            let null_pos = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..null_pos]);
            map.insert(entry.th32ProcessID, name);
            if unsafe { Process32NextW(snap, &mut entry) }.is_err() {
                break;
            }
        }
    }
    unsafe { let _ = CloseHandle(snap); }
    map
}

fn make_process_payload(name: &str, pid: u32, event: &str) -> Value {
    Value::Map(
        [
            ("name".to_string(), Value::Str(name.to_string())),
            ("pid".to_string(), Value::Int(pid as i64)),
            ("event".to_string(), Value::Str(event.to_string())),
        ]
        .into_iter()
        .collect(),
    )
}

/// Windows process start/stop provider using `CreateToolhelp32Snapshot` polling.
///
/// Polls every 500 ms, diffs the process list, and emits [`EventKind::Process`]
/// events for newly started and stopped processes.
///
/// **Payload schema:**
///
/// | Field | Type | Description |
/// |---|---|---|
/// | `name` | `Str` | Executable filename (e.g. `"notepad.exe"`) |
/// | `pid` | `Int` | Process identifier |
/// | `event` | `Str` | `"started"` or `"stopped"` |
pub struct ProcessProvider {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ProcessProvider {
    /// Creates a new, stopped provider.
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            thread: None,
        }
    }
}

impl Default for ProcessProvider {
    fn default() -> Self { Self::new() }
}

impl HookProvider for ProcessProvider {
    fn id(&self) -> &'static str { "process_win32" }

    fn produces(&self) -> &'static [EventKind] { &[EventKind::Process] }

    fn start(&mut self, sink: EventSink) -> Result<(), HookError> {
        if self.running.load(Ordering::SeqCst) {
            return Err(HookError::AlreadyRunning);
        }
        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);

        let handle = thread::Builder::new()
            .name("koakuma-process".to_string())
            .spawn(move || {
                let mut prev = snapshot_processes();
                while running.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(500));
                    if !running.load(Ordering::SeqCst) {
                        break;
                    }
                    let curr = snapshot_processes();

                    // Newly started: present in curr but not prev.
                    for (pid, name) in &curr {
                        if !prev.contains_key(pid) {
                            let _ = sink.send(Event {
                                kind: EventKind::Process,
                                source: "process_win32".to_string(),
                                timestamp: SystemTime::now(),
                                payload: make_process_payload(name, *pid, "started"),
                            });
                        }
                    }

                    // Stopped: present in prev but not curr.
                    for (pid, name) in &prev {
                        if !curr.contains_key(pid) {
                            let _ = sink.send(Event {
                                kind: EventKind::Process,
                                source: "process_win32".to_string(),
                                timestamp: SystemTime::now(),
                                payload: make_process_payload(name, *pid, "stopped"),
                            });
                        }
                    }

                    prev = curr;
                }
            })
            .map_err(|e| HookError::InitFailed(e.to_string()))?;

        self.thread = Some(handle);
        Ok(())
    }

    fn stop(&mut self) {
        // Signal the polling loop to exit on the next iteration.
        self.running.store(false, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ── Virtual key → name ────────────────────────────────────────────────────────

/// Converts a Windows Virtual Key code into a human-readable name.
fn vk_to_name(vk: u32) -> String {
    // A–Z  (0x41–0x5A)
    if matches!(vk, 0x41..=0x5A) {
        return char::from(vk as u8).to_string();
    }
    // 0–9  (0x30–0x39)
    if matches!(vk, 0x30..=0x39) {
        return char::from(vk as u8).to_string();
    }
    // F1–F12  (0x70–0x7B)
    if matches!(vk, 0x70..=0x7B) {
        return format!("F{}", vk - 0x6F);
    }
    match vk {
        0x08 => "Backspace",
        0x09 => "Tab",
        0x0D => "Enter",
        0x1B => "Escape",
        0x20 => "Space",
        0x21 => "PageUp",
        0x22 => "PageDown",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "Left",
        0x26 => "Up",
        0x27 => "Right",
        0x28 => "Down",
        0x2C => "PrintScreen",
        0x2D => "Insert",
        0x2E => "Delete",
        0x5B | 0x5C => "Win",
        0xA0 | 0xA1 => "Shift",
        0xA2 | 0xA3 => "Ctrl",
        0xA4 | 0xA5 => "Alt",
        0xBB => "=",
        0xBD => "-",
        0xBE => ".",
        0xBF => "/",
        0xC0 => "`",
        0xDB => "[",
        0xDC => "\\",
        0xDD => "]",
        0xDE => "'",
        _ => return format!("VK_{:02X}", vk),
    }
    .to_string()
}
