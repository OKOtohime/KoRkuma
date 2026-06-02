//! Built-in [`Action`](koakuma_core::traits::Action) implementations.
//!
//! # Provided actions
//!
//! | Action | Config variant | Platform |
//! |--------|---------------|----------|
//! | [`RunCommandAction`] | `ActionConfig::RunCommand` | All |
//! | [`NotifyAction`] | `ActionConfig::Notify` | All |
//! | [`SimulateInputAction`] | `ActionConfig::SimulateInput` | Windows (stub elsewhere) |
//!
//! `SetVariable` and `Delay` are lightweight builtins that live in
//! `koakuma-core` (`builtins.rs`) and are pre-registered by
//! [`Registry::with_builtins`](koakuma_core::registry::Registry::with_builtins).
//!
//! # Usage
//!
//! ```rust,no_run
//! use koakuma_core::registry::Registry;
//! use koakuma_actions::register_all;
//!
//! let mut registry = Registry::with_builtins();
//! register_all(&mut registry);
//! ```
//!
//! M1.4 adds: `RunScript` (Rhai, sandboxed, permission-gated).
//! M2.2 adds: `HttpRequest` (async via Tokio).

mod notify;
mod run_command;
mod simulate_input;

pub use notify::NotifyAction;
pub use run_command::RunCommandAction;
pub use simulate_input::SimulateInputAction;

use koakuma_core::registry::Registry;

/// Registers all built-in action factories with `registry`.
///
/// Call this from `app` startup after [`Registry::with_builtins`].
///
/// # Examples
///
/// ```rust,no_run
/// use koakuma_core::registry::Registry;
/// use koakuma_actions::register_all;
///
/// let mut registry = Registry::with_builtins();
/// register_all(&mut registry);
/// ```
pub fn register_all(registry: &mut Registry) {
    registry.register_action(run_command::build);
    registry.register_action(notify::build);
    registry.register_action(simulate_input::build);
}
