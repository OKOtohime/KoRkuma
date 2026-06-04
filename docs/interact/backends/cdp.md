# `backends/cdp` — Chrome DevTools Protocol 浏览器后端

> **Crate**: `koakuma-interact` · **文件**: `crates/interact/src/backends/cdp.rs`
> **最后同步**: 2026-06-04 (M2.3 MVP)

## 职责

通过 Chrome DevTools Protocol 操作浏览器标签，实现 `Tier::Background`——无需切换标签页即可执行 JS 操作。

- `resolve`：HTTP GET `http://127.0.0.1:{port}/json`，过滤 `type="page"` 且 URL 包含 `url_pattern` 的标签
- `invoke`：建立 WebSocket 连接 → 发送 `Runtime.evaluate` CDP 命令（JS 操作）→ 等待 `id` 匹配的响应

## 支持的操作（映射为 JS）

| UiOp | 生成的 JS |
|------|-----------|
| `Click { node }` | `document.querySelector(node).click()` |
| `SetText { node, text }` | `querySelector.value = text; dispatchEvent('input')` |
| `Focus { node: Some }` | `document.querySelector(node).focus()` |
| `Focus { node: None }` | `window.focus()` |
| `ReadValue { node }` | `document.querySelector(node).value` |
| `SendKeys` | JS 注释（不完整支持，需 `Input.dispatchKeyEvent` CDP 命令扩展） |

## 依赖关系

- `tokio` (`net`, `io-util`) — 原始 TCP HTTP GET（无 reqwest）
- `tokio-tungstenite` — WebSocket CDP 通信
- `serde_json` — CDP JSON-RPC 编解码

## 使用前提

浏览器需以 `--remote-debugging-port=9222` 启动（或指定其他端口）。

## 设计说明

tab listing 使用原始 TCP + HTTP/1.1 GET 实现，避免引入 `reqwest` 依赖，代码约 30 行。每次 `invoke` 建立独立 WebSocket 连接（短连接），适合低频自动化操作；高频场景可扩展为连接池。

`CDP_ID` 全局原子计数确保请求 ID 唯一，即使并发调用也能正确匹配响应。
