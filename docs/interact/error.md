# `error` — 后端与分发错误类型

> **Crate**: `koakuma-interact` · **文件**: `crates/interact/src/error.rs`
> **最后同步**: 2026-06-04 (M2.3 初始实现)

## 职责

定义两层错误：`BackendError`（单个后端内部错误）和 `DispatchError`（`BackendRegistry::dispatch` 的分发级错误）。区分二者使得协商逻辑可以静默跳过不可用后端，同时向调用方传达明确的失败原因。

## 公开 API

### `BackendError`

| 变体 | 说明 |
|------|------|
| `NotFound(String)` | 后端找到了选择器但找不到具体元素 |
| `NotAvailable(String)` | 后端本身不可用（CDP 端口未开、COM 初始化失败等） |
| `NotSupported(String)` | 后端不支持该操作（例如 win-msg 不支持 SetText） |
| `Internal(String)` | 执行过程中的意外错误 |

### `DispatchError`

| 变体 | 说明 |
|------|------|
| `NoBackend` | 没有任何后端能服务该目标（`Fail` 策略或所有后端不可用时） |
| `Queued` | `Queue` 策略信号；操作已挂起，待目标可达时重试 |
| `Backend(BackendError)` | 从 `BackendError` 自动转换（`#[from]`） |
| `PermissionDenied(String)` | 缺少 `ForegroundTakeover` 权限（Degrade 策略前检查） |

## 依赖关系

无内部依赖；`thiserror` 派生 `Display + Error`。
