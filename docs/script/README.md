# `korkuma-script` — Rhai 脚本引擎集成

> **Cargo 包名**: `korkuma-script` · **路径**: `crates/script/`
> **最后同步**: 2026-06-02

## 职责概述

`korkuma-script` 将 [Rhai](https://rhai.rs) 嵌入式脚本引擎接入 KoRkuma，为高级用户提供超出内置 Action/Constraint 类型的灵活扩展能力：

- **`RunScriptAction`**：`ActionConfig::RunScript { lang: Rhai, source }` 对应的运行时实现，支持读写变量、访问事件数据、打印日志。
- **`ExpressionConstraint`**：`ConstraintConfig::Expression { dsl }` 对应的运行时实现，支持任意 Rhai 布尔表达式作为约束条件。

两者均受沙箱保护（操作数上限、调用层数上限、字符串大小上限），死循环脚本会被资源限制自动终止。执行 `RunScriptAction` 需要宏授予 `Permission::ScriptExecution`。

## 模块索引

| 模块 | 文件 | 说明 |
|------|------|------|
| [`lib`](lib.md) | `src/lib.rs` | 全部实现（RunScriptAction、ExpressionConstraint、工厂函数、注册函数） |

## 对外依赖

| Crate | 用途 |
|-------|------|
| `rhai = "1"` | Rhai 脚本引擎核心（`Engine`、`Scope`、`Dynamic`） |
| `korkuma-core` | 领域模型、trait 接口、权限类型 |
| `serde_json` | 序列化工具（辅助类型兼容） |

## 内部依赖关系图

```
lib.rs
  ├── korkuma_core::traits::{Action, Constraint, Outcome}
  ├── korkuma_core::context::{EvalContext, ExecContext}
  ├── korkuma_core::domain::{ActionConfig, ConstraintConfig}
  ├── korkuma_core::permission::{Permission, PermissionSet}
  ├── korkuma_core::state::StateStore
  ├── korkuma_core::error::{ActionError, ConstraintError}
  ├── korkuma_core::value::Value
  └── rhai::{Engine, Scope, Dynamic, Map}
```