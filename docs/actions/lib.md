# `lib` — korkuma-actions crate 根

> **Crate**: `korkuma-actions` · **文件**: `crates/actions/src/lib.rs`
> **最后同步**: 2026-06-02

## 职责

声明三个子模块，re-export 公开类型，并提供 `register_all` 一键将所有内置 Action 工厂注册到 `Registry`。

## 公开 API

### 类型（re-export）

| 类型 | 来源 | 说明 |
|------|------|------|
| `RunCommandAction` | `run_command` | 外部进程执行 |
| `NotifyAction` | `notify` | 桌面通知 |
| `SimulateInputAction` | `simulate_input` | 键鼠输入模拟 |

### 函数 / 方法

| 签名 | 说明 |
|------|------|
| `fn register_all(registry: &mut Registry)` | 将 `run_command`、`notify`、`simulate_input` 工厂注册到 `Registry` |

## 依赖关系

- [`run_command`](run_command.md) — `build` factory
- [`notify`](notify.md) — `build` factory
- [`simulate_input`](simulate_input.md) — `build` factory
- `korkuma_core::registry::Registry` — 注册入参

## 设计说明

`SetVariable` 和 `Delay` 已在 `korkuma-core/builtins` 预注册（通过 `Registry::with_builtins()`），因此 `register_all` 无需处理它们。
