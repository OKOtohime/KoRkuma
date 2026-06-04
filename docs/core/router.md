# `router` — 事件路由与管道执行

> **Crate**: `koakuma-core` · **文件**: `crates/core/src/router.rs`
> **最后同步**: 2026-06-04 (M2.4 更新：新增 `dispatch_dry_run`；`execute_pipeline` 增加 `dry_run` 参数)

## 职责

`router` 模块实现 `EventRouter`，是整个 Hook → Constraint → Action 管道的调度核心。它维护一个 `HashMap<EventKind, Vec<MacroId>>` 索引，使每个传入事件只访问订阅了该事件类别的宏，而非广播给全部宏，实现 O(1) 初步过滤。

一次 `dispatch` 调用完整执行：事件类别筛选 → 触发器精筛 → 约束求值 → **执行工作流**，并收集所有 `EngineEvent` 返回给调用方（引擎线程），由引擎转发至 UI 线程。

**M2.1 异步化**：匹配与约束求值仍是同步、无锁的；Action 腿改为异步执行——`dispatch` / `dispatch_manual_trigger` 现为 `async fn`，把宏的 [`WorkflowNode`](domain.md) 树委托给 [`workflow::run_workflow`](workflow.md)。引擎用 `Runtime::block_on` 驱动这些 future。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `EventRouter` | struct | 持有索引和宏注册表的事件调度器 |

### 函数 / 方法

| 签名 | 说明 |
|------|------|
| `EventRouter::new() -> Self` | 创建空路由器 |
| `add_macro(&mut self, m: Macro)` | 注册宏并更新 `EventKind` 索引 |
| `remove_macro(&mut self, id: MacroId)` | 注销宏并清理索引条目 |
| `set_enabled(&mut self, id: MacroId, enabled: bool)` | 启用/禁用宏（不移出索引，dispatch 时跳过） |
| `async dispatch(&self, event: &Event, registry: &Registry, store: &Arc<dyn StateStore>) -> Vec<EngineEvent>` | 执行完整管道，返回产生的引擎事件（M2.1 起为 async） |
| `async dispatch_manual_trigger(&self, macro_id: MacroId, registry: &Registry, store: &Arc<dyn StateStore>) -> Vec<EngineEvent>` | 跳过触发器匹配，直接为指定宏求约束+执行工作流（用于 `EngineCommand::TriggerManually`） |
| `dispatch_scheduled(&self, event: &Event, registry: &Arc<Registry>, store: &Arc<dyn StateStore>, scheduler: &WorkflowScheduler) -> Vec<EngineEvent>` | M2.2：同步版派发——按 priority 排序后将工作流提交给调度器；立即返回 `MacroFired` 事件 |
| `async dispatch_manual_trigger(&self, macro_id: MacroId, registry, store) -> Vec<EngineEvent>` | 跳过触发器匹配，直接为指定宏求约束+执行工作流（用于 `TriggerManually`） |
| `async dispatch_dry_run(&self, macro_id: MacroId, registry, store) -> Vec<EngineEvent>` | 同 `dispatch_manual_trigger`，但 `dry_run=true`：动作只记录 `[DRY RUN]` 日志（M2.4） |
| `snapshot(&self) -> EngineSnapshot` | 返回当前注册的所有宏的快照（用于 `EngineCommand::QuerySnapshot`） |

#### `dispatch` 执行流程（M2.1）

```
event.kind ──► index.get(kind)
                    │
              candidates: &[MacroId]
                    │
              for each macro_id:
                ├─ macro.enabled? ──► skip if false
                ├─ TriggerSpec::matches? ──► skip if false (OR across triggers)
                ├─ ConstraintExpr::evaluate? ──► skip if false; push Error if Err
                ├─ push MacroFired
                └─ execute_pipeline.await:
                     ├─ 构建 ExecContext
                     ├─ root = macro.root_workflow()   // workflow 或 actions 包成 Seq
                     └─ run_workflow(&root, &mut ctx, reg).await  // 见 workflow.md
```

**权限门控**已下沉到 [`workflow`](workflow.md) 的 `run_action`（每个动作执行前校验
`required_permissions()`），对所有节点统一生效。`execute_pipeline` 私有方法负责约束求值、
发出 `MacroFired`、构建 `ExecContext` 并调用 `run_workflow`，由 `dispatch` 和
`dispatch_manual_trigger` 共享。

## 依赖关系

依赖以下同 workspace 模块：
- [`domain`](domain.md) — `Macro`、`MacroId`、`TriggerConfig`、`Macro::root_workflow`
- [`event`](event.md) — `Event`、`EventKind`
- [`engine`](engine.md) — `EngineEvent`（返回值类型）
- [`context`](context.md) — `EvalContext`、`ExecContext`
- [`permission`](permission.md) — `PermissionGrant`
- [`registry`](registry.md) — `Registry`
- [`state`](state.md) — `StateStore`
- [`workflow`](workflow.md) — `run_workflow`（Action 腿执行）

## 设计说明

索引是 `HashMap<EventKind, Vec<MacroId>>`：在 `add_macro`/`remove_macro` 时维护，`dispatch` 时只读。这种设计允许引擎线程在收到 `EngineCommand::AddMacro` 后立刻更新索引，无需全量重建。

触发器匹配使用 OR 语义（`m.triggers.iter().any(...)`）：一个宏可以有多个触发器，任意一个匹配即触发求值。

**M2.1**：动作执行从同步内联循环改为异步 `run_workflow`，由引擎的 Tokio 运行时经 `block_on` 驱动。Router 自身仍是单线程、无锁；跨宏并发是 M2.2 调度器的职责。权限门控从此前的 `execute_pipeline` 内联检查下沉到 `workflow::run_action`。

**M2.2**：新增 `dispatch_scheduled`——Router 仍单线程无锁，只负责触发器匹配、约束求值和优先级排序；实际工作流执行交给 `WorkflowScheduler`。原有 `dispatch`（async）保留以供后向兼容（手动触发与测试）。两个路径均按 `Macro.priority` 降序执行。

`dispatch` 与 `dispatch_manual_trigger` 共享私有 `async` 方法 `execute_pipeline`（约束求值 + 工作流执行）。`dispatch_manual_trigger` 跳过触发器类别筛选和 `TriggerSpec::matches`，合成一个 `EventKind::Manual` 事件作为 context。

**M2.4**：新增 `dispatch_dry_run`——与 `dispatch_manual_trigger` 路径相同，但在 `ExecContext` 中设置 `dry_run=true`。`workflow::run_action` 检测到该标志后跳过实际执行，改为发出带 `[DRY RUN]` 前缀的 `ActionLog` 事件，使 UI 可以在不触发副作用的前提下预览工作流执行路径。

**最后同步**: 2026-06-03