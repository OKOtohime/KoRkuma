# `platform_windows` — Windows 平台 HookProvider 实现

> **Crate**: `koakuma-hooks` · **文件**: `crates/hooks/src/platform_windows.rs`
> **最后同步**: 2026-06-02
> **编译条件**: `#[cfg(target_os = "windows")]`

## 职责

实现三个 Windows 平台的 `HookProvider`，覆盖 M1.2 所需的全部事件来源。每个 provider 运行于独立线程，通过 `EventSink` 与引擎解耦。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `HotkeyProvider` | struct | 系统全局低级键盘 hook |
| `WindowFocusProvider` | struct | 前台窗口切换监听 |
| `ProcessProvider` | struct | 进程启停轮询检测 |

三个类型均实现 `HookProvider` trait、`Default`、`Send`。

---

#### `HotkeyProvider`

**Win32 API**: `SetWindowsHookExW(WH_KEYBOARD_LL, ...)`

在独立消息泵线程安装系统级低级键盘 hook。每次 key-down 事件（含 `WM_SYSKEYDOWN`）触发回调，从 `KBDLLHOOKSTRUCT.vkCode` 提取按键名，从 `GetKeyState` 提取修饰键状态，构造 payload 后发往 `EventSink`。

```rust
pub struct HotkeyProvider { /* thread_id, thread */ }
impl HotkeyProvider { pub fn new() -> Self; }
impl HookProvider for HotkeyProvider { ... }
```

**线程模型**: hook 回调必须在安装它的线程上调用，且该线程必须运行消息循环。`start()` 启动独立线程，在其中 `SetWindowsHookExW` + `GetMessage` 循环；`stop()` 向该线程 `PostThreadMessageW(WM_QUIT)`，然后 `join()`。

---

#### `WindowFocusProvider`

**Win32 API**: `SetWinEventHook(EVENT_SYSTEM_FOREGROUND, ..., WINEVENT_OUTOFCONTEXT)`

每当前台窗口切换时触发 WinEvent 回调，从回调的 `hwnd` 参数调用 `GetWindowTextW` 读取窗口标题（最多 512 字符），构造 payload。

```rust
pub struct WindowFocusProvider { /* thread_id, thread */ }
impl WindowFocusProvider { pub fn new() -> Self; }
impl HookProvider for WindowFocusProvider { ... }
```

**`exe` 字段**: M1.2 为空字符串。M1.3 将通过 `GetWindowThreadProcessId` + `QueryFullProcessImageNameW` 填充完整路径。

---

#### `ProcessProvider`

**Win32 API**: `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` + `Process32FirstW` / `Process32NextW`

每 500 ms 拍一次进程快照，与上次快照 diff，对新增 PID 发 `"started"` 事件，对消失 PID 发 `"stopped"` 事件。

```rust
pub struct ProcessProvider { /* running: Arc<AtomicBool>, thread */ }
impl ProcessProvider { pub fn new() -> Self; }
impl HookProvider for ProcessProvider { ... }
```

**停止方式**: 设置 `AtomicBool` 标志为 false，轮询线程在下一次迭代检测到标志后自然退出，无需 `WM_QUIT`。

### 私有辅助

| 函数 | 说明 |
|------|------|
| `fn vk_to_name(vk: u32) -> String` | 将 VK 码转为可读键名（A-Z、0-9、F1-F12 及常用特殊键） |
| `fn snapshot_processes() -> HashMap<u32, String>` | 一次性 toolhelp 快照，返回 pid→exe\_name |
| `fn make_process_payload(name, pid, event) -> Value` | 构造进程事件 payload |

## 依赖关系

依赖以下同 workspace 模块：
- `koakuma_core::error` — `HookError`
- `koakuma_core::event` — `Event`、`EventKind`
- `koakuma_core::traits` — `EventSink`、`HookProvider`
- `koakuma_core::value` — `Value`

外部依赖（仅 Windows 构建）：
- `windows::Win32::UI::WindowsAndMessaging` — hook 安装、消息循环
- `windows::Win32::UI::Accessibility` — WinEvent hook
- `windows::Win32::UI::Input::KeyboardAndMouse` — `GetKeyState`
- `windows::Win32::System::Threading` — `GetCurrentThreadId`
- `windows::Win32::System::Diagnostics::ToolHelp` — 进程快照

## 设计说明

**线程本地 sink**: hook proc 是 `extern "system"` 函数，不能捕获 Rust 闭包环境。`KB_SINK` 和 `WF_SINK` 使用 `thread_local! { RefCell<Option<EventSink>> }` 在 hook 线程上存储 sink；由于 hook proc 总在安装它的线程上调用，TLS 访问天然安全，无需加锁。

**线程 ID 的传递**: hook 和 window focus provider 通过 `Arc<AtomicU32>` 在 `start()` 侧（写 0 → 非零 → 0）与 `stop()` 侧（读 → PostThreadMessageW）之间传递线程 ID，避免 static 全局污染多实例场景。

**WH_KEYBOARD_LL 的系统要求**: 低级键盘 hook 是系统全局 hook，如果安装它的线程不及时处理消息，系统会自动将其卸载。`keyboard_hook_proc` 只做 `channel.send()`（非阻塞），因此在 crossbeam 无界通道下不会出现超时。

**进程检测延迟**: 轮询间隔 500 ms 意味着最坏情况下进程启停事件有约 500 ms 延迟。对于宏触发场景（用户手动启动应用、观察其状态）已足够；若需更低延迟，可在 M2.x 改为 ETW 事件驱动方案。
