# `backend` — 能力分级、目标句柄与后端 trait

> **Crate**: `koakuma-interact` · **文件**: `crates/interact/src/backend.rs`
> **最后同步**: 2026-06-04 (M2.3 初始实现)

## 职责

定义后台交互系统的三个核心抽象：

1. **`Tier`** — 能力分级，描述后端对给定目标能达到的最高层级（Background > ForegroundSynthetic > Unsupported）。
2. **`ResolvedTarget`** — 已解析的目标句柄，持有平台内部数据（HWND、CDP tab info、或测试 stub）。
3. **`InteractionBackend` trait** — 所有平台/协议后端的统一接口，包含 resolve / capability / invoke / enumerate 四个方法。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `Tier` | enum | 能力分级（`Unsupported=0`、`ForegroundSynthetic=1`、`Background=2`），实现 `PartialOrd` 便于比较 |
| `ResolvedTarget` | struct | 已解析目标；`backend_id: &'static str` 标明来源后端；`display_name` 供 UI / 日志使用 |
| `UiNode` | struct | UI 元素树节点（`path`、`name`、`control_type`），由 `enumerate` 返回 |

`ResolvedTarget::inner` 为 `pub(crate)` 的 `TargetInner` 枚举，持有：
- `WindowHandle { hwnd: isize }`（仅 Windows）
- `BrowserTab { port, target_id, ws_url, url }`
- `Stub { id, tier }`（测试用）

### Trait

```rust
#[async_trait]
pub trait InteractionBackend: Send + Sync {
    fn id(&self) -> &'static str;
    async fn resolve(&self, sel: &TargetSelector) -> Result<Vec<ResolvedTarget>, BackendError>;
    fn capability(&self, t: &ResolvedTarget) -> Tier;  // 快速同步检查，不做 I/O
    async fn invoke(&self, t: &ResolvedTarget, op: &UiOp) -> Result<(), BackendError>;
    async fn enumerate(&self, t: &ResolvedTarget) -> Result<Vec<UiNode>, BackendError>;
}
```

## 依赖关系

- [`koakuma_core::domain`](../core/domain.md) — `TargetSelector`、`UiOp`、`UiPath`
- [`error`](error.md) — `BackendError`

## 设计说明

`capability` 是同步方法：协商逻辑需快速扫描所有后端，不应发起 I/O。实现方应根据 `ResolvedTarget::inner` 类型直接判断（UIA 后端对 `WindowHandle` 乐观返回 `Background`；invoke 失败时自然产生错误）。

`ResolvedTarget` 不实现 `Clone`，因 `HWND`（指针）的跨线程传递需要显式管理。使用 `isize` 存储 HWND 值，在 `spawn_blocking` 闭包中重建 `HWND(val as _)`。
