# `registry` — 插件工厂注册表

> **Crate**: `korkuma-core` · **文件**: `crates/core/src/registry.rs`
> **最后同步**: 2026-06-02

## 职责

`registry` 模块实现一个函数指针注册表，将序列化的 Config 枚举值转换为运行时 trait object。`Registry` 是平台的扩展点总线：内置实现通过 `with_builtins()` 预注册，平台特定或用户自定义的实现在 `app` 启动时通过 `register_*` 追加。

这使得 `domain` 层的 Config（纯数据）与运行时 trait object（含行为）完全解耦，支持序列化存储和动态扩展。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `Registry` | struct | 持有三类 factory 函数列表的注册表 |

### 函数 / 方法

| 签名 | 说明 |
|------|------|
| `Registry::new() -> Self` | 创建空注册表 |
| `Registry::with_builtins() -> Self` | 创建并预注册所有内置实现 |
| `register_trigger<F>(&mut self, f: F)` | 注册一个触发器 factory |
| `register_constraint<F>(&mut self, f: F)` | 注册一个约束 factory |
| `register_action<F>(&mut self, f: F)` | 注册一个动作 factory |
| `build_trigger(&self, c: &TriggerConfig) -> Result<Box<dyn TriggerSpec>, RegistryError>` | 按 Config 实例化触发器 |
| `build_constraint(&self, c: &ConstraintConfig) -> Result<Box<dyn Constraint>, RegistryError>` | 按 Config 实例化约束 |
| `build_action(&self, c: &ActionConfig) -> Result<Box<dyn Action>, RegistryError>` | 按 Config 实例化动作 |

#### Factory 函数签名

```rust
// 注册时传入的 factory 闭包类型
Fn(&TriggerConfig)    -> Option<Box<dyn TriggerSpec>>
Fn(&ConstraintConfig) -> Option<Box<dyn Constraint>>
Fn(&ActionConfig)     -> Option<Box<dyn Action>>
```

返回 `None` 表示该 factory 不处理此 Config 变体，注册表继续尝试下一个。返回 `Some` 即命中。

## 依赖关系

依赖以下同 workspace 模块：
- [`builtins`](builtins.md) — `with_builtins()` 调用其 build 函数
- [`domain`](domain.md) — Config 类型
- [`error`](error.md) — `RegistryError`
- [`traits`](traits.md) — `TriggerSpec`、`Constraint`、`Action`

## 设计说明

匹配策略是线性扫描 `find_map`：factory 列表短（通常 5–20 个）且只在宏触发时调用，O(n) 开销可忽略。若未来需要高频匹配，可改为 `HashMap<TypeId, FactoryFn>`，但目前是过早优化。

Factory 要求 `Send + Sync + 'static`，确保 `Registry` 本身也可安全跨线程共享（以 `Arc<Registry>` 传递给 `EventRouter`）。