# `simulate_input` — 键鼠输入模拟

> **Crate**: `koakuma-actions` · **文件**: `crates/actions/src/simulate_input.rs`
> **最后同步**: 2026-06-02
> **平台实现**: Windows（`#[cfg(target_os = "windows")]`）；其他平台返回错误

## 职责

实现 `ActionConfig::SimulateInput`，将 `Vec<InputEvent>` 序列注入操作系统输入流。在管道 Action 层中负责"做什么 → 操控当前用户界面"的场景，如自动填表、快捷键触发等。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `SimulateInputAction` | struct | 持有 `sequence: Vec<InputEvent>` |

### 函数 / 方法

| 签名 | 说明 |
|------|------|
| `fn build(c: &ActionConfig) -> Option<Box<dyn Action>>` | 工厂：从 `ActionConfig::SimulateInput` 构建 |

#### `execute` 行为

遍历 `sequence`，逐个调用 `platform_impl::send_one(event)`。任一事件失败则中止并返回 `ActionError::Failed`。

## InputEvent → Win32 映射（Windows）

| `InputEvent` 变体 | Win32 调用 | 说明 |
|---|---|---|
| `KeyPress { key }` | `SendInput(key_input(vk, 0))` | Key down，VK 由 `name_to_vk` 查表 |
| `KeyRelease { key }` | `SendInput(key_input(vk, KEYEVENTF_KEYUP))` | Key up |
| `Text { text }` | `SendInput([unicode_down, unicode_up] * chars)` | `KEYEVENTF_UNICODE`，无需 VK 查表，支持任意 Unicode 字符 |
| `MouseMove { x, y }` | `SendInput(mouse_input(dx, dy, MOUSEEVENTF_MOVE\|ABSOLUTE))` | 坐标归一化到 [0, 65535] |
| `MouseClick { button }` | `SendInput([down, up])` | `left`/`right`/`middle` |

坐标归一化公式：`dx = x / GetSystemMetrics(SM_CXSCREEN) * 65535`。

#### `name_to_vk` 键名映射

支持：`"A"`–`"Z"`、`"0"`–`"9"`（直接用 ASCII 值）、`"F1"`–`"F12"`、`"Enter"`、`"Escape"`、`"Ctrl"`/`"LCtrl"`/`"RCtrl"`、`"Shift"`/`"LShift"`/`"RShift"`、`"Alt"`/`"LAlt"`/`"RAlt"`、`"Win"`/`"LWin"`/`"RWin"`、方向键、`"Delete"`、`"Insert"` 等。未知键名返回 VK 码 0（无操作）。

## 依赖关系

- `koakuma_core::domain::InputEvent` — 输入序列
- `koakuma_core::permission` — 要求 `Permission::InputSimulation`
- 外部（仅 Windows）：`windows::Win32::UI::Input::KeyboardAndMouse`（`SendInput`）、`windows::Win32::UI::WindowsAndMessaging`（`GetSystemMetrics`）

## 设计说明

**UIPI 限制**：在 Windows Vista+ 中，低权限进程无法向高权限窗口注入输入（User Interface Privilege Isolation）。若 `SendInput` 返回的成功数小于发送数，`execute` 返回 `ActionError::Failed("UIPI block?")`。解决方案是以 UAC 提权运行 Koakuma，或目标窗口同权限级别。

**无延迟**：事件序列之间无内置延迟。若应用需要响应时间（如等待弹出菜单），在序列间插入 `ActionConfig::Delay` Action。

**Text vs KeyPress**：`Text` 使用 `KEYEVENTF_UNICODE` 绕过键盘布局，适合输入任意文字；`KeyPress`/`KeyRelease` 使用 VK 码，适合快捷键组合。两者可混合使用。
