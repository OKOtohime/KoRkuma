# `lib` — InMemoryStateStore + JSON 宏持久化

> **Crate**: `koakuma-store` · **文件**: `crates/store/src/lib.rs`
> **最后同步**: 2026-06-02

## 职责

`koakuma-store` 承担两个互相独立的职责：

1. **运行时状态存储** — `InMemoryStateStore` 实现 `koakuma_core::state::StateStore` trait，以 `Mutex<BTreeMap>` 为底层容器，提供线程安全的键值读写，供引擎线程中的 Action 和 Constraint 共享全局变量。

2. **宏配置持久化** — `load_macros` 和 `save_macros` 提供 `macros.json` 的原子读写：保存时先写 `.tmp` 再 rename，保证崩溃不会留下损坏的配置文件；读取时对"文件不存在"返回空列表而非错误，简化调用端逻辑。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `InMemoryStateStore` | struct | `StateStore` 的内存实现 |
| `StoreError` | enum | JSON 持久化操作的错误类型 |

#### `StoreError` 变体

| 变体 | 说明 |
|------|------|
| `Io(std::io::Error)` | 文件系统操作失败（路径不可写、权限等） |
| `Json(serde_json::Error)` | JSON 序列化或反序列化失败 |

### 函数 / 方法

| 签名 | 说明 |
|------|------|
| `load_macros(path: &Path) -> Result<Vec<Macro>, StoreError>` | 从 JSON 文件加载宏列表；文件不存在时返回 `Ok(vec![])` |
| `save_macros(path: &Path, macros: &[Macro]) -> Result<(), StoreError>` | 原子写入宏列表到 JSON 文件 |
| `InMemoryStateStore::new() -> Self` | 创建空存储 |

`InMemoryStateStore` 实现 `StateStore` 的全部方法：`get`、`set`、`increment`、`remove`、`snapshot`。

`increment` 实现：读取键对应值，若为 `Value::Int(n)` 则加 `by`，否则从 0 开始；写回 `Value::Int(next)` 并返回新值，整体在同一 `Mutex` 锁内完成，无竞态。

#### `save_macros` 原子写入流程

```
1. serde_json::to_string_pretty(macros) → json: String
2. fs::write("<path>.tmp", &json)        // 中间文件
3. fs::rename("<path>.tmp", path)        // 原子替换（POSIX）
```

若步骤 2 失败，原文件不受影响；步骤 3 失败极罕见（同盘 rename 通常是原子的）。

## 依赖关系

- [`koakuma_core::domain::Macro`](../core/domain.md) — 序列化/反序列化目标类型
- `koakuma_core::state::StateStore` — `InMemoryStateStore` 实现该 trait
- `koakuma_core::value::Value` — 存储值类型
- `serde_json` — JSON 序列化
- `thiserror` — `StoreError` 的 `#[derive(Error)]`

## 设计说明

使用 `BTreeMap`（而非 `HashMap`）使 `snapshot` 的键顺序确定，与 UI 变量监视面板的排序一致，也便于测试断言。

`Mutex::lock().unwrap()` 在 panic 情况下会中毒（poison）并传播；这是 V1 可接受行为，V2 考虑改为 `RwLock`（读多写少场景）或 `DashMap`（高并发场景）。

`load_macros` 对 `NotFound` 的特殊处理（返回空列表而非 `Io` 错误）是有意为之：首次启动时 `macros.json` 不存在是正常状态，无需调用方区分"不存在"和"读取失败"。