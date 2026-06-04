# `schema` — ParamSchema：schema 驱动表单描述符

> **Crate**: `koakuma-core` · **文件**: `crates/core/src/schema.rs`
> **最后同步**: 2026-06-04 (M2.4：新增模块)

## 职责

`schema` 模块定义 `ParamSchema` 及其辅助类型，为可视化编辑器（§16.1）提供 schema 驱动的表单描述能力。每个 `TriggerConfig`、`ConstraintConfig`、`ActionConfig` 变体（以及未来的插件 provider）可暴露一份 `ParamSchema`，编辑器据此生成对应表单，而无需为每种类型手写 Slint 组件。

该模块**仅定义类型**，不包含任何渲染逻辑。V3 插件系统（§15.2）中，插件的 `plugin.toml` 也将使用相同 schema 格式声明参数。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `ParamType` | enum | 字段数据类型，决定编辑器渲染的 widget 种类 |
| `ParamField` | struct | 一个表单字段的完整描述（名称、标签、类型、是否必填） |
| `ParamSchema` | struct | 一个 config 变体的全部字段描述 |

#### `ParamType` 变体

| 变体 | widget | 说明 |
|------|--------|------|
| `Str` | 单行文本 | 普通字符串 |
| `Int` | 数字输入 | 整数 |
| `Bool` | 复选框 | 布尔切换 |
| `Enum(Vec<String>)` | 下拉选择 | 固定选项列表 |
| `Path` | 路径输入 | 文件系统路径（可选浏览按钮） |
| `Secret` | 密码框 | 敏感数据，显示遮蔽 |
| `Multiline` | 多行文本 | 适合脚本/消息正文 |
| `Duration` | 数字输入 | 毫秒时长（带单位提示） |
| `Json` | 代码编辑器 | 复杂/嵌套值的回退格式 |

#### `ParamField` 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | `String` | 序列化字段名（与 JSON/serde key 对应） |
| `label` | `String` | 人类可读的标签文本 |
| `ty` | `ParamType` | 字段类型 |
| `required` | `bool` | 是否必填 |

#### `ParamField` 方法

| 签名 | 说明 |
|------|------|
| `fn required(name, label, ty) -> Self` | 构造必填字段 |
| `fn optional(name, label, ty) -> Self` | 构造可选字段 |

#### `ParamSchema` 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `type_name` | `String` | serde `type` tag 值（如 `"Notify"`） |
| `display_name` | `String` | 类型下拉中显示的人类可读名称 |
| `fields` | `Vec<ParamField>` | 有序字段列表 |

#### `ParamSchema` 方法

| 签名 | 说明 |
|------|------|
| `fn new(type_name, display_name, fields) -> Self` | 构造 schema |

## 依赖关系

无外部依赖；纯数据类型模块。

## 设计说明

`ParamSchema` 是内置变体与未来插件 provider 共用的**通用表单描述格式**，通过该接口使插件组件成为编辑器一等公民而无需额外 Slint 代码（见 DESIGN.md §16.1）。

当前 M2.4 的编辑器对已有变体使用 JSON TextEdit 作为参数编辑器（而非完整的 schema 驱动表单）；`ParamSchema` 类型已就绪，等 V3 插件系统接入时可直接驱动渲染。
