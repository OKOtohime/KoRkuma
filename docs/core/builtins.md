# `builtins` — 平台无关内置实现

> **Crate**: `korkuma-core` · **文件**: `crates/core/src/builtins.rs`
> **最后同步**: 2026-06-03 (M2.1：Action 异步化)

## 职责

`builtins` 模块提供所有平台无关的 `TriggerSpec`、`Constraint`、`Action` 内置实现，并在 `Registry::with_builtins()` 中统一注册。平台特定实现（如 Win32 快捷键、窗口焦点）放在 `korkuma-hooks` 和 `korkuma-constraints`，在 `app` 启动时追加注册。

该模块的所有类型均为 `pub(crate)`，不对外暴露具体实现，外部代码只通过 `Registry` 工厂访问这些能力。

## 内置组件清单

### TriggerSpec 实现

| 内部类型 | 对应 TriggerConfig | 说明 |
|----------|--------------------|------|
| `ManualTriggerSpec` | `TriggerConfig::Manual` | 匹配 `EventKind::Manual` |

### Constraint 实现

| 内部类型 | 对应 ConstraintConfig | 说明 |
|----------|-----------------------|------|
| `TimeRangeConstraint` | `ConstraintConfig::TimeRange { from, to }` | UTC 时间范围检查，正确处理跨午夜 |
| `VarCompareConstraint` | `ConstraintConfig::VarCompare { key, op, value }` | 从 StateStore 读取变量并与字面值比较 |

#### `TimeRangeConstraint` 设计细节

时间以"距午夜的分钟数"表示。跨午夜判断：若 `from_min > to_min`（如 23:00 → 01:00），则 `now >= from OR now <= to`；否则 `from <= now <= to`。时刻取自 `ctx.event.timestamp`（UTC），不依赖系统时钟，保证确定性测试。

#### `VarCompareConstraint` 设计细节

支持的比较操作：`Eq`、`Ne`、`Lt`、`Gt`、`Le`、`Ge`。顺序比较（`Lt`/`Gt`/`Le`/`Ge`）仅支持同类型的 `Int`、`Float`、`Str`；类型不匹配时返回 `false`（不报错）。

### Action 实现

| 内部类型 | 对应 ActionConfig | 说明 |
|----------|-------------------|------|
| `SetVariableAction` | `ActionConfig::SetVariable { scope, key, value }` | 写入全局或局部变量 |
| `DelayAction` | `ActionConfig::Delay { millis }` | 异步等待指定毫秒（`tokio::time::sleep`） |

#### `SetVariableAction`

- `VarScope::Global` → 写入 `ctx.store`
- `VarScope::Local` → 写入 `ctx.locals`（仅本次执行可见）
- 不需要任何权限（`required_permissions` 返回空集）

#### `DelayAction`

M2.1 起 `execute` 为 `async fn`，使用 `tokio::time::sleep(...).await` 让出运行时而非阻塞工作线程（V1 曾用 `std::thread::sleep`）。两个内置 Action 均以 `#[async_trait]` 实现。

## 依赖关系

依赖以下同 workspace 模块：
- [`context`](context.md) — `EvalContext`、`ExecContext`
- [`domain`](domain.md) — `ActionConfig`、`ConstraintConfig`、`TriggerConfig`、`VarScope`、`CompareOp`
- [`error`](error.md) — `ActionError`、`ConstraintError`
- [`event`](event.md) — `Event`、`EventKind`
- [`permission`](permission.md) — `PermissionSet`
- [`traits`](traits.md) — `Action`、`Constraint`、`Outcome`、`TriggerSpec`
- [`value`](value.md) — `Value`

## 设计说明

所有 `build_*` 函数是自由函数而非方法，签名为 `fn(&XxxConfig) -> Option<Box<dyn Xxx>>`，与 `Registry::register_*` 接受的 factory 类型完全匹配，可直接作为函数指针传入，无需闭包包装。