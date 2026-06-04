# `error` — 错误类型

> **Crate**: `korkuma-core` · **文件**: `crates/core/src/error.rs`
> **最后同步**: 2026-06-02

## 职责

`error` 模块集中定义 `korkuma-core` 各层的错误类型，使用 `thiserror` 派生 `std::error::Error`。各错误类型与管道的不同阶段一一对应，便于调用方精确处理。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `HookError` | enum | `HookProvider::start` 失败时返回 |
| `RegistryError` | enum | `Registry::build_*` 找不到匹配 provider 时返回 |
| `ConstraintError` | enum | `Constraint::evaluate` 失败时返回（可由 `RegistryError` 转换） |
| `ActionError` | enum | `Action::execute` 失败时返回 |

#### `HookError` 变体

| 变体 | 说明 |
|------|------|
| `InitFailed(String)` | Hook 初始化失败，附带原因 |
| `AlreadyRunning` | Provider 已在运行，不能重复启动 |

#### `RegistryError` 变体

| 变体 | 说明 |
|------|------|
| `UnknownProvider(String)` | 没有注册的 factory 能处理该 Config |

#### `ConstraintError` 变体

| 变体 | 说明 |
|------|------|
| `Registry(RegistryError)` | 叶节点 build 失败（自动 `#[from]` 转换） |
| `EvalFailed(String)` | 求值过程中运行时错误 |

#### `ActionError` 变体

| 变体 | 说明 |
|------|------|
| `Failed(String)` | 通用执行失败 |
| `PermissionDenied(String)` | 缺少所需权限 |
| `Cancelled` | 执行被 `CancellationToken` 中断 |

## 依赖关系

该模块无任何 `use crate::` 内部依赖，仅依赖 `thiserror`。

## 设计说明

`ConstraintError::Registry` 使用 `#[from]` 使 `RegistryError` 可通过 `?` 自动转换，避免调用方手动 `map_err`。

`ActionError::Cancelled` 无附加信息——取消是正常控制流，不需要错误上下文。