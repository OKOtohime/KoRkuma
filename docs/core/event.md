# `event` — 事件类型与管道入口

> **Crate**: `korkuma-core` · **文件**: `crates/core/src/event.rs`
> **最后同步**: 2026-06-02

## 职责

`event` 模块定义 `Event` 和 `EventKind`，是整个自动化管道的起点。`HookProvider` 产生 `Event` 并写入 `EventSink`，`EventRouter` 以 `EventKind` 为索引键将事件分发给订阅的宏，`TriggerSpec` 再做细粒度匹配。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `EventKind` | enum | 事件类别，用作路由索引键（`Hash + Eq`） |
| `Event` | struct | 携带完整信息的具体事件 |

#### `EventKind` 变体

| 变体 | 触发来源 |
|------|----------|
| `Hotkey` | 键盘快捷键 hook |
| `WindowFocus` | 窗口焦点变更 |
| `Process` | 进程启动/停止 |
| `Timer` | 定时调度（cron） |
| `FileChange` | 文件系统变更 |
| `Manual` | 用户手动触发 |
| `Custom` | 第三方 HookProvider |

#### `Event` 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `kind` | `EventKind` | 路由索引键 |
| `source` | `String` | 产生事件的 provider 标识符 |
| `timestamp` | `SystemTime` | 事件发生的 UTC 时刻，供 TimeRange 约束使用 |
| `payload` | `Value` | 事件携带的附加数据（按键名、窗口标题、进程名等） |

## 依赖关系

依赖以下同 workspace 模块：
- [`value`](value.md) — `payload` 字段的类型

## 设计说明

`EventKind` 实现了 `Copy + Hash + Eq`，可直接作为 `HashMap` 键使用，是 `EventRouter` O(1) 分发的基础。

`payload` 使用 `Value`（而非 `serde_json::Value`）以避免 `korkuma-core` 在类型层面硬依赖 serde_json 的具体类型。