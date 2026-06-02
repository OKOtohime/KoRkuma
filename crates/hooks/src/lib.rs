//! Platform-specific [`HookProvider`](koakuma_core::traits::HookProvider) implementations.
//!
//! # Provided providers
//!
//! | Provider | Platform | Event kind |
//! |----------|----------|------------|
//! | [`HotkeyProvider`] | Windows | [`EventKind::Hotkey`] |
//! | [`WindowFocusProvider`] | Windows | [`EventKind::WindowFocus`] |
//! | [`ProcessProvider`] | Windows | [`EventKind::Process`] |
//!
//! Platform-specific providers are compiled only on the target OS. The
//! [`trigger_spec`] module contains cross-platform [`TriggerSpec`] implementations
//! that match events produced by these providers.
//!
//! # Usage
//!
//! ```rust,no_run
//! use koakuma_core::registry::Registry;
//! use koakuma_hooks::register_trigger_specs;
//!
//! let mut registry = Registry::with_builtins();
//! register_trigger_specs(&mut registry);
//! ```
//!
//! M2.1 adds Linux/X11 counterparts; Wayland accepted as degraded.

pub mod trigger_spec;

#[cfg(target_os = "windows")]
mod platform_windows;

#[cfg(target_os = "windows")]
pub use platform_windows::{HotkeyProvider, ProcessProvider, WindowFocusProvider};

pub use trigger_spec::{HotkeyTriggerSpec, ProcessTriggerSpec, WindowFocusTriggerSpec};

use koakuma_core::registry::Registry;

/// Registers [`TriggerSpec`](koakuma_core::traits::TriggerSpec) factories for all three
/// hook event kinds into `registry`.
///
/// Call this from `app` startup after [`Registry::with_builtins`].
///
/// # Examples
///
/// ```rust,no_run
/// use koakuma_core::registry::Registry;
/// use koakuma_hooks::register_trigger_specs;
///
/// let mut registry = Registry::with_builtins();
/// register_trigger_specs(&mut registry);
/// ```
pub fn register_trigger_specs(registry: &mut Registry) {
    registry.register_trigger(trigger_spec::build_hotkey_spec);
    registry.register_trigger(trigger_spec::build_window_focus_spec);
    registry.register_trigger(trigger_spec::build_process_spec);
}
