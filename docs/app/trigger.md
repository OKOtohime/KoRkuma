# `trigger` — 触发器配置帮助函数

> **Crate**: `korkuma-app` · **文件**: `crates/app/src/trigger.rs`
> **最后同步**: 2026-06-04

## 职责

将 `korkuma_core::domain::TriggerConfig` 领域类型与 Slint 生成的 `TriggerRow` UI 类型之间的双向转换、显示格式化、以及从 UI 字段构造配置等职责集中管理。涵盖触发器的完整 CRUD 生命周期：列表展示 → 选中回显 → 编辑提交。

## 公开 API

### 函数 / 方法

| 签名 | 说明 |
|------|------|
| `format_hotkey(keys: &[KeyCombo]) -> String` | `[{Ctrl+F1}, {Alt+A}]` → `"Ctrl+F1, Alt+A"`；空列表返回 `"—"` |
| `parse_hotkey_str(s: &str) -> Vec<KeyCombo>` | `"Ctrl+F1"` → `[KeyCombo { modifiers: ["Ctrl"], key: "F1" }]`；空字符串返回 `vec![]` |
| `describe_trigger(t: &TriggerConfig) -> (String, String)` | 返回 `(kind, summary)` 元组，用于填充 `TriggerRow` |
| `to_slint_trigger_rows(triggers: &[TriggerConfig]) -> Vec<TriggerRow>` | 批量转换触发器列表为 Slint 显示行 |
| `default_trigger_config(kind: &str) -> TriggerConfig` | 按 kind 字符串（`"Schedule"`/`"Hotkey"`/…）返回带默认参数的 `TriggerConfig` |
| `populate_trigger_fields(ui: &MainWindow, trigger: &TriggerConfig)` | 将触发器参数写入 Slint UI 字段（cron / hotkey / title_pat / proc_name 等） |
| `build_trigger_from_ui(ui: &MainWindow, kind: &str) -> TriggerConfig` | 从 UI 当前字段值构造 `TriggerConfig`；未知 kind 回退为 `Manual` |

## 依赖关系

- [`korkuma_core::domain`](../core/domain.md) — `TriggerConfig`、`KeyCombo`、`ProcessEvent`
- `crate::MainWindow`、`crate::TriggerRow` — Slint 生成类型

## 设计说明

**无状态纯函数**：所有函数均为无副作用转换（除 `populate_trigger_fields` 写 UI 字段外），便于单元测试和复用。

**kind 字符串约定**：与 Slint `ComboBox` 的选项值完全匹配（`"Manual"/"Schedule"/"Hotkey"/"WindowFocus"/"Process"`）；`default_trigger_config` 和 `build_trigger_from_ui` 的 `_` 分支均回退到 `Manual`，避免 panic。