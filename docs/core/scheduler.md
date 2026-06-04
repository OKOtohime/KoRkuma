# `scheduler` — 工作流调度器

> **Crate**: `korkuma-core` · **文件**: `crates/core/src/scheduler.rs`
> **最后同步**: 2026-06-03 (M2.2 新增)

## 职责

`scheduler` 是 M2.2 引入的**调度层**，位于同步 `EventRouter` 与 Tokio 运行时之间（见 DESIGN.md §14.3）。Router 保持单线程无锁，调度器承担所有并发协调：

- **并发策略执行**：按每个宏的 `ConcurrencyPolicy` 决策是否启动、入队、丢弃或重启工作流
- **优先级排序**：`dispatch_scheduled` 已按 `Macro.priority` 降序处理；调度器按序接收任务
- **共享资源仲裁**：通过 `ResourcePool` 为声明了相同资源的并发动作提供异步互斥锁，防止合成输入交错
- **取消传播**：`CancellationToken` 贯穿各策略（RestartIfRunning 取消旧实例、Debounce 取消计时器）

## 公开 API

### 常量

| 常量 | 值 | 说明 |
|------|----|------|
| `RESOURCE_INPUT` | `"input"` | 键盘/鼠标注入动作的资源 ID |
| `RESOURCE_CLIPBOARD` | `"clipboard"` | 剪贴板读写动作的资源 ID |

窗口作用域资源使用 `"window:<id>"` 命名约定。

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `WorkflowScheduler` | struct（`Arc` 包装） | 核心调度器，`Clone`、`Send`、`Sync` |

### `WorkflowScheduler` 方法

| 签名 | 说明 |
|------|------|
| `fn new(handle: Handle, evt_tx: Sender<EngineEvent>) -> Self` | 创建调度器，绑定到指定 Tokio 运行时 |
| `fn resource_pool(&self) -> &ResourcePool` | 返回共享资源锁池（构建 ExecContext 时传入） |
| `fn schedule(macro_id, policy, ctx, workflow, reg)` | 根据策略调度工作流，立即返回（fire-and-forget） |

### 函数（测试辅助）

| 签名 | 说明 |
|------|------|
| `fn test_exec_ctx(store, pool) -> ExecContext` | 构建最小化 ExecContext，供调度器单元测试使用 |

## 并发策略语义

`WorkflowScheduler::schedule` 根据 `ConcurrencyPolicy`（见 [domain.md](domain.md)）决定执行路径：

| 策略 | 当已有实例运行时 | 当无实例运行时 |
|------|----------------|---------------|
| `Parallel` | 新实例直接 spawn | spawn |
| `DropIfRunning` | 静默丢弃 | spawn |
| `Queue { max }` | 若队列未满则入队；满则丢弃 | spawn |
| `RestartIfRunning` | 取消全部旧 token，spawn 新实例 | spawn |
| `Debounce { ms }` | 取消旧计时器，重新等 ms | 等 ms 后 spawn |
| `Throttle { ms }` | 若距上次 < ms 则丢弃 | 记录时间戳，spawn |

`Queue` 策略完成后自动从队列头部取下一项继续执行（使用循环，非递归 spawn，避免 Send 证明循环依赖）。

## 资源仲裁机制

`ResourcePool`（定义于 [`context`](context.md)）维护 `HashMap<String, Arc<tokio::sync::Mutex<()>>>`，每个资源 ID 对应一个异步互斥锁。

执行路径：

```
scheduler.schedule(ctx, workflow, ...)
  └─ execute(ctx, workflow, ...)
       └─ run_workflow(&workflow, &mut ctx, &reg)
            └─ run_action(cfg, ctx, reg)
                 ├─ action.resources()          // 获取资源 ID 列表
                 ├─ ctx.resource_pool.get_lock(id).await   // 获取 Arc<Mutex>
                 ├─ lock.lock_owned().await      // 获取 OwnedMutexGuard
                 ├─ action.execute(ctx).await    // 持锁期间执行
                 └─ drop(guards)                // 自动释放
```

所有从同一调度器派发的工作流共享同一个 `ResourcePool`（通过 `scheduler.resource_pool().clone()` 传入 `ExecContext`）。不同宏试图占用同一资源时，第二个会阻塞在 `lock_owned().await`，直到第一个释放。

## 工作流取消

`ExecContext.cancel: CancellationToken` 贯通所有节点。`run_node` 在每个节点入口检查 `is_cancelled()`，命中即返回 `Stop`。以下场景触发取消：

- `RestartIfRunning`：调度器主动调用旧实例的 `token.cancel()`
- `WorkflowNode::Timeout`：超时后 `ctx.cancel.cancel()`
- 未来：UI 停止按钮、关机信号

## 引擎事件反馈

调度器通过构造时传入的 `crossbeam_channel::Sender<EngineEvent>` 将工作流运行时产生的错误事件（`EngineEvent::Error`）发回引擎循环。`MacroFired` 事件在 `dispatch_scheduled` 中同步返回，不经过调度器 channel。

## 依赖关系

依赖以下同 workspace 模块：
- [`context`](context.md) — `ExecContext`、`CancellationToken`、`LogHandle`、`ResourcePool`
- [`domain`](domain.md) — `ConcurrencyPolicy`、`MacroId`、`WorkflowNode`
- [`engine`](engine.md) — `EngineEvent`
- [`registry`](registry.md) — `Registry`
- [`workflow`](workflow.md) — `run_workflow`

外部依赖：`tokio::sync::Mutex`（状态锁）、`tokio::runtime::Handle`（spawn）、`crossbeam_channel`（事件回传）。

## 设计说明

调度器内部使用 `Arc<SchedulerInner>` 包装两个 `tokio::sync::Mutex`：一个用于 `HashMap<MacroId, MacroState>` 的并发状态（每个宏的运行计数、队列、取消 token、计时），一个内嵌于 `ResourcePool` 用于动态创建资源锁。

所有持锁操作在进入 `.await` 点**之前**显式 `drop`，确保不跨 await 持 Tokio Mutex，避免死锁。`Queue` 策略使用循环替代递归 spawn，以满足 `tokio::spawn` 要求的 `Send + 'static` 约束（递归 async fn 会造成循环 Send 证明）。
