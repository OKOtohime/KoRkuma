# `main` — 程序入口与组件编排

> **Crate**: `korkuma-app` · **文件**: `crates/app/src/main.rs`
> **最后同步**: 2026-06-05 (M2.5：变量监视器 Timer)

## 职责

`main.rs` 是精简的编排入口（~150 行），仅负责将各模块串联成可运行系统；所有具体逻辑已提取到独立模块：

| 职责 | 模块 |
|------|------|
| 区域语言规范化、渲染后端探测 | [`setup`](setup.md) |
| 触发器格式化 / UI ↔ 领域转换 | [`trigger`](trigger.md) |
| UI 模型刷新与编辑器回填 | [`model`](model.md) |
| 引擎事件日志格式化 | [`engine_fmt`](engine_fmt.md) |
| macros.json 热重载监听 | [`watcher`](watcher.md) |
| 全部 `on_*` 回调注册 + 持久化 | [`callbacks`](callbacks.md) |
| 约束/工作流树扁平化与编辑 | [`tree_model`](tree_model.md) |

`slint::include_modules!()` 宏必须保留在 crate root（`main.rs`），生成的 `MainWindow`、`MacroItem`、`LogEntry` 等类型通过 `use crate::TypeName` 在子模块中引用。`MACROS_PATH` 也声明为 `pub const` 供子模块访问。

## 公开 API

无公开类型；这是二进制 crate 的入口点。

## 启动顺序

```
main()
  ├─ setup::normalize_lang_for_slint()
  ├─ setup::select_backend()            ← 设置 SLINT_BACKEND
  ├─ Registry::with_builtins()
  │   + register_trigger_specs / register_actions / register_script_*
  ├─ Arc<InMemoryStateStore::new()>
  ├─ MainWindow::new()
  │   └─ femtovg 失败 → retry winit-software
  ├─ slint::select_bundled_translation(setup::system_locale())
  ├─ VecModel × 5 初始化 + set_* 绑定到 UI
  ├─ start_engine(registry, store, engine_fmt::format_engine_event)
  ├─ #[cfg(windows)] start_hooks(event_sink)
  ├─ load_macros(MACROS_PATH) → AddMacro × N
  ├─ watcher::spawn_file_watcher(...)   ← 热重载线程
  ├─ callbacks::wire_callbacks(...)     ← 注册所有 on_* 处理器
  ├─ ui.run()                           ← 阻塞至关闭
  ├─ #[cfg(windows)] providers.stop()
  └─ drop(engine)                       ← 发送 Shutdown，join 引擎线程
```

## 关键私有函数

| 函数 | 说明 |
|------|------|
| `start_hooks(event_sink)` *(仅 Windows)* | 启动 `HotkeyProvider`、`WindowFocusProvider`、`ProcessProvider` 并返回列表 |

## 常量

| 名称 | 值 | 说明 |
|------|-----|------|
| `pub const MACROS_PATH: &str` | `"macros.json"` | 宏配置文件路径；子模块通过 `crate::MACROS_PATH` 引用 |

## 依赖关系

- [`setup`](setup.md) — 平台/渲染后端初始化
- [`engine_fmt`](engine_fmt.md) — 引擎事件格式化
- [`watcher`](watcher.md) — 文件热重载
- [`callbacks`](callbacks.md) — UI 回调注册
- [`tree_model`](tree_model.md) — 约束/工作流树操作
- [`korkuma_core::engine_loop`](../core/engine_loop.md) — `start_engine`、`EngineHandle`
- [`korkuma_core::engine`](../core/engine.md) — `EngineCommand`
- [`korkuma_core::domain`](../core/domain.md) — `Macro`
- [`korkuma_core::registry`](../core/registry.md) — `Registry::with_builtins`
- [`korkuma_core::state`](../core/state.md) — `StateStore` trait
- [`korkuma_store`](../store/README.md) — `InMemoryStateStore`、`load_macros`
- [`korkuma_hooks`](../hooks/README.md) — `register_trigger_specs`（+ Windows providers）
- [`korkuma_actions`](../actions/README.md) — `register_all`
- [`korkuma_script`](../script/README.md) — `register_actions`、`register_constraints`
- `slint::include_modules!()` — 引入 Slint 生成类型

## 设计说明

**渲染后端回退**：femtovg 初始化失败时，在引擎线程尚未启动的窗口期内切换为 software 渲染重试，避免因 GPU 误判导致启动失败。

**graceful shutdown 顺序**：先 stop hooks → drop engine（发送 Shutdown + join）。确保引擎退出前不再有新事件入队。

**`slint::include_modules!()` 位置约束**：Slint 代码生成宏必须在 crate root 展开，子模块只能通过 `use crate::` 访问生成类型，不能在子模块内 include。

**M2.5 变量监视器**：`main` 额外创建 `permission_model` / `var_model` 并绑定到 UI。变量监视器由一个 `slint::Timer`（`TimerMode::Repeated`，1s）在 UI 线程周期性调用 `StateStore::snapshot()`，将快照映射为 `VarRow`（值经 `serde_json::to_string`）并重建模型；Timer 句柄需存活至 `ui.run()` 返回（`drop(var_timer)` 显式释放）。直接复用主线程持有的 `store` Arc，无需引擎往返。