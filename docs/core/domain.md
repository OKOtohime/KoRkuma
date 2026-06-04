# `domain` — 核心领域数据结构

> **Crate**: `korkuma-core` · **文件**: `crates/core/src/domain.rs`
> **最后同步**: 2026-06-04 (M2.3：新增 `TargetSelector`、`OnNoBackground`、`UiPath`、`UiOp`、`ActionConfig::Interact`)

## 职责

`domain` 模块是整个平台的数据模型层，定义"宏"（Macro）这一核心三元组：触发器 + 约束 + 动作序列。所有类型均可序列化，使宏可持久化存储、跨进程传输、跨版本迁移。

该模块不包含任何执行逻辑，只是纯粹的数据描述，是 `registry`、`router`、`builtins` 的输入来源。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `MacroId` | type alias | `uuid::Uuid`，宏的唯一标识符 |
| `KeyCombo` | struct | 键盘快捷键描述（修饰键 + 主键） |
| `ProcessEvent` | enum | 进程事件种类：`Started` / `Stopped` |
| `FsEventKind` | enum | 文件系统事件：`Created` / `Modified` / `Deleted` |
| `VarScope` | enum | 变量命名空间：`Global`（跨宏） / `Local`（当次执行） |
| `CompareOp` | enum | 比较运算符：`Eq`、`Ne`、`Lt`、`Gt`、`Le`、`Ge` |
| `ScriptLang` | enum | 支持的脚本语言（当前仅 `Rhai`） |
| `InputEvent` | enum | 模拟输入序列的单步操作 |
| `ValueTemplate` | type alias | `Value` 的别名，V2 将扩展为模板字符串 |
| `TriggerConfig` | enum | 触发器配置（序列化形式） |
| `ActionConfig` | enum | 动作配置（序列化形式） |
| `ConstraintConfig` | enum | 约束叶节点配置（序列化形式） |
| `ConstraintExpr` | enum | 布尔表达式树（AND / OR / NOT） |
| `WaitCondition` | enum | `WorkflowNode::Wait` 的等待条件（M2.1：`Duration`） |
| `WorkflowNode` | enum | 可执行工作流树节点（M2.1，含控制流） |
| `ConcurrencyPolicy` | enum | 宏重复触发时的并发策略（M2.2，6 个变体） |
| `TargetSelector` | enum | **M2.3** 后台交互的目标选择器（序列化形式） |
| `UiPath` | type alias | `String`：UI 元素路径，格式因后端而异 |
| `UiOp` | enum | **M2.3** 后台 UI 操作（Click / SetText / SendKeys / Focus / ReadValue） |
| `OnNoBackground` | enum | **M2.3** 无后台能力时的降级策略（Degrade / Fail / Queue） |
| `Macro` | struct | 完整宏定义 |

#### `ConcurrencyPolicy` 变体（M2.2）

| 变体 | 说明 |
|------|------|
| `Parallel`（默认） | 每次触发均独立运行（V1 行为） |
| `Queue { max }` | 串行排队；超出 `max` 容量时丢弃 |
| `DropIfRunning` | 有实例运行时忽略新触发 |
| `RestartIfRunning` | 取消旧实例，重新开始 |
| `Debounce { ms }` | 等待 `ms` 毫秒，期间新触发会重置计时（取最后一次） |
| `Throttle { ms }` | 每 `ms` 窗口最多执行一次（取第一次） |

#### `TriggerConfig` 变体

| 变体 | 说明 |
|------|------|
| `Hotkey { keys }` | 一个或多个快捷键组合（OR 语义） |
| `WindowFocus { title_pattern, regex }` | 窗口标题匹配（字符串或正则） |
| `Process { name, event }` | 进程事件监听 |
| `Schedule { cron }` | cron 表达式定时触发 |
| `FileChange { path, kind }` | 文件系统变更监听 |
| `Manual` | 手动触发（无硬件事件来源） |
| `Custom { provider, params }` | 第三方 provider 扩展点 |

#### `ActionConfig` 变体

| 变体 | 说明 |
|------|------|
| `RunCommand { program, args, capture }` | 运行外部命令 |
| `Notify { title, body }` | 系统通知 |
| `SimulateInput { sequence }` | 模拟键盘/鼠标输入序列 |
| `HttpRequest { method, url, body }` | HTTP 请求 |
| `SetVariable { scope, key, value }` | 写入状态变量 |
| `Delay { millis }` | 等待指定毫秒 |
| `RunScript { lang, source }` | 执行脚本（Rhai） |
| `Interact { target, op, on_no_background }` | **M2.3** 后台 UI 自动化操作（见 `korkuma-interact`） |
| `Custom { provider, params }` | 第三方 provider 扩展点 |

#### `ConstraintExpr` — 布尔表达式树

```rust
pub enum ConstraintExpr {
    Always,                              // 恒为 true
    Leaf { constraint: ConstraintConfig }, // 叶节点
    Not  { expr: Box<ConstraintExpr> },  // 逻辑非
    All  { exprs: Vec<ConstraintExpr> }, // 逻辑与（短路）
    Any  { exprs: Vec<ConstraintExpr> }, // 逻辑或（短路）
}
```

`ConstraintExpr::evaluate(&self, ctx, reg)` 递归求值，叶节点委托 `Registry::build_constraint` 实例化并调用 `Constraint::evaluate`。

