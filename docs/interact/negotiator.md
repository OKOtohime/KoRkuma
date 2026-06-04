# `negotiator` — 能力协商与降级策略

> **Crate**: `koakuma-interact` · **文件**: `crates/interact/src/negotiator.rs`
> **最后同步**: 2026-06-04 (M2.3 初始实现)

## 职责

`BackendRegistry` 持有所有已注册的后端，在 `dispatch` 时执行能力协商：

1. 并发调用所有后端的 `resolve` 收集候选目标
2. 按 `Tier` 降序排列（Background 优先）
3. 尝试最高层级后端；若失败或无 Background 后端，根据 `OnNoBackground` 策略处理

协商完全对调用方透明——`InteractAction` 只需传入 `TargetSelector + UiOp + OnNoBackground`。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `BackendRegistry` | struct | 后端注册表；`new()` / `register<B>` / `dispatch` / `enumerate_nodes` |

### 函数 / 方法

| 签名 | 说明 |
|------|------|
| `BackendRegistry::new() -> Self` | 创建空注册表 |
| `BackendRegistry::register<B: InteractionBackend + 'static>(&mut self, b: B)` | 注册后端（注册顺序在同 tier 内有序） |
| `BackendRegistry::dispatch(sel, op, policy, permissions) -> Result<(), DispatchError>` | 协商并执行；`async` |
| `BackendRegistry::enumerate_nodes(sel) -> Result<Vec<UiNode>, DispatchError>` | 枚举目标 UI 节点（供目标选择器 UI，M2.4）；`async` |

### dispatch 决策树

```
collect_candidates(sel)
  ├─ has Background candidate? → invoke → Done
  └─ no Background:
       ├─ Fail → DispatchError::NoBackend
       ├─ Queue → DispatchError::Queued
       └─ Degrade:
            ├─ no ForegroundTakeover perm? → PermissionDenied
            ├─ has ForegroundSynthetic? → invoke → Done
            └─ none → NoBackend
```

## 依赖关系

- [`backend`](backend.md) — `InteractionBackend`、`ResolvedTarget`、`Tier`、`UiNode`
- [`error`](error.md) — `DispatchError`
- [`koakuma_core::domain`](../core/domain.md) — `OnNoBackground`、`TargetSelector`、`UiOp`
- [`koakuma_core::permission`](../core/permission.md) — `Permission::ForegroundTakeover`、`PermissionGrant`

## 设计说明

`collect_candidates` 当前串行调用各后端 `resolve`（`await` 依次执行）。后端数量通常 ≤ 4，串行开销可忽略；如有需要可并发化。

`OnNoBackground::Queue` 仅返回 `DispatchError::Queued`；完整的等待队列语义由 M2.4 的 `WorkflowScheduler` 实现——届时调度器可监听目标就绪事件并重新触发工作流。
