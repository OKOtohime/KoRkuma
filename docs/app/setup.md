# `setup` — 平台/渲染后端初始化

> **Crate**: `korkuma-app` · **文件**: `crates/app/src/setup.rs`
> **最后同步**: 2026-06-04

## 职责

在 Slint 平台初始化之前执行所有必须就绪的环境准备工作，包括区域语言规范化与渲染后端探测。所有函数须在 `MainWindow::new()` 之前调用，否则 `SLINT_BACKEND` / `LANG` 环境变量的修改对 Slint 无效。

## 公开 API

### 函数 / 方法

| 签名 | 说明 |
|------|------|
| `normalize_lang_for_slint()` | 将 `LANG` 覆写为 `en_US.UTF-8`，当检测到 CJK / SEA 语系时；Slint 的 ICU4X 不含这些语言的 ML 分词模型 |
| `system_locale() -> String` | 读取 `LANG` / `LANGUAGE` / `LC_ALL`，返回 BCP-47 格式 locale 标签（`zh_CN` → `zh-CN`） |
| `select_backend() -> &'static str` | 探测硬件 GL，设置 `SLINT_BACKEND`；若已有 `SLINT_BACKEND` 环境变量则原样返回 `"custom (SLINT_BACKEND env)"` |

### 私有辅助

| 签名 | 说明 |
|------|------|
| `hardware_gl_available() -> bool` | 检查 `LIBGL_ALWAYS_SOFTWARE=1` 与 `GALLIUM_DRIVER ∈ {llvmpipe,softpipe,swr}`，委托 `platform_has_hw_gl()` |
| `platform_has_hw_gl() -> bool` | **Linux**：`/dev/dri/renderD128` 或 `card0` 存在且有 `DISPLAY`/`WAYLAND_DISPLAY`；**Windows**：`true`；**其他**：`true` |

## 依赖关系

无依赖其他 crate 模块；仅使用标准库 `std::env` 和 `std::path::Path`。

## 设计说明

**必须早于 Slint 初始化**：`set_var` 调用带 `// SAFETY` 注释，说明此时 Slint 平台尚未提交，也尚未生成子线程，满足 `set_var` 的线程安全前提。

**语言探测范围**：目前仅屏蔽需要 ML 分词器的语系（`ja`/`zh`/`th`/`km`/`lo`/`my`），其他语言直接放行；未来可扩展为查询 Slint 已绑定的翻译列表。