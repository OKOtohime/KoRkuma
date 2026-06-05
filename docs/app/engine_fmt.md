# `engine_fmt` — 引擎事件日志格式化

> **Crate**: `korkuma-app` · **文件**: `crates/app/src/engine_fmt.rs`
> **最后同步**: 2026-06-04

## 职责

将 `korkuma_core::engine::EngineEvent` 枚举转换为供日志面板展示的单行字符串，集中管理所有日志前缀格式。

## 公开 API

### 函数

| 签名 | 说明 |
|------|------|
| `format_engine_event(ev: &EngineEvent) -> String` | 匹配所有 `EngineEvent` 变体，返回带前缀的日志行 |

### 格式规范

| 变体 | 输出格式 |
|------|---------|
| `MacroFired { name, id, .. }` | `[FIRED] "name" (id前8位)` |
| `ActionLog { level, action, message, .. }` | `[ERR/WRN/INF/DBG] [action] message` |
| `VariableChanged { key, value }` | `[VAR] key = value` |
| `Error { macro_id: Some(id), message }` | `[ERR] (id前8位) message` |
| `Error { macro_id: None, message }` | `[ERR] message` |

## 依赖关系

- [`korkuma_core::engine`](../core/engine.md) — `EngineEvent`、`LogLevel`

## 设计说明

**id 截断**：UUID 只取前 8 位（`.to_string()[..8]`），在日志行宽度有限的情况下可识别性已足够，同时保持行长可控。