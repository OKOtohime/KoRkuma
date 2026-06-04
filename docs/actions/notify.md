# `notify` — 桌面通知

> **Crate**: `korkuma-actions` · **文件**: `crates/actions/src/notify.rs`
> **最后同步**: 2026-06-02

## 职责

实现 `ActionConfig::Notify`，通过 `notify-rust` 显示系统桌面通知。

跨平台：Linux 使用 D-Bus/zbus（纯 Rust，无需 `libdbus-dev`）；Windows 使用 WinRT `ToastNotification`；macOS 使用原生用户通知 API。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `NotifyAction` | struct | 持有 `title` 和 `body` 字符串 |

### 函数 / 方法

| 签名 | 说明 |
|------|------|
| `fn build(c: &ActionConfig) -> Option<Box<dyn Action>>` | 工厂：从 `ActionConfig::Notify` 构建 |

#### `execute` 行为

调用：
```rust
notify_rust::Notification::new()
    .summary(&self.title)
    .body(&self.body)
    .show()?
```

`show()` 为同步调用，等待通知发送完成（不等待用户交互）。失败时返回 `ActionError::Failed`。

## 依赖关系

- `korkuma_core::domain::ActionConfig` — 配置入参
- 外部：`notify-rust 4`

## 设计说明

`NotifyAction` 不需要任何权限（通知属于非破坏性操作），`required_permissions` 返回空集。

在 Windows 上，`notify-rust` 使用 `winrt-notification`，可能要求应用注册 App User Model ID 才能在操作中心显示图标。M1.2 不做此注册；通知仍会弹出但无自定义图标。M1.3 可在 `app` 初始化时注册 AUMID。
