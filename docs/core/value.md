# `value` — 核心动态值类型

> **Crate**: `koakuma-core` · **文件**: `crates/core/src/value.rs`
> **最后同步**: 2026-06-02

## 职责

`value` 模块定义贯穿整个管道的通用数据载体 `Value`。它解耦了领域逻辑与具体序列化格式（`serde_json`）和脚本运行时（rhai），使各层可以用统一类型传递数据，而无需在各处引入外部类型。

`Value` 被用作事件 payload、状态变量值、Action 参数模板，以及 Constraint DSL 的求值结果。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `Value` | enum | 动态多态值，涵盖 JSON 兼容的所有原生类型 |

#### `Value` 变体

| 变体 | 说明 |
|------|------|
| `Null` | 空值，缺省状态 |
| `Bool(bool)` | 布尔 |
| `Int(i64)` | 64 位有符号整数 |
| `Float(f64)` | 64 位浮点 |
| `Str(String)` | UTF-8 字符串 |
| `List(Vec<Value>)` | 同质/异质列表 |
| `Map(BTreeMap<String, Value>)` | 有序键值对（`BTreeMap` 保证确定性序列化） |

`Value` 派生 `Clone`、`Debug`、`PartialEq`，并通过 `#[serde(untagged)]` 与 JSON 无缝互转。

## 依赖关系

该模块无任何 `use crate::` 内部依赖，是依赖图的叶节点。

## 设计说明

使用 `BTreeMap`（而非 `HashMap`）是为了确保 `Map` 变体的序列化顺序确定，方便快照比对和测试断言。

`#[serde(untagged)]` 使 JSON 反序列化能自动推断变体，与标准 JSON 格式完全兼容，但要求各变体在 JSON 层面可区分（布尔/整数/浮点/字符串/数组/对象均可区分）。