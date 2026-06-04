# `korkuma-actions` — 内置 Action 实现

> **Cargo 包名**: `korkuma-actions` · **路径**: `crates/actions/`
> **最后同步**: 2026-06-03 (M2.1：Action 异步化)

## 职责概述

`korkuma-actions` 提供管道末端的执行能力（Action 层）。与 `korkuma-core/builtins` 中轻量级的 `SetVariable`、`Delay` 互补，本 crate 实现较重量级或平台相关的 Action：进程执行、桌面通知、输入模拟。

**M2.1**：三个 Action 均改为 `#[async_trait]` 的 `async fn execute`，签名外其逻辑不变（`RunCommand`/`Notify`/`SimulateInput` 内部仍为同步调用，无内部 await 点，future 为 `Send`）。新增 `async-trait` 依赖。

## 模块索引

| 模块 | 文件 | 说明 |
|------|------|------|
| [`lib`](lib.md) | `src/lib.rs` | crate 根，`register_all` 入口 |
| [`run_command`](run_command.md) | `src/run_command.rs` | 外部进程执行（跨平台） |
| [`notify`](notify.md) | `src/notify.md` | 桌面通知（跨平台，via notify-rust） |
| [`simulate_input`](simulate_input.md) | `src/simulate_input.rs` | 键鼠输入模拟（Windows；其他平台返回错误） |

## 对外依赖

| Crate | 用途 |
|-------|------|
| `korkuma-core` | `Action` trait、`ActionConfig`、`ExecContext`、权限类型 |
| `async-trait` | `async fn execute` 的 dyn 兼容（M2.1） |
| `notify-rust 4` | 跨平台桌面通知（Linux: zbus/D-Bus；Windows: WinRT；macOS: native） |
| `windows 0.58`（仅 Windows） | `SendInput`、`GetSystemMetrics` 等 Win32 API（`simulate_input` 使用） |

## 内部依赖关系图

```
lib
 ├── run_command   (cross-platform, std::process)
 ├── notify        (cross-platform, notify-rust)
 └── simulate_input
       ├── platform_impl  (#[cfg(windows)]  — SendInput)
       └── platform_impl  (#[cfg(not(windows))] — stub)
```

## 已实现 / 计划

| Action | 状态 | 说明 |
|--------|------|------|
| `RunCommand` | ✅ M1.2 | `std::process::Command`，可选捕获 stdout/stderr |
| `Notify` | ✅ M1.2 | `notify-rust`，跨平台 |
| `SimulateInput` | ✅ M1.2 | Win32 `SendInput`，Linux 返回错误 |
| `SetVariable` | ✅ M1.1 | 在 `korkuma-core/builtins` |
| `Delay` | ✅ M1.1 | 在 `korkuma-core/builtins` |
| `RunScript` | M1.4 | Rhai 沙箱 |
| `HttpRequest` | M2.2 | Tokio + reqwest |