#### `WorkflowNode` — 工作流树（M2.1）

V2 用此递归树取代 V1 的扁平动作列表，承载控制流。由 [`workflow::run_workflow`](workflow.md) 异步解释执行。`#[serde(tag = "node")]` 标记。

| 变体 | 说明 |
|------|------|
| `Action(ActionConfig)` | 叶动作 |
| `Seq(Vec<WorkflowNode>)` | 顺序执行，遇 Stop/失败中止 |
| `Parallel(Vec<WorkflowNode>)` | 并发执行（各分支 fork 上下文，局部变量隔离） |
| `If { cond, then, otherwise }` | 按 `ConstraintExpr` 条件分支 |
| `While { cond, body, max_iter }` | 条件循环，`max_iter` 封顶 |
| `ForEach { items, var, body }` | 遍历字面量 `Value::List`，绑定到局部 `var` |
| `Retry { body, times, backoff_ms }` | 失败重试，指数间隔 |
| `Timeout { body, millis }` | 超时则失败 |
| `Wait { until }` | 阻塞于 `WaitCondition` |

`WaitCondition` 当前仅 `Duration { millis }`（`tokio::time::sleep`）；`Event` / `VarPredicate` 等待预留给 M2.2 调度器。

#### `Macro` 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | `MacroId` | 唯一标识 |
| `name` | `String` | 用户可见名称 |
| `description` | `String` | 用户描述 |
| `enabled` | `bool` | 是否参与路由分发 |
| `category` | `Option<String>` | 分组标签 |
| `triggers` | `Vec<TriggerConfig>` | OR 语义：任一触发则求值 |
| `constraints` | `ConstraintExpr` | 约束表达式树，为 `Always` 时无条件执行 |
| `actions` | `Vec<ActionConfig>` | 扁平动作列表（V1）；当 `workflow` 为 `None` 时作为回退工作流 |
| `workflow` | `Option<WorkflowNode>` | （M2.1）控制流工作流树；`#[serde(default)]` 使旧配置可加载；非空时优先于 `actions` |
| `granted_permissions` | `PermissionSet` | 宏保存时预授权的操作集合 |
| `priority` | `i32` | （M2.2）派发优先级，值越高越先执行；默认 `0`；`#[serde(default)]` |
| `concurrency` | `ConcurrencyPolicy` | （M2.2）重复触发时的并发策略；默认 `Parallel`；`#[serde(default)]` |

`Macro::root_workflow(&self) -> WorkflowNode`：返回待执行的根节点——有 `workflow` 则原样返回，否则把 `actions` 包成 `Seq`，使 V1 宏获得等价顺序语义。

## 依赖关系

依赖以下同 workspace 模块：
- [`value`](value.md) — `ValueTemplate`、`ConstraintConfig::VarCompare` 的值类型
- [`permission`](permission.md) — `PermissionSet`
- [`context`](context.md) — `ConstraintExpr::evaluate` 的参数类型
- [`registry`](registry.md) — `ConstraintExpr::evaluate` 中调用 `Registry::build_constraint`
- [`error`](error.md) — `ConstraintError`

## 设计说明

`ValueTemplate` 当前是 `Value` 的别名，V2 将扩展为支持模板插值的字符串（例如 `"{{event.payload.key}}"`），届时将成为独立类型。

所有 Config 枚举使用 `#[serde(tag = "type")]` 标记联合体（`WorkflowNode` 用 `tag = "node"`），保证 JSON 反序列化时可区分变体且具备前向兼容性。

**M2.1 向后兼容**：`Macro::workflow` 为 `#[serde(default)]` 的 `Option`，旧 `macros.json`（仅有 `actions`）零改动即可加载，经 `root_workflow` 包成 `Seq` 顺序执行。这是 V1→V2 schema 演进的无破坏迁移路径。

**M2.3 新增类型**：

`TargetSelector` 变体（`#[serde(tag = "type")]`，`Default = Foreground`）：

| 变体 | 说明 |
|------|------|
| `Foreground`（默认） | 当前前台窗口 |
| `Window { title_pattern, regex }` | 按标题匹配窗口 |
| `Process { name }` | 按进程名匹配（MVP 实现为标题 substring 搜索） |
| `BrowserTab { url_pattern }` | 按 URL 匹配浏览器标签（CDP 或 WebExt） |
| `Custom { provider, params }` | 插件自定义选择器 |

`UiOp` 变体（`#[serde(tag = "op")]`），均含 `node: UiPath`（CSS selector 或 `"name:X"` / `"id:X"` 格式）：

| 变体 | 说明 |
|------|------|
| `Click { node }` | 点击 / invoke 元素 |
| `SetText { node, text }` | 设置输入值 |
| `SendKeys { keys }` | 注入键盘事件（Vec\<KeyCombo\>） |
| `Focus { node: Option<..> }` | 聚焦元素（None = 窗口/标签根） |
| `ReadValue { node }` | 读取元素值（日志记录） |

`OnNoBackground` 变体（`Default = Degrade`）：

| 变体 | 说明 |
|------|------|
| `Degrade` | 降级到 ForegroundSynthetic（需要 `ForegroundTakeover` 权限） |
| `Fail` | 不降级，直接报错 |
| `Queue` | 挂起等待目标可达（M2.4 调度器支持完整语义） |