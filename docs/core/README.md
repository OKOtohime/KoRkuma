# `koakuma-core` — 核心引擎与领域模型

> **Cargo 包名**: `koakuma-core` · **路径**: `crates/core/`
> **最后同步**: 2026-06-03 (M2.1)

## 职责概述

`koakuma-core` 是整个平台的基础层，定义所有跨平台共享的领域类型、trait 契约、事件路由、约束求值和引擎命令协议。其他 crate（actions、hooks、store、constraints、app）均依赖此 crate，但此 crate 不依赖它们，保持单向依赖图。

## 模块索引

| 模块 | 文件 | 说明 |
|------|------|------|
| [`value`](value.md) | `src/value.rs` | 核心动态值类型 `Value` |
| [`event`](event.md) | `src/event.rs` | `Event` 与 `EventKind` — 管道入口 |
| [`domain`](domain.md) | `src/domain.rs` | 核心数据结构：`Macro`、`TriggerConfig`、`ActionConfig`、`ConstraintExpr` 等 |
| [`error`](error.md) | `src/error.rs` | 所有错误类型 |
| [`permission`](permission.md) | `src/permission.rs` | 权限声明与运行时授权 |
| [`traits`](traits.md) | `src/traits.rs` | 核心扩展点：`HookProvider`、`TriggerSpec`、`Constraint`、`Action` |
| [`context`](context.md) | `src/context.rs` | `EvalContext` 与 `ExecContext` — trait 实现的运行时入参 |
| [`state`](state.md) | `src/state.rs` | `StateStore` trait — 全局变量存储抽象 |
| [`registry`](registry.md) | `src/registry.rs` | 工厂注册表，将 Config 转换为 trait object |
| [`router`](router.md) | `src/router.rs` | `EventRouter` — O(1) 事件分发与完整管道执行 |
| [`workflow`](workflow.md) | `src/workflow.rs` | （M2.1）异步工作流引擎，驱动 `WorkflowNode` 控制流树 |
| [`engine`](engine.md) | `src/engine.rs` | 引擎命令/事件协议 (`EngineCommand`、`EngineEvent`) |
| [`engine_loop`](engine_loop.md) | `src/engine_loop.rs` | `start_engine` — 引擎线程 + 双通道事件循环 + Tokio 运行时 |
| [`builtins`](builtins.md) | `src/builtins.rs` | 平台无关的内置 trait 实现 |

## 对外依赖

| Crate | 用途 |
|-------|------|
| `serde` + `serde_json` | 领域类型序列化（`Macro` 存储与传输） |
| `uuid` | `MacroId` 类型 |
| `thiserror` | 错误类型派生 |
| `crossbeam-channel` | `EventSink`（`Sender<Event>`）与引擎命令通道 |
| `async-trait` | `Action::execute` 异步化且保持 dyn 兼容（M2.1） |
| `tokio` | 引擎多线程运行时、`time::sleep`/`timeout`（M2.1） |
| `futures` | `join_all` 并发执行 `Parallel` 工作流分支（M2.1） |

## 内部依赖关系图

```
value ◄──── domain ◄──┬── router
  ▲                   ├── builtins
  │         event ◄───┘
  │           ▲
  │         context ◄─── router
  │           ▲            ▲
  │         traits ◄── builtins
  │
permission ◄─ domain
  ▲
  └── context

error ◄── registry ◄── builtins ◄── router
state ◄── context ◄─── router
engine ◄──────────────── router
workflow ◄── router          (workflow 依赖 domain/context/registry/traits/engine)
```