# `lib` — korkuma-hooks crate 根

> **Crate**: `korkuma-hooks` · **文件**: `crates/hooks/src/lib.rs`
> **最后同步**: 2026-06-02

## 职责

crate 根模块，声明子模块、通过条件 `pub use` 导出平台 provider，并暴露 `register_trigger_specs` 供 `app` 在启动时注册 TriggerSpec 工厂。

## 公开 API

### 类型（re-export，仅 Windows）

| 类型 | 来源 | 说明 |
|------|------|------|
| `HotkeyProvider` | `platform_windows` | 低级键盘 hook |
| `WindowFocusProvider` | `platform_windows` | 前台窗口变更 |
| `ProcessProvider` | `platform_windows` | 进程启停轮询 |

### 类型（re-export，跨平台）

| 类型 | 来源 | 说明 |
|------|------|------|
| `HotkeyTriggerSpec` | `trigger_spec` | 热键事件细粒度匹配 |
| `WindowFocusTriggerSpec` | `trigger_spec` | 窗口标题匹配 |
| `ProcessTriggerSpec` | `trigger_spec` | 进程名 + 事件类型匹配 |

### 函数 / 方法

| 签名 | 说明 |
|------|------|
| `fn register_trigger_specs(registry: &mut Registry)` | 注册三个 TriggerSpec 工厂到 `Registry` |

## 依赖关系

依赖以下同 workspace 模块：
- [`trigger_spec`](trigger_spec.md) — 工厂函数
- [`platform_windows`](platform_windows.md) — Windows provider（条件编译）
- `korkuma_core::registry::Registry` — 注册入参

## 设计说明

平台条件编译通过 `#[cfg(target_os = "windows")]` 隔离。未来 M2.1 引入 Linux provider 时，只需在此增加 `#[cfg(target_os = "linux")]` 块，调用方代码无需修改。
