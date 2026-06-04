# `workflow` — 异步工作流引擎

> **Crate**: `korkuma-core` · **文件**: `crates/core/src/workflow.rs`
> **最后同步**: 2026-06-04 (M2.4 更新：`run_action` 新增 `dry_run` 短路逻辑)

## 职责

`workflow` 是 M2.1 引入的**异步控制流引擎**，负责三元组的 Action 腿执行。在管道
`Hook → Constraint → Action` 中，`router` 完成同步、无锁的事件匹配与约束求值后，把命中的宏的
[`WorkflowNode`](domain.md) 树交给本模块在引擎的 Tokio 运行时上异步驱动。它取代了 V1 的扁平动作循环。

权限门控集中在此：每个 `Action` 在执行前由 `run_action` 校验 `required_permissions()`，未获全部授权则不执行并产生 `EngineEvent::Error`。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `Flow` | enum | 节点执行的控制流结果：`Continue` / `Stop` / `Failed` |

#### `Flow`

工作流引擎内部信号；公开的 [`Action`](traits.md) 仍只用 [`Outcome`](traits.md)（`Continue`/`Stop`）。
`Failed` 与 `Stop` 区分开，使 `WorkflowNode::Retry` 只在**真正失败**（构建错误、权限拒绝、动作 `Err`、超时）时重试，而正常的 `Stop` 不触发重试。

### 函数

| 签名 | 说明 |
|------|------|
| `async fn run_workflow(root: &WorkflowNode, ctx: &mut ExecContext, reg: &Registry) -> Vec<EngineEvent>` | 执行工作流树，返回产生的引擎事件（M2.1 仅错误事件；动作日志走 `LogHandle`）。终止 `Flow` 被丢弃，调用方只需事件。 |

> 递归解释器 `run_node` 为私有。因 `async fn` 不能直接自递归，其返回类型为装箱 future
> `Pin<Box<dyn Future<Output = (Flow, Vec<EngineEvent>)> + Send + '_>>`。
> **M2.2 起加上 `+ Send`**，使调度器可通过 `tokio::spawn` 在多线程运行时上调度工作流。

## 节点语义

| 节点 | 行为 |
|------|------|
| `Action` | 构建 → 权限门控 → **获取资源锁**（M2.2）→ `await` 执行该动作 → 释放锁。 |
| `Seq` | 依次执行子节点；遇首个 `Stop` 或 `Failed` 即中止并向上传播。 |
| `Parallel` | 各子节点用 [`ExecContext::fork`](context.md) 的独立上下文并发执行（`join_all`）；局部变量隔离，全局 `StateStore` 共享。任一 `Failed` → `Failed`，否则任一 `Stop` → `Stop`。 |
| `If` | 用 `ConstraintExpr` 求值 `cond`，执行 `then` 或 `otherwise`；条件求值出错 → `Failed`。 |
| `While` | `cond` 为真则循环执行 `body`，由 `max_iter` 封顶；每轮检查取消标志。 |
| `ForEach` | 遍历字面量 `Value::List`，将每个元素绑定到局部变量 `var` 后执行 `body`。 |
| `Retry` | `body` 失败时最多重试至 `times` 次，每次间隔 `backoff_ms` 异步睡眠。 |
| `Timeout` | 用 `tokio::time::timeout` 包裹 `body`，超时则触发 `ctx.cancel` 并返回 `Failed` + 超时错误事件。 |
| `Wait` | 阻塞于 `WaitCondition`（M2.1 实现 `Duration`，即 `tokio::time::sleep`）。 |

## 依赖关系

依赖以下同 workspace 模块：
- [`domain`](domain.md) — `WorkflowNode`、`WaitCondition`、`ConstraintExpr`、`ActionConfig`
- [`context`](context.md) — `ExecContext`（含 `fork`）、`EvalContext`
- [`engine`](engine.md) — `EngineEvent`（返回值）
- [`registry`](registry.md) — `Registry::build_action`
- [`traits`](traits.md) — `Action`、`Outcome`
- [`error`](error.md) — `ConstraintError`
- [`value`](value.md) — `Value`（`ForEach` 列表）

外部依赖 `futures::future::join_all`（并发分支）、`tokio::time`（`sleep`/`timeout`）。

## 设计说明

- **取消语义**：每个 `run_node` 入口检查 `ctx.cancel.is_cancelled()`，命中即返回 `Stop`。`Timeout` 超时后调用 `ctx.cancel.cancel()`，使嵌套的取消感知工作（如 Rhai 脚本的 `on_progress`）中止。
- **`Parallel` 的局部变量隔离**：分支用 `fork()` 各得一份局部变量副本，写入不回流父上下文；分支间只能通过共享的 `StateStore` 通信。这是有意的安全默认，避免并发写局部变量的竞态。
- **V1 兼容**：没有 `workflow` 字段的旧宏由 [`Macro::root_workflow`](domain.md) 包装为 `Seq`，语义与 V1 顺序执行完全一致。
- **M2.2 资源仲裁**：`run_action` 在执行每个动作前调用 `ctx.resource_pool.get_lock(id)` 获取 `Arc<TokioMutex<()>>`，再调用 `lock_owned().await` 持有 `OwnedMutexGuard`，动作完成后 guard 自动释放。此机制通过 `ExecContext.resource_pool` 在同一调度器派发的所有工作流间共享，跨宏资源冲突因此有效。
- **M2.2 SendΔ**：`NodeFuture` 加了 `+ Send` 约束，允许调度器以 `tokio::spawn` 在多线程运行时并发执行工作流。调用方无需改变，`run_workflow` 接口不变。
- **M2.4 dry_run**：`run_action` 在权限门控和资源锁之前检查 `ctx.dry_run`；若为 `true`，直接返回一个 `EngineEvent::ActionLog { message: "[DRY RUN] …" }` 并跳过实际执行。仅私有辅助函数 `action_summary` 新增（不影响公开 API）。
