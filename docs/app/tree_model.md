# `tree_model` — 约束/工作流扁平树模型与编辑操作

> **Crate**: `korkuma-app` · **文件**: `crates/app/src/tree_model.rs`
> **最后同步**: 2026-06-04 (M2.4：新增模块)

## 职责

`tree_model` 为可视化编辑器（DESIGN.md §16.2）提供 `ConstraintExpr` 和 `WorkflowNode` 的**扁平化表示**及**双向编辑操作**。

Slint 的 `ListView` 需要扁平的 `VecModel`；而 `ConstraintExpr`/`WorkflowNode` 是递归树。本模块提供两方向的桥接：

- **深度优先展开**（`flatten_*`）：树 → `Vec<*TreeRow>`，每行携带 `depth`（缩进深度）、`kind`（节点类型）、`summary`（人类可读摘要）、`params_json`（可编辑的 JSON 参数）。
- **原位编辑**：通过平铺行的序号（flat position）找到树中对应节点，执行插入/删除/更新。

该模块是 `korkuma-app` 内部私有模块（`mod tree_model`），不向外 crate 暴露。

## 公开 API（crate 内可见）

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `ConstraintTreeRow` | struct | 约束树的一个扁平行（对应 Slint `ConstraintRow` 的数据来源） |
| `WorkflowTreeRow` | struct | 工作流树的一个扁平行（对应 Slint `WorkflowRow` 的数据来源） |

#### `ConstraintTreeRow` 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `depth` | `i32` | 缩进深度（×16px） |
| `kind` | `String` | `"Always"` / `"Not"` / `"All"` / `"Any"` / `"Leaf"` |
| `leaf_type` | `String` | 叶节点变体名（`"ActiveWindow"` / `"TimeRange"` 等），非叶为空 |
| `summary` | `String` | 人类可读的一行摘要 |
| `params_json` | `String` | 叶节点的序列化 `ConstraintConfig`（组节点为空） |

#### `WorkflowTreeRow` 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `depth` | `i32` | 缩进深度 |
| `kind` | `String` | `"Action"` / `"Seq"` / `"Parallel"` / `"If"` / `"Then"` / `"Else"` / `"While"` / `"ForEach"` / `"Retry"` / `"Timeout"` / `"Wait"` |
| `action_type` | `String` | Action 节点的变体名（`"Notify"` 等），非 Action 为空 |
| `summary` | `String` | 人类可读摘要 |
| `params_json` | `String` | Action 节点的序列化 `ActionConfig`，非 Action 为空 |
| `is_container` | `bool` | 是否为容器节点（有子节点） |

### 约束树函数

| 签名 | 说明 |
|------|------|
| `fn flatten_constraint(expr: &ConstraintExpr) -> Vec<ConstraintTreeRow>` | 深度优先展开约束树 |
| `fn default_constraint_config(leaf_type: &str) -> ConstraintConfig` | 指定类型的默认 `ConstraintConfig` |
| `fn add_constraint_leaf(root, leaf) -> ConstraintExpr` | 向根节点追加叶子 |
| `fn wrap_constraint_and(root) -> ConstraintExpr` | 将根包裹进 `All`（已是 `All` 则不变） |
| `fn wrap_constraint_or(root) -> ConstraintExpr` | 将根包裹进 `Any` |
| `fn wrap_constraint_not(root) -> ConstraintExpr` | 将根包裹进 `Not` |
| `fn update_constraint_leaf(root, pos: usize, new_json: &str) -> ConstraintExpr` | 更新 flat position `pos` 处叶节点的 `ConstraintConfig`；JSON 解析失败则返回原树 |
| `fn delete_constraint_at(root, pos: usize) -> ConstraintExpr` | 删除 flat position `pos` 处的节点及其子树 |

### 工作流树函数

| 签名 | 说明 |
|------|------|
| `fn flatten_workflow(node: &WorkflowNode) -> Vec<WorkflowTreeRow>` | 深度优先展开工作流树 |
| `fn default_action_config(action_type: &str) -> ActionConfig` | 指定类型的默认 `ActionConfig` |
| `fn add_workflow_action(root, cfg: ActionConfig) -> WorkflowNode` | 追加 Action 节点到顶层 `Seq` |
| `fn add_workflow_if(root) -> WorkflowNode` | 追加 `If`（`Always` 条件，空 `Seq` 体） |
| `fn add_workflow_parallel(root) -> WorkflowNode` | 追加 `Parallel`（2 个空 `Seq` 分支） |
| `fn update_workflow_action(root, pos: usize, new_json: &str) -> WorkflowNode` | 更新 flat position `pos` 处 Action 节点的 `ActionConfig` |
| `fn delete_workflow_at(root, pos: usize) -> WorkflowNode` | 删除 flat position `pos` 处的节点及其子树 |

## 依赖关系

- `korkuma_core::domain` — `ConstraintExpr`、`ConstraintConfig`、`WorkflowNode`、`ActionConfig`、`TargetSelector`、`UiOp`、`OnNoBackground`
- `korkuma_core::value` — `Value`（`VarCompare` 默认值）
- `serde_json` — 序列化/反序列化 params JSON

## 设计说明

**Flat position（行号）**的语义：`flatten_*` 的深度优先遍历顺序即是平铺行的顺序。编辑操作通过在树上**模拟同样的深度优先计数**来定位目标节点，使得"行号 → 树节点"查找不需要额外的路径编码，但要求编辑前后必须使用同一次 flatten 结果作为参照。

`If` 节点在展开时额外插入 `"Then"` / `"Else"` 标签行（`is_container=true`），用于视觉上区分两个分支，计数也因此包含这些虚拟行。`delete_workflow_at` / `update_workflow_action` 的递归计数器与 `flatten_workflow_rec` 保持完全相同的行为。

删除含子树的组节点（`All`、`Any`、`Not`、`Seq`、`Parallel` 等）时，同时删除全部子节点，通过 `skip_*_count` 辅助函数跳过子树的计数偏移，确保后续行号不出错。

简化规则：`All`/`Any` 删除子节点后若只剩 1 个子节点，自动展开为该子节点；0 个子节点退化为 `Always`/`Seq(vec![])`。
