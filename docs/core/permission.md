# `permission` — 权限声明与运行时授权

> **Crate**: `korkuma-core` · **文件**: `crates/core/src/permission.rs`
> **最后同步**: 2026-06-05 (M2.5：新增 `aggregate_from_workflow`、`Permission::describe`；权限聚合下沉到工作流树)

## 职责

`permission` 模块实现"宏保存时预授权"机制（Permission Manager）。用户在编辑宏时声明所需权限（`PermissionSet`），保存后这些权限固化为 `Macro.granted_permissions`。运行时，引擎将其转换为 `PermissionGrant`，Action 在执行前通过 `PermissionGrant::allows` 检查授权。

该机制确保所有敏感操作（文件 I/O、网络、输入模拟、脚本执行）在宏激活前已经过用户确认，而非在触发时弹框打断用户。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `PathScope` | enum | 文件路径授权范围 |
| `Permission` | enum | 单项权限声明 |
| `PermissionSet` | struct | 宏声明的所需权限集合（可序列化） |
| `PermissionGrant` | struct | 运行时授权对象，从 `PermissionSet` 构建 |

#### `PathScope` 变体

| 变体 | 说明 |
|------|------|
| `Exact(PathBuf)` | 仅允许操作该精确路径 |
| `Prefix(PathBuf)` | 允许操作该目录及其所有子路径 |
| `Any` | 不限制路径（最大权限） |

#### `Permission` 变体

| 变体 | 说明 |
|------|------|
| `InputSimulation` | 模拟键盘/鼠标输入 |
| `FileRead { scope }` | 文件读取，受 `PathScope` 限制 |
| `FileWrite { scope }` | 文件写入，受 `PathScope` 限制 |
| `Network` | 发起网络请求 |
| `RunCommand` | 执行外部命令/进程 |
| `ScriptExecution` | 在沙箱中运行脚本 |
| `ClipboardRead` | 读取剪贴板 |
| `ClipboardWrite` | 写入剪贴板 |
| `WindowInteraction` | **M2.3** 通过 UIA / PostMessage 与后台窗口交互 |
| `BrowserControl` | **M2.3** 通过 CDP / WebExt 控制浏览器标签 |
| `ForegroundTakeover` | **M2.3** 将目标窗口抬到前台（`Degrade` 降级策略所需） |

### 函数 / 方法

| 签名 | 说明 |
|------|------|
| `aggregate_from_configs(actions: &[ActionConfig]) -> PermissionSet` | **M1.4 新增**：静态分析扁平 ActionConfig 列表，返回所需权限的去重并集 |
| `aggregate_from_workflow(root: &WorkflowNode) -> PermissionSet` | **M2.5 新增**：深度优先遍历整个工作流树，聚合所有可达 Action 的权限；修复 `aggregate_from_configs` 漏掉工作流树内嵌套动作的问题，是保存时审批的权限来源 |
| `Permission::describe(&self) -> String` | **M2.5 新增**：渲染人类可读权限标签（含 `PathScope` 细化），供审批对话框与权限管理页显示 |
| `PermissionGrant::new(permissions: Vec<Permission>) -> Self` | 直接构建授权对象 |
| `PermissionGrant::from_set(set: &PermissionSet) -> Self` | 从 `PermissionSet` 转换（宏触发时调用） |
| `PermissionGrant::allows(&self, permission: &Permission) -> bool` | Action 执行前的权限检查 |

#### `aggregate_from_configs` — 权限聚合规则

| `ActionConfig` 变体 | 聚合的 `Permission` |
|---------------------|---------------------|
| `RunCommand` | `RunCommand` |
| `SimulateInput` | `InputSimulation` |
| `RunScript` | `ScriptExecution` |
| `HttpRequest` | `Network` |
| `Notify`、`SetVariable`、`Delay`、`Custom` | 无 |
| `Interact { target: BrowserTab, .. }` | `BrowserControl` |
| `Interact { target: 其他, .. }` | `WindowInteraction` |
| `Interact { on_no_background: Degrade, .. }` | +`ForegroundTakeover` |

重复出现的同类 Action 只计一次（`Vec::contains` 去重）。M2.5 将单动作的聚合逻辑提取为私有 `add_action_perms`，由 `aggregate_from_configs`（扁平列表）和 `aggregate_from_workflow`（经私有 `collect_workflow_perms` 递归遍历 `Seq`/`Parallel`/`If` 的 `then`+`otherwise`/`While`/`ForEach`/`Retry`/`Timeout` 的 body）共用。

#### `Permission::describe` — 权限标签渲染

返回简短英文标签；文件权限附带 `PathScope` 细化：`Exact(p)` → `exactly <p>`、`Prefix(p)` → `under <p>`、`Any` → `any path`（私有 `scope_label`）。例：`FileWrite { scope: Prefix("/tmp") }` → `"File write (under /tmp)"`。

## 依赖关系

依赖以下同 workspace 模块：
- [`domain`](domain.md) — `aggregate_from_configs` 接受 `&[ActionConfig]` 参数

## 设计说明

`Permission` 使用 `#[serde(tag = "kind")]`，序列化后 JSON 中有明确的 `"kind"` 字段，便于前端展示和用户审计。

`allows` 使用 `Vec::contains`，当前 `Permission` 的 `PartialEq` 实现依赖派生，因此 `FileRead { scope: Any }` 和 `FileRead { scope: Exact(...) }` 不相等——调用方需用相同的 `Permission` 值检查，不支持模糊匹配（如"检查是否有任何 FileRead 权限"）。这是 V1 的已知局限，V2 将引入分层权限查询 API。