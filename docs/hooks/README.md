# `koakuma-hooks` — 平台事件监听

> **Cargo 包名**: `koakuma-hooks` · **路径**: `crates/hooks/`
> **最后同步**: 2026-06-02

## 职责概述

`koakuma-hooks` 提供平台特定的 `HookProvider` 实现，负责监听操作系统事件（快捷键、窗口焦点、进程启停）并将其转换为 `Event` 推送到引擎的 `EventSink`。同时提供跨平台的 `TriggerSpec` 实现，用于在 `EventRouter` 中对各事件类型做细粒度匹配。

## 模块索引

| 模块 | 文件 | 说明 |
|------|------|------|
| [`lib`](lib.md) | `src/lib.rs` | crate 根，re-export 平台 provider，提供 `register_trigger_specs` |
| [`trigger_spec`](trigger_spec.md) | `src/trigger_spec.rs` | 跨平台 TriggerSpec 实现（Hotkey / WindowFocus / Process） |
| [`platform_windows`](platform_windows.md) | `src/platform_windows.rs` | Windows 平台三个 HookProvider（`#[cfg(target_os = "windows")]`） |

## 对外依赖

| Crate | 用途 |
|-------|------|
| `koakuma-core` | `HookProvider` trait、`TriggerSpec` trait、`EventSink`、`Event`、`EventKind`、域类型 |
| `windows 0.58` | Win32 API 绑定（仅 `target_os = "windows"`） |

## Windows 功能特性

| feature | 用途 |
|---|---|
| `Win32_Foundation` | 基础句柄类型 `HWND`、`LPARAM` 等 |
| `Win32_UI_WindowsAndMessaging` | `SetWindowsHookExW`、`GetMessageW`、`PostThreadMessageW` 等 |
| `Win32_UI_Accessibility` | `SetWinEventHook`、`UnhookWinEvent` |
| `Win32_UI_Input_KeyboardAndMouse` | `GetKeyState` |
| `Win32_System_Threading` | `GetCurrentThreadId` |
| `Win32_System_Diagnostics_ToolHelp` | `CreateToolhelp32Snapshot` 进程枚举 |

## 内部依赖关系图

```
lib
 ├── trigger_spec   (cross-platform, no #[cfg])
 └── platform_windows  (#[cfg(target_os = "windows")])
         ↓
   koakuma-core traits
```
