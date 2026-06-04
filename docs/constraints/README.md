# `koakuma-constraints` — 内置 Constraint 实现

> **Cargo 包名**: `koakuma-constraints` · **路径**: `crates/constraints/`
> **最后同步**: 2026-06-02

## 职责概述

`koakuma-constraints` 提供平台特定的 `Constraint` 实现（如 `ActiveWindowConstraint`），与 `koakuma-core::builtins` 中平台无关的约束互补。M1.1 将这些实现接入 `Registry`，M2.2 引入完整的 DSL 表达式引擎。

## 模块索引

| 模块 | 文件 | 说明 |
|------|------|------|
| [`lib`](lib.md) | `src/lib.rs` | crate 根，当前为占位（M1.1 起填充） |

## 对外依赖

| Crate | 用途 |
|-------|------|
| `koakuma-core` | `Constraint` trait、`ConstraintConfig`、`EvalContext` |

## 计划实现（按里程碑）

| 里程碑 | Constraint | 说明 |
|--------|------------|------|
| M1.1 | `ActiveWindowConstraint` | 检查前台窗口标题（字符串或正则匹配） |
| M2.2 | DSL 表达式引擎 | 解析 `ConstraintConfig::Expression { dsl }` |