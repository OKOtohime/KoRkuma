# `lib` — crate 根模块

> **Crate**: `korkuma-core` · **文件**: `crates/core/src/lib.rs`
> **最后同步**: 2026-06-03 (M2.1：新增 `workflow` 模块)

## 职责

crate 根模块，声明并公开所有子模块。无独立逻辑，是 `korkuma-core` 的模块入口。

## 公开模块

```rust
pub mod builtins;
pub mod context;
pub mod domain;
pub mod engine;
pub mod engine_loop;
pub mod error;
pub mod event;
pub mod permission;
pub mod registry;
pub mod router;
pub mod state;
pub mod traits;
pub mod value;
pub mod workflow;   // M2.1：异步工作流引擎
```

所有模块均以 `pub mod` 导出，外部 crate 通过 `korkuma_core::<module>::<Type>` 访问。

## 设计说明

透传模块，无需额外文档。详见各子模块文档。