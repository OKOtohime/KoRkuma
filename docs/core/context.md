# `context` — 运行时执行上下文

> **Crate**: `koakuma-core` · **文件**: `crates/core/src/context.rs`
> **最后同步**: 2026-06-04 (M2.4：`ExecContext` 新增 `dry_run: bool` 字段)

## 职责

`context` 模块定义 trait 实现的两个核心入参：`EvalContext`（只读，传入 `Constraint::evaluate`）和 `ExecContext`（可变，传入 `Action::execute`）。它们将一次宏触发所需的所有运行时数据打包成结构体，使 trait 方法签名保持稳定，同时允许未来扩展字段而不破坏已有实现。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `LocalVars` | type alias | `BTreeMap<String, Value>`，单次执行的局部变量 |
| `CancellationToken` | struct | 可跨线程共享的取消信号（基于 `AtomicBool`） |
| `LogHandle` | struct | Action 向引擎发送结构化日志的句柄（M1.1 为 no-op） |
| `ResourcePool` | struct | 共享资源异步锁池，防止并发宏交错访问同一设备（M2.2） |
| `EvalContext<'a>` | struct | 约束求值的只读上下文 |
| `ExecContext` | struct | 动作执行的可变上下文 |

#### `ResourcePool` 方法（M2.2）

| 签名 | 说明 |
|------|------|
| `fn new() -> Self` | 创建空锁池 |
| `async fn get_lock(&self, id: &str) -> Arc<tokio::sync::Mutex<()>>` | 返回（或创建）指定资源 ID 的异步互斥锁 |

`ResourcePool` 内部是 `Arc<TokioMutex<HashMap<String, Arc<TokioMutex<()>>>>>`.
`Clone` 产生同一池的新句柄——所有共享同一调度器的工作流拥有同一个池，跨宏资源锁因此有效。

#### `CancellationToken` 方法

| 签名 | 说明 |
|------|------|
| `fn new() -> Self` | 创建初始未取消的 token |
| `fn cancel(&self)` | 标记为已取消（`SeqCst` 写） |
| `fn is_cancelled(&self) -> bool` | 检查是否已取消（`SeqCst` 读） |

`CancellationToken` 内部是 `Arc<AtomicBool>`，`clone` 产生同一信号的新句柄，任一持有者调用 `cancel` 对所有持有者可见。

#### `EvalContext<'a>` 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `event` | `&'a Event` | 触发该次求值的原始事件 |
| `macro_id` | `MacroId` | 当前宏的 ID |
| `store` | `&'a dyn StateStore` | 全局状态存储（只读访问） |

#### `ExecContext` 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `event` | `Event` | 触发事件（拥有所有权） |
| `macro_id` | `MacroId` | 当前宏的 ID |
| `locals` | `LocalVars` | 局部变量，`VarScope::Local` 写入此处 |
| `store` | `Arc<dyn StateStore>` | 全局状态存储（可写） |
| `permissions` | `PermissionGrant` | 运行时授权集合 |
| `cancel` | `CancellationToken` | 取消信号 |
| `log` | `LogHandle` | 日志句柄 |
| `resource_pool` | `ResourcePool` | 共享资源锁池，来自 `WorkflowScheduler::resource_pool()`（M2.2） |
| `dry_run` | `bool` | 若为 `true`，动作只记录日志而不实际执行；由 `EngineCommand::DryRunMacro` 设置（M2.4） |

#### `ExecContext` 方法（M2.1）

| 签名 | 说明 |
|------|------|
| `fn fork(&self) -> ExecContext` | 为并发工作流分支创建独立上下文 |

`fork` 共享全局 `store`（`Arc`）、`cancel`、`permissions`、`log`、`resource_pool`，但复制 `locals`。分支内对局部变量的写入**不回流**父上下文——分支间仅通过共享 `StateStore` 通信。共享 `resource_pool` 使并行分支内的资源锁也有效。由 [`WorkflowNode::Parallel`](domain.md) 使用。

## 依赖关系

依赖以下同 workspace 模块：
- [`event`](event.md) — `Event`
- [`domain`](domain.md) — `MacroId`
- [`permission`](permission.md) — `PermissionGrant`
- [`state`](state.md) — `StateStore`
- [`value`](value.md) — `Value`（通过 `LocalVars`）

## 设计说明

`LogHandle::log` 在 M1.1 是空操作。M1.2 将其接入 `crossbeam_channel::Sender<EngineEvent>`，Action 调用后引擎转发到 UI 线程的变更日志面板。

`EvalContext` 使用生命周期 `'a` 并借用 `store` 和 `event`，避免克隆开销；`ExecContext` 拥有 `event` 并通过 `Arc` 共享 `store`，以支持并行动作执行。M2.1 的 `fork` 即依赖此设计：`Arc<dyn StateStore>` 让所有并发分支共享同一全局状态，而 `locals` 副本保证分支隔离。