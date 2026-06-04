# `lib` — Rhai 脚本集成（RunScript + Expression DSL）

> **Crate**: `korkuma-script` · **文件**: `crates/script/src/lib.rs`
> **最后同步**: 2026-06-03 (M2.1：`RunScriptAction` 异步化)

## 职责

> **M2.1**：`RunScriptAction` 改为 `#[async_trait]` 的 `async fn execute`。脚本仍同步执行（Rhai 引擎无内部 await 点，future 因此为 `Send`）；`ExpressionConstraint::evaluate` 保持同步。新增 `async-trait` 依赖。

`korkuma-script` 是 M1.4 引入的脚本执行层，将 Rhai 嵌入式脚本引擎接入 KoRkuma 管道，提供两个能力：

1. **`RunScriptAction`**：允许宏的动作列表中直接内嵌一段 Rhai 代码，可读取事件数据、读写状态变量、调用宿主函数白名单。
2. **`ExpressionConstraint`**：允许约束树的叶节点使用任意 Rhai 布尔表达式（替代固定的 `VarCompare`/`TimeRange` 类型），表达更复杂的条件逻辑。

两者均通过 Rhai 沙箱（`set_max_operations`、`set_max_call_levels`、`set_max_string_size`）防止无限循环和资源耗尽，符合 DESIGN.md §7 对脚本安全的要求。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `RunScriptAction` | struct | 实现 `Action` trait，执行 Rhai 源码 |
| `ExpressionConstraint` | struct | 实现 `Constraint` trait，求值 Rhai 布尔表达式 |

#### `RunScriptAction` 沙箱参数

| 参数 | 值 |
|------|---|
| `max_operations` | 50 000 |
| `max_call_levels` | 50 |
| `max_string_size` | 1 MiB |
| 取消信号 | `CancellationToken` → `engine.on_progress` |

#### `ExpressionConstraint` 沙箱参数

| 参数 | 值 |
|------|---|
| `max_operations` | 10 000 |
| `max_call_levels` | 20 |
| `max_string_size` | 256 KiB |

### 宿主 API（`RunScriptAction` 脚本内可用）

| 函数 | 签名 | 说明 |
|------|------|------|
| `get_var` | `(key: &str) -> Dynamic` | 先查 locals，后查全局 Store；键不存在返回 `()` |
| `set_var` | `(key: &str, val: Dynamic)` | 延迟写入，脚本结束后批量应用到 Store |
| `log` | `(msg: &str)` | 向 stdout 打印 `[SCRIPT] msg` |

`ExpressionConstraint` 仅暴露 `get_var`（只读）；不暴露 `set_var`/`log`。

### 函数

| 签名 | 说明 |
|------|------|
| `build_run_script(c: &ActionConfig) -> Option<Box<dyn Action>>` | 工厂：匹配 `ActionConfig::RunScript`，构建 `RunScriptAction` |
| `build_expression(c: &ConstraintConfig) -> Option<Box<dyn Constraint>>` | 工厂：匹配 `ConstraintConfig::Expression`，构建 `ExpressionConstraint` |
| `register_actions(registry: &mut Registry)` | 向 Registry 注册 `RunScript` 动作工厂 |
| `register_constraints(registry: &mut Registry)` | 向 Registry 注册 `Expression` 约束工厂 |

### 内部工具函数（私有）

| 函数 | 说明 |
|------|------|
| `value_to_dynamic(&Value) -> Dynamic` | `Value` → Rhai `Dynamic`；`Map` 转 `RhaiMap`，`List` 转 `Vec<Dynamic>` |
| `dynamic_to_value(Dynamic) -> Value` | `Dynamic` → `Value`；不支持的类型返回 `Value::Null` |
| `build_sandbox(max_ops, max_levels, max_str) -> Engine` | 统一构建带沙箱限制的 `Engine::new()` 实例 |

## 依赖关系

依赖以下同 workspace 模块：
- [`context`](../core/context.md) — `EvalContext`、`ExecContext`、`CancellationToken`
- [`domain`](../core/domain.md) — `ActionConfig`、`ConstraintConfig`
- [`error`](../core/error.md) — `ActionError`、`ConstraintError`
- [`permission`](../core/permission.md) — `Permission`、`PermissionSet`
- [`registry`](../core/registry.md) — `Registry`（注册工厂）
- [`state`](../core/state.md) — `StateStore`（`get_var`/`set_var` 操作）
- [`traits`](../core/traits.md) — `Action`、`Constraint`、`Outcome`
- [`value`](../core/value.md) — `Value`（Value ↔ Dynamic 转换）

外部依赖：
- `rhai = "1"` — Rhai 脚本引擎，提供 `Engine`、`Scope`、`Dynamic`、`Map`

## 设计说明

**延迟写入**：`set_var` 注册的闭包将写操作追加到 `Arc<Mutex<Vec<(String, Value)>>>` 中，脚本运行完毕后再批量 `ctx.store.set()`。这避免了在闭包捕获中持有 `&mut ExecContext` 引用（Rust 借用规则不允许），同时保证脚本中多次 `set_var` 的顺序语义。

**快照读**：`get_var` 捕获 `locals_snap`（执行时的本地变量克隆）和 `Arc<dyn StateStore>` 引用。读 locals 优先，再查 Store。这意味着脚本 *看不到* 自身之前 `set_var` 调用的中间结果（写入在脚本结束后才生效）。这是 V1 的已知局限，V2 可引入双缓冲或即时写入策略。

**沙箱不阻止网络/文件系统**：`Engine::new()` 包含 Rhai 标准库，但不包含文件 I/O 或网络调用（这些在 Rhai 标准库中不存在）。V1 的安全边界是：脚本无法通过 Rhai 标准函数访问 OS 资源；但可通过宿主函数 `get_var`/`set_var`/`log` 间接影响系统状态，后者均属于安全操作。需要更高权限的宿主函数（如 `run_command`）应在 V2 中以 `Permission::RunCommand` 守卫注册。

**`ExpressionConstraint` 无取消令牌**：`EvalContext` 不包含 `CancellationToken`，因此约束求值只靠 `max_operations` 终止，不支持外部中断。对于约束场景（通常是轻量计算）这是可接受的 V1 折衷。