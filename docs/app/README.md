# `korkuma-app` — Slint GUI 入口

> **Cargo 包名**: `korkuma-app` · **路径**: `crates/app/`
> **最后同步**: 2026-06-04 (M2.4：新增 `tree_model` 模块)

## 职责概述

`korkuma-app` 是可执行文件 `korkuma` 的入口（M1.4），负责：渲染后端探测、构建 `Registry`（注册所有内置 + 平台 provider + Rhai 脚本）、启动引擎线程、桥接 UI 回调与 `EngineCommand`，以及 `macros.json` 热重载。

## 模块索引

| 模块 | 文件 | 说明 |
|------|------|------|
| [`main`](main.md) | `src/main.rs` | 程序入口：组件组装、UI 回调、热重载 watcher |
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

`build.rs` 调用 `slint_build::compile("src/ui.slint")` 生成 Rust 绑定，`main.rs` 通过 `slint::include_modules!()` 宏引入，生成的 `MainWindow`、`MacroItem`、`LogEntry` struct 可直接使用。