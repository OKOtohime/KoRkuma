# `traits` — 核心扩展点 trait

> **Crate**: `korkuma-core` · **文件**: `crates/core/src/traits.rs`
> **最后同步**: 2026-06-03 (M2.2：`Action` 新增 `resources()` 方法)

## 职责

`traits` 模块定义平台的核心扩展接口。第三方开发者（或平台内置的 `korkuma-actions`、`korkuma-hooks`、`korkuma-constraints`）通过实现这些 trait 向平台注入能力，所有实现均通过 `Registry` 注册，与核心引擎解耦。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `EventSink` | type alias | `crossbeam_channel::Sender<Event>`，HookProvider 推送事件的通道端 |
| `Outcome` | enum | Action 执行后的控制流信号 |

#### `Outcome` 变体

`Outcome` 实现了 `#[derive(Debug)]`（M1.4 补充），可直接用于测试断言的格式化输出。

| 变体 | 说明 |
|------|------|
| `Continue` | 继续执行下一个 Action |
| `Stop` | 中止当前宏的剩余 Action |

### Trait

#### `HookProvider`

```rust
pub trait HookProvider: Send {
    fn id(&self) -> &'static str;
    fn produces(&self) -> &'static [EventKind];
    fn start(&mut self, sink: EventSink) -> Result<(), HookError>;
    fn stop(&mut self);
}
```

平台特定事件源，运行于独立线程。`produces()` 声明该 provider 产生的 `EventKind` 集合，引擎用此信息预构建路由索引，避免无关事件被分发。

#### `TriggerSpec`

```rust
pub trait TriggerSpec: Send + Sync {
    fn subscribed_kinds(&self) -> &[EventKind];
    fn matches(&self, event: &Event) -> bool;
}
```

从 `TriggerConfig` 实例化，执行细粒度的每事件匹配。`EventRouter` 先以 `EventKind` 粗筛（O(1)），再调用 `matches` 精筛，实现两级过滤。

#### `Constraint`

```rust
pub trait Constraint: Send + Sync {
    fn evaluate(&self, ctx: &EvalContext) -> Result<bool, ConstraintError>;
}
```

约束表达式树的叶节点。`ConstraintExpr::Leaf` 通过 `Registry::build_constraint` 实例化此 trait，再调用 `evaluate`。实现者不应持有可变状态——`evaluate` 签名为 `&self`。

#### `Action`

```rust
#[async_trait]
pub trait Action: Send + Sync {
    fn id(&self) -> &'static str;
    fn required_permissions(&self) -> PermissionSet;
    fn resources(&self) -> Vec<String> { vec![] }  // M2.2
    async fn execute(&self, ctx: &mut ExecContext) -> Result<Outcome, ActionError>;
}
```

单步可执行动作。**M2.1 起 `execute` 为 `async fn`**，由 `#[async_trait]`（`dtolnay/async-trait`）保持 trait 的 dyn 兼容性——`Registry` 仍以 `Box<dyn Action>` 存储、[`workflow`](workflow.md) 引擎 `await` 执行。实现者应对异步 I/O（定时、网络）使用 `.await` 而非阻塞运行时（如 `Delay` 用 `tokio::time::sleep`）。`required_permissions` 供 UI 在宏保存时提示授权；中央权限门控在 `workflow::run_action`。

**M2.2 新增 `resources()`**：声明该动作执行时需要独占访问的资源 ID 列表（如 `"input"`、`"clipboard"`、`"window:<id>"`）。默认实现返回空——不需要资源隔离的动作无需重写。调度器（通过 `workflow::run_action`）在执行前获取对应的 `OwnedMutexGuard` 并持至完成，防止多宏并发写入同一设备。内置资源名见 `scheduler::RESOURCE_INPUT` / `RESOURCE_CLIPBOARD`。

## 依赖关系

依赖以下同 workspace 模块：
- [`event`](event.md) — `Event`、`EventKind`
- [`context`](context.md) — `EvalContext`、`ExecContext`
- [`error`](error.md) — `HookError`、`ConstraintError`、`ActionError`
- [`permission`](permission.md) — `PermissionSet`

## 设计说明

`HookProvider` 仅要求 `Send`（不要求 `Sync`），因为每个 provider 独占其线程，不需要共享引用。其余三个 trait 要求 `Send + Sync` 以支持并发求值。

`Action` 经 `#[async_trait]` 生成的 `execute` future 默认带 `Send` 约束。即便实现内部使用非 `Send` 类型（如 Rhai `Engine`），只要不跨 `.await` 持有即可——`RunScriptAction` 全程同步执行脚本、无内部 await 点，因此 future 仍为 `Send`。`Outcome` 仍只有 `Continue`/`Stop`；分支/循环由 [`WorkflowNode`](domain.md) 的结构承载，而非 `Outcome` 跳转。