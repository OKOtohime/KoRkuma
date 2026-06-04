# `main` — 程序入口与组件组装

> **Crate**: `koakuma-app` · **文件**: `crates/app/src/main.rs`
> **最后同步**: 2026-06-02

## 职责

程序启动点，负责将所有 crate 组装为可运行系统，并承担以下职责：

1. 渲染后端自动探测（硬件 OpenGL vs 软件渲染回退）
2. 构建 `Registry`（内置 + hooks 触发器 spec + actions + script actions/constraints）
3. 创建 `StateStore`
4. 初始化 Slint UI 与 `VecModel<MacroItem>` / `VecModel<LogEntry>`
5. 启动引擎线程（`start_engine`），通过 `upgrade_in_event_loop` 将 `EngineEvent` 推入 UI 日志
6. 启动 Windows 平台 hook provider（条件编译）
7. 从 `macros.json` 加载宏配置并向引擎发送 `AddMacro`
8. 启动 `macros.json` 热重载 watcher，增量同步引擎与 UI 模型
9. 绑定全部 UI 回调（add / delete / toggle / trigger / select）
10. 运行 Slint 事件循环并优雅关机

## 公开 API

无公开类型；这是二进制 crate 的入口点。所有函数均为模块私有。

## 启动顺序

```
main()
  ├─ select_backend()              ← 设置 SLINT_BACKEND 环境变量
  ├─ Registry::with_builtins()
  │   + register_trigger_specs(&mut registry)   // koakuma_hooks
  │   + register_actions(&mut registry)         // koakuma_actions
  │   + register_script_actions(&mut registry)  // koakuma_script
  │   + register_script_constraints(&mut registry)
  ├─ Arc<InMemoryStateStore::new()>
  ├─ MainWindow::new()
  │   └─ femtovg init failure → retry with winit-software
  ├─ start_engine(registry, store, on_event_callback)
  │   └─ EngineHandle + EventSink
  ├─ #[cfg(windows)] start_hooks(event_sink)
  ├─ load_macros(MACROS_PATH) → AddMacro × N
  ├─ spawn_file_watcher(...)      ← hot-reload 线程
  ├─ wire callbacks               ← on_add/delete/toggle/trigger/select
  ├─ ui.run()                     ← 阻塞至关闭
  ├─ #[cfg(windows)] providers.stop()
  └─ drop(engine)                 ← sends Shutdown + joins
```

## 关键私有函数

| 函数 | 说明 |
|------|------|
| `select_backend() -> &'static str` | 探测硬件 GL 可用性，设置 `SLINT_BACKEND`；尊重已有环境变量 |
| `hardware_gl_available() -> bool` | 检查 `LIBGL_ALWAYS_SOFTWARE` / `GALLIUM_DRIVER`，委托 `platform_has_hw_gl()` |
| `platform_has_hw_gl() -> bool` | Linux：DRI 节点 + 显示服务器；Windows：乐观 `true`；其他：`true` |
| `spawn_file_watcher(...)` | 启动 `notify` 监听线程，对 `macros.json` 的外部写入做增量 engine diff |
| `reload_ui_model(ui, macros)` | 主线程：清空并重建 `VecModel<MacroItem>`，同步 editor 选中行 |
| `persist(macros, suppress_reload)` | 原子写 `macros.json`（`koakuma_store::save_macros`），写前设置 `suppress_reload` flag 避免 watcher 触发冗余刷新 |
| `refresh_editor(ui, macros, idx)` | 将 `macros[idx]` 的 triggers/constraints/actions 序列化为 JSON 填入 3-tab 编辑器 |
| `format_engine_event(ev)` | 将 `EngineEvent` 格式化为短日志行（`[FIRED]`、`[ERR]`、`[VAR]` 等前缀） |
| `create_default_macro()` | 创建带 Manual 触发器 + Notify action 的默认宏，`aggregate_from_configs` 自动填充权限 |
| `start_hooks(event_sink)` *(仅 Windows)* | 启动 `HotkeyProvider`、`WindowFocusProvider`、`ProcessProvider` 并返回列表 |

## 热重载机制

