# `korkuma-app` — Slint GUI 入口

> **Cargo 包名**: `korkuma-app` · **路径**: `crates/app/`
> **最后同步**: 2026-06-04 (M2.4 重构：main.rs 拆分为 7 个模块)

## 职责概述

`korkuma-app` 是可执行文件 `korkuma` 的入口，负责：渲染后端探测、构建 `Registry`（注册所有内置 + 平台 provider + Rhai 脚本）、启动引擎线程、桥接 UI 回调与 `EngineCommand`，以及 `macros.json` 热重载。

## 模块索引

| 模块 | 文件 | 说明 |
|------|------|------|
| [`main`](main.md) | `src/main.rs` | 精简编排入口：组件组装、启动顺序、graceful shutdown |
| [`setup`](setup.md) | `src/setup.rs` | 区域语言规范化、渲染后端探测（硬件 GL vs 软件回退） |
| [`trigger`](trigger.md) | `src/trigger.rs` | 触发器配置格式化、UI ↔ 领域双向转换 |
| [`model`](model.md) | `src/model.rs` | UI 模型刷新、编辑器三面板回填、热重载模型重建 |
| [`engine_fmt`](engine_fmt.md) | `src/engine_fmt.rs` | 引擎事件 → 日志行格式化 |
| [`watcher`](watcher.md) | `src/watcher.rs` | macros.json 热重载监听（增量引擎同步） |
| [`callbacks`](callbacks.md) | `src/callbacks.rs` | 全部 `on_*` 回调注册 + `persist` + 默认宏工厂 |
| [`tree_model`](tree_model.md) | `src/tree_model.rs` | 约束/工作流树扁平化与编辑操作（M2.4） |
| UI 定义 | `src/ui.slint` | Slint DSL UI 布局（不生成独立文档） |

## 对外依赖

| Crate | 用途 |
|-------|------|
| `korkuma-core` | 引擎类型、`Registry`、`EngineCommand`/`EngineEvent`、`permission` |
| `korkuma-hooks` | 平台 `HookProvider` 注册 |
| `korkuma-actions` | 内置 Action 注册 |
| `korkuma-script` | Rhai ScriptAction / ScriptConstraint 注册 |
| `korkuma-store` | `InMemoryStateStore`、`load_macros`、`save_macros` |
| `slint` | UI 框架，`ui.slint` 编译为 Rust 绑定（femtovg + software features） |
| `notify` | 文件系统事件监听（macros.json 热重载） |
| `crossbeam-channel` | engine sender 跨线程传递 |
| `uuid` | 新建宏的 UUID 生成 |
| `serde_json` | macros.json 序列化 / 反序列化 |

## 构建说明

`build.rs` 调用 `slint_build::compile("src/ui.slint")` 生成 Rust 绑定，`main.rs` 通过 `slint::include_modules!()` 宏引入，生成的 `MainWindow`、`MacroItem`、`LogEntry` 等类型可在子模块中通过 `use crate::TypeName` 引用。