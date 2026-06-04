# `engine` — 引擎命令与事件协议

> **Crate**: `koakuma-core` · **文件**: `crates/core/src/engine.rs`
> **最后同步**: 2026-06-02

## 职责

`engine` 模块定义引擎线程与 UI 线程之间的通信协议——命令类型 `EngineCommand`（UI → 引擎）和事件类型 `EngineEvent`（引擎 → UI）。这两组类型是引擎的公开接口边界，使 UI 层（Slint）无需直接操作 `EventRouter` 或 `StateStore`。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `LogLevel` | enum | 日志级别，用于 `EngineEvent::ActionLog` |
| `EngineCommand` | enum | UI 线程发往引擎线程的命令 |
| `EngineEvent` | enum | 引擎线程发往 UI 线程的事件 |
| `EngineSnapshot` | struct | `QuerySnapshot` 命令的响应载体 |

#### `LogLevel` 变体

`Debug` / `Info` / `Warn` / `Error`

#### `EngineCommand` 变体

| 变体 | 说明 |
|------|------|
| `AddMacro(Macro)` | 添加或覆盖一条宏 |
| `UpdateMacro(Macro)` | 更新已有宏（保留 ID） |
| `DeleteMacro(MacroId)` | 删除宏 |
| `SetEnabled(MacroId, bool)` | 切换宏的启用状态 |
| `TriggerManually(MacroId)` | 手动触发一条宏 |
| `QuerySnapshot(Sender<EngineSnapshot>)` | 请求引擎当前状态快照，结果通过附带的 channel 返回 |
| `Shutdown` | 优雅关闭引擎线程 |

#### `EngineEvent` 变体

| 变体 | 说明 |
|------|------|
| `MacroFired { id, at }` | 宏成功触发，附带触发时刻 |
| `ActionLog { macro_id, action, level, message }` | 动作执行日志条目 |
| `VariableChanged { key, value }` | 状态变量被写入/更新 |
| `Error { macro_id, message }` | 执行过程中的错误（`macro_id` 为 `None` 时为引擎级错误） |

#### `EngineSnapshot` 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `macros` | `Vec<Macro>` | 当前注册的所有宏的列表 |

## 依赖关系

依赖以下同 workspace 模块：
- [`domain`](domain.md) — `Macro`、`MacroId`
- [`value`](value.md) — `Value`（`VariableChanged` 的值类型）

## 设计说明

`QuerySnapshot` 使用 `crossbeam_channel::Sender<EngineSnapshot>` 作为一次性响应通道，实现请求-响应模式而无需锁。UI 线程发送命令后在 receiver 端阻塞（或异步等待），引擎处理完毕后发送响应。

`EngineEvent` 由 `EventRouter::dispatch` 返回，M1.3 接入引擎主循环后将通过 `slint::invoke_from_event_loop` 推送到 UI 线程，更新界面状态。