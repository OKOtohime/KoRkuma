# `engine_loop` — 引擎线程与双通道事件循环

> **Crate**: `koakuma-core` · **文件**: `crates/core/src/engine_loop.rs`
> **最后同步**: 2026-06-03 (M2.1：引入 Tokio 运行时)

## 职责

`engine_loop` 是整个 Hook → Constraint → Action 管道的运行时调度核心。它在独立线程上持有 `EventRouter`，并同时监听两条 crossbeam 通道：

- **命令通道** (`EngineCommand`)：来自 UI 线程或控制器，用于增删宏、查快照、触发手动执行、关机。
- **事件通道** (`Event`)：来自各 `HookProvider` 线程，每条事件经过 `EventRouter::dispatch` 驱动完整的 H→C→A 管道。

该模块提供的公开接口极小——只有 `start_engine` 一个函数和 `EngineHandle` 一个句柄——其余所有逻辑都封装在引擎线程内部，保证 UI 线程永不阻塞。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `EngineHandle` | struct | 引擎线程句柄，用于发送命令和关机 |

#### `EngineHandle` 方法

| 签名 | 说明 |
|------|------|
| `fn send(&self, cmd: EngineCommand)` | 向引擎线程发送命令（fire-and-forget，不阻塞） |
| `fn clone_sender(&self) -> Sender<EngineCommand>` | 返回可 clone 的命令 Sender，供多个 UI 回调共享 |
| `fn stop(&mut self)` | 发送 `Shutdown` 并 join 引擎线程（幂等） |

`EngineHandle` 实现 `Drop`，析构时自动调用 `stop()`，确保引擎总是被正确关闭。

### 函数

| 签名 | 说明 |
|------|------|
| `fn start_engine<F>(registry, store, on_event) -> (EngineHandle, EventSink)` | 启动引擎线程，返回控制句柄和事件写入端 |

#### `start_engine` 参数

| 参数 | 类型 | 说明 |
|------|------|------|
| `registry` | `Arc<Registry>` | 完全填充的工厂注册表，引擎线程共享只读 |
| `store` | `Arc<dyn StateStore>` | 全局状态存储，引擎线程与 Action 共享读写 |
| `on_event` | `impl Fn(EngineEvent) + Send + 'static` | 引擎事件回调（在引擎线程执行） |

返回的 `EventSink`（`crossbeam_channel::Sender<Event>`）可 `clone()` 后分别传给各 `HookProvider::start()`。

#### 引擎循环逻辑

```
// M2.1: 引擎线程持有一个多线程 Tokio 运行时
let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build();
loop {
    select! {
        recv(cmd_rx) → EngineCommand::AddMacro(m)       → router.add_macro(m)
                     → EngineCommand::UpdateMacro(m)    → remove + add
                     → EngineCommand::DeleteMacro(id)   → router.remove_macro(id)
                     → EngineCommand::SetEnabled(id, b) → router.set_enabled(id, b)
                     → EngineCommand::TriggerManually(id)→ runtime.block_on(router.dispatch_manual_trigger(...))
                     → EngineCommand::QuerySnapshot(tx) → tx.send(router.snapshot())
                     → EngineCommand::Shutdown | Err    → return
        recv(evt_rx) → Event                             → runtime.block_on(router.dispatch(event, ...))
                                                           → on_event(ev) for each EngineEvent
    }
}
```

## 依赖关系

依赖以下同 workspace 模块：
- [`engine`](engine.md) — `EngineCommand`、`EngineEvent`
- [`event`](event.md) — `Event`
- [`registry`](registry.md) — `Registry`
- [`router`](router.md) — `EventRouter`、`dispatch`、`dispatch_manual_trigger`、`snapshot`
- [`state`](state.md) — `StateStore`
- [`traits`](traits.md) — `EventSink`

外部依赖：
- `crossbeam_channel` — `select!` 宏、`unbounded` 通道
- `tokio` — 多线程运行时，`block_on` 驱动异步 `dispatch`（M2.1）

## 设计说明

**`on_event` 在引擎线程调用**：M1.3 中回调通过 `ui_weak.upgrade_in_event_loop` 将日志条目跨线程推送到 Slint UI 的 `VecModel<LogEntry>`，引擎线程本身不接触任何 `!Send` 的 UI 类型。

**`crossbeam::select!` 的公平性**：当两条通道同时有消息时，`select!` 随机选择一条处理，避免命令通道饥饿。这对于"关机时仍有事件积压"的情况是安全的——引擎在处理 `Shutdown` 时立刻返回，不等待清空事件队列（M1.2 可接受的设计）。

**`EngineHandle::Drop` 幂等性**：`stop()` 用 `Option::take` 确保 join 只执行一次；`send(Shutdown)` 在通道已关闭时静默丢弃错误。因此显式调用 `stop()` 后再 drop `EngineHandle` 是安全的。

**M2.1 异步执行**：引擎线程构建一个多线程 Tokio 运行时，对每个 `dispatch` / `dispatch_manual_trigger`（现为 `async`）调用 `runtime.block_on`，驱动宏的异步 [`workflow`](workflow.md)。引擎循环本身仍单线程串行处理命令与事件——`block_on` 把单次宏的工作流跑到完成（单宏内可并发，跨宏串行）。

**V2.2 演进**：M2.2 调度器将以 `runtime.spawn` 替代 `block_on`，引入跨宏并发、优先级排序与共享资源（input/clipboard）仲裁；引擎循环（路由+命令）仍保持单线程。
