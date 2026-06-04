# `trigger_spec` — 跨平台 TriggerSpec 实现

> **Crate**: `korkuma-hooks` · **文件**: `crates/hooks/src/trigger_spec.rs`
> **最后同步**: 2026-06-02

## 职责

提供三种 `TriggerSpec` 实现，对应 `korkuma-hooks` 生产的三类事件。这些 spec 在 `EventRouter` 的第二级过滤中调用：路由器先以 `EventKind` O(1) 粗筛，再调用 `matches()` 精筛。

该模块无任何平台条件编译，在所有目标 OS 上均可编译和测试。

## 事件 Payload 契约

各 Provider 产生的事件使用以下固定 payload 结构，TriggerSpec 依赖这些字段进行匹配：

| EventKind | Payload 字段 | 类型 | 说明 |
|---|---|---|---|
| `Hotkey` | `key` | `Str` | 键名（`"A"`、`"F5"`、`"Enter"` 等） |
| `Hotkey` | `modifiers` | `List<Str>` | 活跃修饰键子集：`"Ctrl"`, `"Shift"`, `"Alt"`, `"Win"` |
| `Hotkey` | `vk_code` | `Int` | 原始 Windows 虚拟键码 |
| `WindowFocus` | `title` | `Str` | 窗口标题文本 |
| `WindowFocus` | `exe` | `Str` | 可执行文件名（M1.2 暂为空字符串） |
| `Process` | `name` | `Str` | 可执行文件名（如 `"notepad.exe"`） |
| `Process` | `pid` | `Int` | 进程 ID |
| `Process` | `event` | `Str` | `"started"` 或 `"stopped"` |

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `HotkeyTriggerSpec` | struct | 匹配 `EventKind::Hotkey` 事件 |
| `WindowFocusTriggerSpec` | struct | 匹配 `EventKind::WindowFocus` 事件 |
| `ProcessTriggerSpec` | struct | 匹配 `EventKind::Process` 事件 |

#### `HotkeyTriggerSpec`

匹配时将触发配置中的 `Vec<KeyCombo>` 与事件 payload 对比（OR 语义）。键名和修饰键名均忽略大小写，修饰键顺序不影响结果（集合比较）。

```rust
pub struct HotkeyTriggerSpec { keys: Vec<KeyCombo> }

impl HotkeyTriggerSpec {
    pub fn new(keys: Vec<KeyCombo>) -> Self;
}
```

#### `WindowFocusTriggerSpec`

用大小写不敏感的**子串搜索**匹配窗口标题。`regex = true` 在 M1.4 脚本引擎接入后支持，M1.2 两种模式均降级为子串匹配。

```rust
pub struct WindowFocusTriggerSpec { title_pattern: String, regex: bool }

impl WindowFocusTriggerSpec {
    pub fn new(title_pattern: String, regex: bool) -> Self;
}
```

#### `ProcessTriggerSpec`

进程名做大小写不敏感**子串搜索**（`"notepad"` 匹配 `"notepad.exe"`），事件类型做精确匹配（`"started"` / `"stopped"`）。

```rust
pub struct ProcessTriggerSpec { name: String, event: ProcessEvent }

impl ProcessTriggerSpec {
    pub fn new(name: String, event: ProcessEvent) -> Self;
}
```

### 工厂函数（供 `Registry::register_trigger` 使用）

| 函数 | 处理的 TriggerConfig 变体 |
|------|--------------------------|
| `fn build_hotkey_spec(c: &TriggerConfig) -> Option<Box<dyn TriggerSpec>>` | `TriggerConfig::Hotkey` |
| `fn build_window_focus_spec(c: &TriggerConfig) -> Option<Box<dyn TriggerSpec>>` | `TriggerConfig::WindowFocus` |
| `fn build_process_spec(c: &TriggerConfig) -> Option<Box<dyn TriggerSpec>>` | `TriggerConfig::Process` |

## 依赖关系

依赖以下同 workspace 模块：
- `korkuma_core::domain` — `KeyCombo`、`ProcessEvent`、`TriggerConfig`
- `korkuma_core::event` — `Event`、`EventKind`
- `korkuma_core::traits` — `TriggerSpec`
- `korkuma_core::value` — `Value`

## 设计说明

payload 字段读取使用 `payload.get("key")` 返回 `Option<&Value>` 的模式匹配。Rust 2024 edition 下 match ergonomics 会自动处理引用，无需显式 `ref`（已修正 M1.2 实现中的 4 处误用）。

`regex` 字段保留在结构体中是为了将来在 M1.4 中接入 `korkuma-script` 的 Rhai 正则匹配，届时只需在 `WindowFocusTriggerSpec::matches` 中添加正则分支，调用方无需修改 `TriggerConfig` 数据格式。
