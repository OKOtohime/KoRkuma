# `callbacks` — UI 回调注册与宏持久化

> **Crate**: `korkuma-app` · **文件**: `crates/app/src/callbacks.rs`
> **最后同步**: 2026-06-05 (M2.5：权限管理回调)

## 职责

注册 Slint `MainWindow` 的全部 `on_*` 事件处理器，将用户操作路由为引擎命令（`EngineCommand`）并持久化到 `macros.json`。同时提供宏数据的持久化辅助函数与默认宏工厂。

## 公开 API

### 函数

| 签名 | 说明 |
|------|------|
| `persist(macros: &[Macro], suppress_reload: &AtomicBool)` | 先设 `suppress_reload=true` 再调 `save_macros`，避免 watcher 触发冗余 UI 刷新；保存失败时复位 flag |
| `create_default_macro() -> Macro` | 生成带 `Manual` 触发器 + `Notify` action 的新宏，调用 `aggregate_from_configs` 自动填充 `granted_permissions` |
| `wire_callbacks(ui, local_macros, macros_model, engine_sender, ui_weak, suppress_reload)` | 注册所有 UI 回调（见下表） |

### 注册的回调

#### 宏级操作

| 回调 | 行为 |
|------|------|
| `on_add_macro` | 创建默认宏，追加到列表，发送 `AddMacro`，选中并刷新编辑器 |
| `on_delete_macro(idx)` | 发送 `DeleteMacro`，从列表移除，更新选中索引 |
| `on_toggle_enabled(idx, enabled)` | 更新 `m.enabled`，发送 `SetEnabled`，同步 `MacroItem` 行 |
| `on_trigger_macro(idx)` | 发送 `TriggerManually`，刷新编辑器 |
| `on_dry_run_macro(idx)` | 发送 `DryRunMacro` |
| `on_macro_selected(idx)` | 刷新三面板编辑器 |
| `on_rename_macro(idx, name)` | 更新 `m.name`，同步 `MacroItem` 行 |
| `on_move_macro_up(idx)` | 列表 swap，同步 model 行，更新选中索引 |
| `on_move_macro_down(idx)` | 列表 swap，同步 model 行，更新选中索引 |

#### 约束编辑

| 回调 | 行为 |
|------|------|
| `on_constraint_node_selected(idx)` | 将该行 `params_json` 填入编辑 JSON 框 |
| `on_constraint_update_node(idx, json)` | 调 `update_constraint_leaf`，重建 `ConstraintRow` 列表 |
| `on_constraint_delete_node(idx)` | 调 `delete_constraint_at`，重建列表，清空选中 |
| `on_constraint_add_leaf(leaf_type)` | 调 `add_constraint_leaf`，重建列表 |
| `on_constraint_wrap(kind)` | 调 `wrap_constraint_at`（AND/OR/NOT），重建列表 |
| `on_constraint_move_up(idx)` | 调 `move_constraint_node(…, true)` |
| `on_constraint_move_down(idx)` | 调 `move_constraint_node(…, false)` |

#### 工作流编辑

| 回调 | 行为 |
|------|------|
| `on_workflow_node_selected(idx)` | 填入 `params_json`；若为 `Interact` 类型，还解析 `TargetSelector`/`OnNoBackground` 填入专属字段 |
| `on_workflow_update_node(idx, json)` | 调 `update_workflow_action`，重建 `WorkflowRow` 列表 |
| `on_workflow_delete_node(idx)` | 调 `delete_workflow_at`，重建列表，清空选中 |
| `on_workflow_add_action(action_type)` | 调 `add_workflow_action`，重建列表 |
| `on_workflow_add_if` | 调 `add_workflow_if`，重建列表 |
| `on_workflow_add_parallel` | 调 `add_workflow_parallel`，重建列表 |
| `on_workflow_move_up(idx)` | 调 `move_workflow_node(…, true)` |
| `on_workflow_move_down(idx)` | 调 `move_workflow_node(…, false)` |

#### 触发器编辑

| 回调 | 行为 |
|------|------|
| `on_trigger_select(tidx)` | 调 `populate_trigger_fields`，回显触发器参数到 UI 字段 |
| `on_trigger_add(kind)` | `push` 默认触发器配置，重建 `TriggerRow` 列表 |
| `on_trigger_delete(tidx)` | `remove` 指定触发器，重建列表，清空选中 |
| `on_trigger_apply` | 调 `build_trigger_from_ui`，用当前 UI 字段值更新所选触发器，重建列表 |

## 依赖关系

- [`korkuma_core::domain`](../core/domain.md) — `Macro`、`ActionConfig`、`ConstraintExpr`、`TriggerConfig`、`TargetSelector`、`OnNoBackground`
- [`korkuma_core::engine`](../core/engine.md) — `EngineCommand`
- [`korkuma_core::permission`](../core/permission.md) — `aggregate_from_configs`
- [`korkuma_store`](../store/README.md) — `save_macros`
- [`crate::model`](model.md) — `rebuild_model`、`refresh_editor`、`to_slint_constraint_rows`、`to_slint_workflow_rows`
- [`crate::trigger`](trigger.md) — `build_trigger_from_ui`、`default_trigger_config`、`populate_trigger_fields`、`to_slint_trigger_rows`
- [`crate::tree_model`](tree_model.md) — 约束/工作流树的所有编辑函数
- `crate::{ConstraintRow, MacroItem, MainWindow, TriggerRow, WorkflowRow, MACROS_PATH}` — Slint 生成类型与路径常量

## 设计说明

**闭包捕获策略**：每个 `on_*` 块均单独 `Arc::clone` / `.clone()` 捕获所需的共享量，不共享同一个 `Arc` 集合，避免 `Clone`-heavy 捕获。

**所有写操作均以 `persist` 结尾**：约定每个修改宏数据的回调最后均调用 `persist`，确保 UI 与磁盘始终一致；`persist` 内部处理 suppress_reload，无需回调自行管理。

## M2.5 权限管理回调

| 回调 | 行为 |
|------|------|
| `request-save()` | 用 `aggregate_from_workflow` 聚合选中宏整棵工作流的权限，写入 `pending-permissions` 模型并置 `show-permission-dialog = true`（弹出审批对话框） |
| `approve-permissions()` | 关闭对话框，重新聚合并写入 `granted_permissions`，发送 `UpdateMacro` + `persist`，刷新权限行 |
| `cancel-permissions()` | 仅关闭对话框，不改动授权 |
| `revoke-permission(int)` | 从选中宏 `granted_permissions` 移除该下标权限，发送 `UpdateMacro` + `persist`，刷新权限行；下发后该宏对应动作在 `workflow::run_action` 中央门控被拦截 |

撤销/审批均经 `UpdateMacro` 让 `EventRouter` 以新 `granted_permissions` 重新注册宏，运行时门控立即生效。