# `state` — 状态存储抽象

> **Crate**: `koakuma-core` · **文件**: `crates/core/src/state.rs`
> **最后同步**: 2026-06-02

## 职责

`state` 模块定义 `StateStore` trait，抽象全局键值变量存储。引擎和 Action 通过此 trait 读写跨宏共享的运行时状态，而不依赖具体实现（内存、SQLite、Redis 等）。

测试使用 `koakuma-store::InMemoryStateStore`，生产部署可替换为持久化实现。

## 公开 API

### Trait

```rust
pub trait StateStore: Send + Sync {
    fn get(&self, key: &str) -> Option<Value>;
    fn set(&self, key: &str, value: Value);
    fn increment(&self, key: &str, by: i64) -> i64;
    fn remove(&self, key: &str);
    fn snapshot(&self) -> BTreeMap<String, Value>;
}
```

#### 方法说明

| 方法 | 说明 |
|------|------|
| `get` | 按键读取变量，不存在时返回 `None` |
| `set` | 写入或更新变量 |
| `increment` | 原子自增，返回操作后的新值；键不存在时从 0 开始 |
| `remove` | 删除变量 |
| `snapshot` | 返回全量快照，供 UI 变量监视面板使用 |

## 依赖关系

依赖以下同 workspace 模块：
- [`value`](value.md) — `Value`

## 设计说明

`increment` 的原子语义（由实现者保证）用于实现"N 次触发内计数"等频率限制模式，避免 get-modify-set 的竞态。

`set` 签名接受 `&self`（而非 `&mut self`），要求实现者内部使用 `Mutex` / `RwLock` 或原子操作，使 `Arc<dyn StateStore>` 可跨线程共享写入。