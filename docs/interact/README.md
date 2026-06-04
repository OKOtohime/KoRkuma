# `korkuma-interact` — 后台交互与目标抽象

> **路径**: `crates/interact` · **最后同步**: 2026-06-04 (M2.3 初始实现)

实现 DESIGN.md §13 所定义的后台交互子系统：在不抢占用户焦点的前提下操作目标窗口或浏览器标签，并在无法后台时按策略降级或明确报错。

## 架构位置

```
WorkflowEngine → InteractAction → BackendRegistry
                                     ├── UiaBackend      (Windows, Background)
                                     ├── WinMsgBackend   (Windows, Background)
                                     ├── SendInputBackend (Windows, ForegroundSynthetic)
                                     └── CdpBackend      (跨平台, Background)
```

## 模块索引

| 模块 | 文件 | 说明 |
|------|------|------|
| [`backend`](backend.md) | `src/backend.rs` | `Tier`、`ResolvedTarget`、`UiNode`、`InteractionBackend` trait |
| [`error`](error.md) | `src/error.rs` | `BackendError`、`DispatchError` |
| [`negotiator`](negotiator.md) | `src/negotiator.rs` | `BackendRegistry` 能力协商 + `OnNoBackground` 策略 |
| [`action`](action.md) | `src/action.rs` | `InteractAction` (impl `Action`) + `register_actions` |
| [`backends/cdp`](backends/cdp.md) | `src/backends/cdp.rs` | Chrome DevTools Protocol 浏览器后端 |
| `backends/windows/uia` | `src/backends/windows/uia.rs` | Windows UI Automation 后端（`#[cfg(windows)]`） |
| `backends/windows/win_msg` | `src/backends/windows/win_msg.rs` | Windows PostMessage 后端 |
| `backends/windows/send_input` | `src/backends/windows/send_input.rs` | SendInput 前台降级后端 |
| `backends/stub` | `src/backends/stub.rs` | 测试用 Stub 后端 |
