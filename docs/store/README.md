# `koakuma-store` — 状态存储 + 宏配置持久化

> **Cargo 包名**: `koakuma-store` · **路径**: `crates/store/`
> **最后同步**: 2026-06-02 (M1.3 update)

## 职责概述

`koakuma-store` 承担两个职责：

1. 提供 `koakuma_core::state::StateStore` trait 的具体实现（`InMemoryStateStore`），用于开发、测试及 M1 阶段运行时的全局键值状态存储。
2. 提供宏配置的 JSON 原子读写（`load_macros` / `save_macros`），供 `koakuma-app` 在启动和 UI 操作时持久化 `macros.json`。

## 模块索引

| 模块 | 文件 | 说明 |
|------|------|------|
| [`lib`](lib.md) | `src/lib.rs` | `InMemoryStateStore`、`StoreError`、`load_macros`、`save_macros` |

## 对外依赖

| Crate | 用途 |
|-------|------|
| `koakuma-core` | `StateStore` trait、`Value` 类型、`Macro` 领域模型 |
| `serde_json` | JSON 序列化/反序列化 |
| `thiserror` | `StoreError` 的派生宏 |