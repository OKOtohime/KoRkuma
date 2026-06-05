# `watcher` — macros.json 热重载监听

> **Crate**: `korkuma-app` · **文件**: `crates/app/src/watcher.rs`
> **最后同步**: 2026-06-04

## 职责

在独立后台线程中监听当前目录的文件系统事件，当检测到 `macros.json` 被外部写入时，对本地宏列表与引擎状态做增量同步，并通过 `upgrade_in_event_loop` 刷新 UI。

## 公开 API

### 函数

| 签名 | 说明 |
|------|------|
| `spawn_file_watcher(local_macros, engine_sender, ui_weak, suppress_reload) -> notify::RecommendedWatcher` | 创建 `notify` watcher，监听当前目录（非递归），启动监听线程；返回 watcher 句柄（须由调用方持有，否则 watcher 被 drop 后停止） |

### 参数

| 参数 | 类型 | 说明 |
|------|------|------|
| `local_macros` | `Arc<Mutex<Vec<Macro>>>` | 主线程共享的宏列表，同步后原地更新 |
| `engine_sender` | `crossbeam_channel::Sender<EngineCommand>` | 向引擎发送增量 Add/Update/Delete |
| `ui_weak` | `slint::Weak<MainWindow>` | 弱引用 UI，通过 `upgrade_in_event_loop` 在主线程刷新模型 |
| `suppress_reload` | `Arc<AtomicBool>` | 由 `persist()` 设置，为 `true` 时跳过本次重载（避免 UI 操作触发冗余刷新） |

## 增量同步逻辑

```
old_ids = set(local_macros.id)
new_ids = set(loaded.id)

for m in new_macros:
    if m.id in old_ids → EngineCommand::UpdateMacro
    else               → EngineCommand::AddMacro

for id in old_ids - new_ids → EngineCommand::DeleteMacro

local_macros ← new_macros
```

## 依赖关系

- [`korkuma_core::domain`](../core/domain.md) — `Macro`
- [`korkuma_core::engine`](../core/engine.md) — `EngineCommand`
- [`korkuma_store`](../store/README.md) — `load_macros`
- [`crate::model`](model.md) — `reload_ui_model`
- `crate::{LogEntry, MainWindow, MACROS_PATH}` — Slint 生成类型与路径常量
- `notify` — 文件系统监听
- `slint::{Model, VecModel}` — 日志行写入

## 设计说明

**watcher 句柄必须持有**：`notify::RecommendedWatcher` 被 drop 时自动停止监听；`main.rs` 用 `_watcher` 绑定保证其生命周期覆盖整个 UI 事件循环。

**suppress_reload 语义**：`swap(false, Relaxed)` — 读取并清除，保证每次 `persist()` 只静默一次 watcher 触发，而非永久屏蔽。

**过滤条件**：仅处理 `file_name == "macros.json"` 且 `EventKind` 为 `Create`/`Modify` 的事件，忽略 `.tmp` 中间文件和目录变更。