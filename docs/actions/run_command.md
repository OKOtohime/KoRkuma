# `run_command` — 外部进程执行

> **Crate**: `korkuma-actions` · **文件**: `crates/actions/src/run_command.rs`
> **最后同步**: 2026-06-02

## 职责

实现 `ActionConfig::RunCommand`，通过 `std::process::Command` 启动外部进程。完全跨平台，无操作系统限制。

在管道中处于 Action 层末端，执行用户指定的系统命令（脚本、工具、程序）。

## 公开 API

### 类型

| 类型 | 种类 | 说明 |
|------|------|------|
| `RunCommandAction` | struct | 封装程序名、参数列表、捕获标志 |

### 函数 / 方法

| 签名 | 说明 |
|------|------|
| `fn build(c: &ActionConfig) -> Option<Box<dyn Action>>` | 工厂：从 `ActionConfig::RunCommand` 构建 |

#### `execute` 行为

| `capture` | 行为 |
|-----------|------|
| `false` | `spawn()` 后立即返回 `Continue`，不等待子进程结束 |
| `true` | `output()` 等待完成；stdout/stderr 写入 `LogHandle`；非零退出码返回 `ActionError::Failed` |

## 依赖关系

- `korkuma_core::permission` — 要求 `Permission::RunCommand` 已授权
- `korkuma_core::context::ExecContext` — 访问 `permissions` 和 `log`

## 设计说明

`capture = false` 使子进程在后台独立运行，父进程（引擎线程）不阻塞。这是 V1 的设计选择；V2 异步化后，`capture = true` 也将改为非阻塞等待（`tokio::process::Command`）。

权限检查在 `execute` 入口做，而非 `build` 时——与 `korkuma-core` 设计保持一致：build 只构造，execute 才强制授权。
