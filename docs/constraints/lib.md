# `lib` — korkuma-constraints crate 根

> **Crate**: `korkuma-constraints` · **文件**: `crates/constraints/src/lib.rs`
> **最后同步**: 2026-06-02

## 职责

当前为文档占位模块，无运行时代码。M1.1 中将实现 `ActiveWindowConstraint` 并在此导出；`app` 启动时将其注册到 `Registry`。

## 设计说明

M2.2 将引入 `korkuma-script` crate（Rhai 沙箱），届时 `ConstraintConfig::Expression { dsl }` 的处理也在此 crate 实现，通过 `Registry::register_constraint` 注册。

详见 [README](README.md)。