`spawn_file_watcher` 在独立线程中运行 `notify::recommended_watcher`，监听当前目录（非递归）。触发条件：

- 路径 basename 恰好为 `macros.json`（过滤 `.tmp` 临时文件）
- 事件类型为 `Create` 或 `Modify`（包括原子写的 rename-to）
- `suppress_reload` flag 为 `false`（排除 `persist()` 自身写入）

热重载时对 id 集合做 diff：新增 → `AddMacro`，更新 → `UpdateMacro`，消失 → `DeleteMacro`。UI 模型通过 `upgrade_in_event_loop` 在主线程重建。

## `on_event` 回调

EngineEvent 通过闭包捕获 `ui_weak`，在 `upgrade_in_event_loop` 中插入 `VecModel<LogEntry>`（最多保留 500 条，FIFO 淘汰）：

| EngineEvent | 日志前缀 |
|---|---|
| `MacroFired { name, id, .. }` | `[FIRED] "name" (id前8位)` |
| `ActionLog { level, action, message, .. }` | `[ERR/WRN/INF/DBG] [action] message` |
| `VariableChanged { key, value }` | `[VAR] key = value` |
| `Error { macro_id, message }` | `[ERR] (id前8位) message` |

## macros.json 格式

```json
[
  {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "name": "Ctrl+F1 通知",
    "description": "",
    "enabled": true,
    "category": null,
    "triggers": [{"type": "Hotkey", "keys": [{"modifiers": ["Ctrl"], "key": "F1"}]}],
    "constraints": {"op": "Always"},
    "actions": [{"type": "Notify", "title": "Koakuma", "body": "热键触发！"}],
    "granted_permissions": []
  }
]
```

## 依赖关系

依赖以下同 workspace 模块：

- [`koakuma_core::engine_loop`](../core/engine_loop.md) — `start_engine`、`EngineHandle`
- [`koakuma_core::engine`](../core/engine.md) — `EngineCommand`、`EngineEvent`、`LogLevel`
- [`koakuma_core::domain`](../core/domain.md) — `Macro`、`TriggerConfig`、`ActionConfig`、`ConstraintExpr`
- [`koakuma_core::permission`](../core/permission.md) — `aggregate_from_configs`
- [`koakuma_core::registry`](../core/registry.md) — `Registry::with_builtins`
- [`koakuma_core::state`](../core/state.md) — `StateStore` trait
- [`koakuma_store`](../store/README.md) — `InMemoryStateStore`、`load_macros`、`save_macros`
- [`koakuma_hooks`](../hooks/README.md) — `register_trigger_specs`（+ Windows: `HotkeyProvider`、`WindowFocusProvider`、`ProcessProvider`）
- [`koakuma_actions`](../actions/README.md) — `register_all`
- [`koakuma_script`](../script/README.md) — `register_actions`、`register_constraints`
- `slint::include_modules!()` — 引入 `MainWindow`、`MacroItem`、`LogEntry`
- `notify` — 文件系统事件监听（热重载）
- `crossbeam_channel` — engine sender 跨线程传递
- `uuid` — 新宏 UUID 生成

## 设计说明

**渲染后端回退**：`select_backend` 在任何 Slint 初始化之前调用（设置 env var 比 Slint API 更早）。若 femtovg 初始化失败（GPU 不可用但探测误判），`MainWindow::new()` 的 match arm 捕获错误，在 engine 线程尚未启动时安全地切换为 software 渲染重试。

**suppress_reload 原子标志**：`persist()` 在写文件前设 `true`；watcher 线程读到 true 时 `swap(false)` 并跳过重载。这确保用户在 UI 内的任何宏操作不触发多余的全量重建。

**graceful shutdown 顺序**：先 stop hooks → hook 线程退出后 `EventSink` 不再产生新事件 → drop EngineHandle → 引擎收到 Shutdown → join。这确保在引擎退出前不再有新事件进入队列。

**granted_permissions 自动填充**：`create_default_macro` 调用 `aggregate_from_configs(&actions)` 而非手动填写，保证新建宏的权限与其 action 列表始终一致。
