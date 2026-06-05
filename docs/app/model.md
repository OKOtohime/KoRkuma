# `model` — UI 模型转换与编辑器刷新

> **Crate**: `korkuma-app` · **文件**: `crates/app/src/model.rs`
> **最后同步**: 2026-06-05 (M2.5：权限行刷新)

## 职责

在 `korkuma_core` 领域类型与 Slint `VecModel` UI 模型之间充当转换层，负责将宏数据写入三个编辑器面板（触发器 / 约束 / 工作流），并在热重载或选中变更时完整刷新 UI。

## 公开 API

### 函数 / 方法

| 签名 | 说明 |
|------|------|
| `rebuild_model<T: Clone + 'static>(model: &VecModel<T>, items: Vec<T>)` | 清空并重新填充任意 `VecModel`，逐行 `remove` + 再 `push`（Slint 不提供 `clear`）|
| `to_slint_constraint_rows(rows: &[ConstraintTreeRow]) -> Vec<ConstraintRow>` | 将扁平化约束树行转换为 Slint `ConstraintRow` |
| `to_slint_workflow_rows(rows: &[WorkflowTreeRow]) -> Vec<WorkflowRow>` | 将扁平化工作流树行转换为 Slint `WorkflowRow` |
| `refresh_editor(ui: &MainWindow, macros: &[Macro], idx: usize)` | 用 `macros[idx]` 刷新三个编辑器面板；`idx` 越界时静默返回 |
| `reload_ui_model(ui: &MainWindow, macros: &[Macro])` | 重建宏列表 `VecModel<MacroItem>`，同步编辑器选中行，并插入热重载日志行 |

## 依赖关系

- [`korkuma_core::domain`](../core/domain.md) — `Macro`
- [`crate::tree_model`](tree_model.md) — `flatten_constraint`、`flatten_workflow`、`ConstraintTreeRow`、`WorkflowTreeRow`
- [`crate::trigger`](trigger.md) — `to_slint_trigger_rows`
- `crate::{ConstraintRow, LogEntry, MacroItem, MainWindow, TriggerRow, WorkflowRow}` — Slint 生成类型
- `crate::MACROS_PATH` — 热重载日志消息中显示文件名
- `slint::{Model, VecModel}` — `as_any().downcast_ref()` 访问具体 model

## 设计说明

**`as_any` 模式**：Slint 通过 `ModelRc<T>` 暴露 model，需用 `as_any().downcast_ref::<VecModel<T>>()` 取得具体类型。所有写操作均在此 downcast 成功后才执行，避免 panic。

**`reload_ui_model` 与 `refresh_editor` 分工**：`reload_ui_model` 只负责宏列表和日志；`refresh_editor` 只负责编辑器面板。`callbacks` 在普通操作（增删改宏）时直接调用 `refresh_editor`，避免重建整个列表。

**M2.5 权限行**：新增 `to_permission_rows(&[Permission]) -> Vec<PermissionRow>`（经 `Permission::describe` 生成标签）与 `refresh_permission_rows(&MainWindow, &Macro)`（重建 Permissions tab 列表）；`refresh_editor` 在切换宏时一并刷新权限行。变量监视器的 `VarRow` 不在此模块——由 `main` 的 `slint::Timer` 直接从 `StateStore::snapshot()` 构建。