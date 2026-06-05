# KoRkuma

> **Experimental WIP** — A cross-platform desktop automation platform built with Rust + Slint, inspired by [MacroDroid](https://www.macrodroid.com/) and [KeyMouseGo](https://github.com/taojy123/KeyMouseGo).

---

## What is KoRkuma?

KoRkuma lets you automate your desktop by defining **macros** — each macro is a three-part rule:

```
Trigger  ──►  Constraint  ──►  Action
 When?          If yes?         Do what?
```

- **Trigger** — when to fire: hotkeys, window focus, process start/stop, schedule, file changes
- **Constraint** — whether to proceed: AND/OR/NOT tree of conditions (active window, time range, variable compare, Rhai DSL)
- **Action** — what to do: run a command, send a notification, simulate input, set a variable, run a script, interact with a background window, and more

Macros are stored as JSON, fully serializable, diffable, and shareable.

---

## Status

This project is **experimental** and under active solo development. The core engine and GUI are functional on Windows; Linux/Wayland and Android support are planned but not yet implemented.

| Milestone | Description | Status |
|-----------|-------------|--------|
| M1.1 | Workspace, core engine, domain model, 25 tests | ✅ |
| M1.2 | Windows hooks (hotkey / window / process) + built-in actions | ✅ |
| M1.3 | Slint GUI, JSON persistence, hot reload, adaptive renderer | ✅ |
| M1.4 | Permission manager, Rhai scripting, sandboxed DSL constraints | ✅ |
| M2.1 | Async workflow engine (Seq / Parallel / If / While / Retry / Timeout) | ✅ |
| M2.2 | Workflow scheduler, concurrency policies, resource arbitration | ✅ |
| M2.3 | Background interaction: Windows UI Automation, CDP, PostMessage | ✅ |
| M2.4 | Visual constraint tree editor + workflow block editor | ✅ |
| M2.5 | Permission approval UI, permission management page, variable monitor | ✅ |
| M3.x | Plugin system (WASM), Linux X11/Wayland support | Planned |
| M4.x | Android companion app, desktop↔mobile protocol | Planned |

**Current test count: 210 passing** across the full workspace.

---

## Key Features

### Macro Engine
- **Typed, serializable configuration** — `TriggerConfig`, `ConstraintConfig`, `ActionConfig` enums with full serde support; macros round-trip to/from JSON
- **Smart event routing** — O(1) `EventKind → [MacroId]` index; only subscribed macros are evaluated per event
- **AND/OR/NOT constraint tree** — arbitrary nesting, evaluated recursively; empty tree always passes

### Async Workflow Engine
- Replaces flat action lists with a **`WorkflowNode` tree**: `Seq`, `Parallel`, `If`, `While`, `ForEach`, `Retry`, `Timeout`, `Wait`
- Async execution via Tokio; parallel branches run concurrently with isolated local variable scopes
- Old `macros.json` files load without changes (`actions` is auto-wrapped in `Seq`)

### Concurrency & Scheduling
Six per-macro concurrency policies: `Parallel` (default), `Queue`, `DropIfRunning`, `RestartIfRunning`, `Debounce`, `Throttle`

Shared resource arbitration (`input`, `clipboard`, `window:<id>`) prevents simultaneous macro runs from interleaving synthetic input.

### Background Interaction (Windows)
Macros can operate on **background windows without stealing focus**:

| Backend | Method | Capability |
|---------|--------|------------|
| `uia` | Windows UI Automation | Background (no focus steal) |
| `win-msg` | PostMessage | Background (legacy Win32) |
| `cdp` | Chrome DevTools Protocol | Background (browser tabs) |
| `sendinput` | SetForegroundWindow + SendInput | Foreground synthetic (fallback) |

Capability negotiation picks the best available backend; `OnNoBackground` policy controls fallback behavior (`Degrade`, `Fail`, or `Queue`).

### Permission Manager
- Actions declare required permissions statically
- UI aggregates permissions at **save time** and presents a single approval dialog
- Permissions are enforced at **run time** — unauthorized actions are blocked with a logged error
- Per-macro permission revocation from the Permissions tab

### Rhai Scripting
- `RunScript` action: sandboxed Rhai scripts with `get_var`/`set_var`/`log` host functions
- `Expression` constraint: Rhai boolean DSL evaluated against the current state
- Resource limits: max operations, call depth, string size; `CancellationToken` integration for timeout/stop

### Visual GUI (Slint)
- Macro list with enable/disable, manual trigger, dry-run, add/delete
- **Constraint tree editor** — flat-indented list model ↔ `ConstraintExpr`; add leaf, wrap in AND/OR/NOT, delete
- **Workflow block editor** — flat-indented list model ↔ `WorkflowNode`; add action/If/Parallel, target selector, `OnNoBackground` control
- **Permissions tab** — current grants per macro + revoke buttons
- **Variable monitor** — live `StateStore` snapshot, refreshes every second
- **Log view** — real-time action log from the engine
- JSON preview for triggers (editable in future milestones)
- Hot reload: editing `macros.json` externally updates the running engine automatically

---

## Architecture

```
┌──────────────────────────────────────────────────────┐
│  Slint UI (main thread)                              │
│  VecModel<MacroItem> · ConstraintTree · WorkflowTree │
│  ──(EngineCommand via mpsc)──► Engine thread         │
│  ◄──(EngineEvent via upgrade_in_event_loop)──        │
└──────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────┐
│  Engine thread                                       │
│  EventRouter · WorkflowScheduler · StateStore        │
│  Registry (Trigger / Constraint / Action factories)  │
└──────────────────────────────────────────────────────┘
        ▲ Events                    ▼ Tokio tasks
┌───────────────────┐   ┌──────────────────────────────┐
│  Hook threads     │   │  Tokio multi-thread runtime  │
│  HotkeyProvider   │   │  WorkflowEngine              │
│  WindowFocus      │   │  InteractionBackends         │
│  ProcessProvider  │   │  CancellationToken / Timeout │
└───────────────────┘   └──────────────────────────────┘
```

**Crate layout:**

```
crates/
├── core/        Domain model, traits, engine loop, router, scheduler, workflow
├── hooks/       Platform hook providers (Windows: keyboard, window, process)
├── actions/     RunCommand, Notify, SimulateInput, SetVariable, Delay
├── constraints/ Built-in constraint evaluators
├── script/      Rhai RunScript action + Expression DSL constraint
├── store/       JSON persistence (atomic write) + InMemoryStateStore
├── interact/    InteractionBackend trait, UIA/CDP/PostMessage/SendInput backends
└── app/         Slint GUI + main (assembles everything)
```

---

## Building

**Prerequisites:** Rust stable toolchain, platform build tools.

```bash
# Build
cargo build

# Run (opens the GUI)
cargo run

# Run tests
cargo test

# Lint
cargo clippy

# Format
cargo fmt
```

On Windows, copy `macros.json.example` to `macros.json` next to the binary and launch — the example macros demonstrate hotkeys, window focus triggers, process detection, variable state, and scripting.

**Supported targets:**
- `x86_64-unknown-linux-gnu` (build + test; Linux GUI functional)
- `x86_64-pc-windows-gnullvm` (cross-compile via `cargo-zigbuild`)

---

## Roadmap

| Wave | Focus |
|------|-------|
| V3 | Plugin system (WASM components + WIT world), Linux X11 & Wayland hooks |
| V4 | Android accessibility service, desktop↔mobile secure protocol, plugin marketplace |

---

## Inspirations

- **[MacroDroid](https://www.macrodroid.com/)** — the trigger/constraint/action macro model for Android
- **[KeyMouseGo](https://github.com/taojy123/KeyMouseGo)** — lightweight keyboard & mouse automation

---

## Contributing

This is a solo experimental project. Issues, ideas, and pull requests are welcome — please open an issue to discuss significant changes before submitting a PR.

---

## License

See [LICENSE](LICENSE).
