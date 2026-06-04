# `action` — InteractAction 与工厂注册

> **Crate**: `korkuma-interact` · **文件**: `crates/interact/src/action.rs`
> **最后同步**: 2026-06-04 (M2.3 初始实现)

## 职责

`InteractAction` 是 `ActionConfig::Interact` 的运行时实现：它持有 `BackendRegistry` 的 `Arc` 引用，在 `execute` 时：

1. 检查是否持有 `WindowInteraction` / `BrowserControl` 权限（`required_permissions` 声明，引擎门控）
2. 委托 `BackendRegistry::dispatch` 执行 `UiOp`
3. 将 `DispatchError` 映射为 `ActionError`

`register_actions` 将工厂函数注册到 `Registry`，使 `Registry::build_action(ActionConfig::Interact { .. })` 可以正常工作。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `InteractAction` | struct | `(target, op, on_no_background, registry: Arc<BackendRegistry>)`；实现 `async_trait::Action` |

### 函数

| 签名 | 说明 |
|------|------|
| `register_actions(registry: &mut Registry, backend_registry: Arc<BackendRegistry>)` | 向全局 `Registry` 注册 `Interact` 工厂；在 app 启动时调用 |

### 权限声明规则

| 目标类型 | 所需权限 |
|---------|---------|
| `BrowserTab` | `BrowserControl` |
| 其他（Window / Process / Foreground） | `WindowInteraction` |
| `on_no_background: Degrade`（任意目标） | +`ForegroundTakeover` |

## 依赖关系

- [`negotiator`](negotiator.md) — `BackendRegistry`
- [`error`](error.md) — `DispatchError`
- [`korkuma_core::domain`](../core/domain.md) — `ActionConfig::Interact`、`TargetSelector`、`UiOp`、`OnNoBackground`
- [`korkuma_core::permission`](../core/permission.md) — `Permission`、`PermissionSet`
- [`korkuma_core::traits`](../core/traits.md) — `Action`、`Outcome`

## 设计说明

`register_actions` 通过闭包捕获 `Arc<BackendRegistry>`，与 `korkuma-script::register_actions` 的模式一致。每个 `InteractAction` 实例克隆 `Arc`（廉价引用计数），所有实例共享同一 `BackendRegistry`。

权限门控由两层保证：`required_permissions()` 声明让引擎在执行前进行静态检查；`BackendRegistry::dispatch` 在 `Degrade` 路径上再次检查 `ForegroundTakeover`，防止权限绕过。
