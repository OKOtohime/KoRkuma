# Koakuma 开发者文档

> **最后同步**: 2026-06-04 (M2.3：后台交互与目标抽象)

Koakuma 是一个跨平台自动化平台（类 MacroDroid），使用 Rust + Slint 构建。

架构管道：`Hook → Event → EventRouter → ConstraintEngine → WorkflowEngine → Actions`

M2.1 起，Action 腿由 [`core/workflow`](core/workflow.md) 的异步工作流引擎驱动，支持 `Seq`/`Parallel`/`If`/`While`/`ForEach`/`Retry`/`Timeout`/`Wait` 控制流节点（见 [`core/domain` 的 `WorkflowNode`](core/domain.md)）。

## Crate 索引

| Crate | 路径 | 说明 |
|-------|------|------|
| [`koakuma-core`](core/README.md) | `crates/core` | 核心引擎、领域模型、路由、约束 |
| [`koakuma-actions`](actions/README.md) | `crates/actions` | 内置 Action 实现 |
| [`koakuma-hooks`](hooks/README.md) | `crates/hooks` | 平台事件监听 (HookProvider) |
| [`koakuma-store`](store/README.md) | `crates/store` | 状态存储 (InMemoryStateStore) |
| [`koakuma-constraints`](constraints/README.md) | `crates/constraints` | 内置 Constraint 实现 |
| [`koakuma-script`](script/README.md) | `crates/script` | Rhai 脚本引擎集成（RunScript Action + Expression DSL 约束） |
| [`koakuma-interact`](interact/README.md) | `crates/interact` | 后台交互后端（UIA / CDP / PostMessage / SendInput）+ 能力协商 |
| [`koakuma-app`](app/README.md) | `crates/app` | Slint GUI 入口 |