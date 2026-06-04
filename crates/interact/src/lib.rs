//! Background interaction and target abstraction for Koakuma (M2.3).
//!
//! Provides [`InteractionBackend`] implementations for:
//! - **Windows UIA** — UI Automation (background, no focus steal)
//! - **Windows PostMessage** — Win32 message posting (background, legacy apps)
//! - **Windows SendInput** — synthetic input with foreground takeover (fallback)
//! - **CDP** — Chrome DevTools Protocol for browser tab control
//!
//! The [`BackendRegistry`] performs capability negotiation: it picks the highest
//! [`Tier`] backend available for a [`TargetSelector`] and applies the
//! [`OnNoBackground`](koakuma_core::domain::OnNoBackground) fallback policy when
//! no `Background`-tier backend is found.
//!
//! # Example
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use koakuma_interact::{BackendRegistry, backends::StubBackend, backend::Tier};
//! use koakuma_interact::backends::cdp::CdpBackend;
//!
//! let mut reg = BackendRegistry::new();
//! reg.register(StubBackend::new(Tier::Background));
//! reg.register(CdpBackend::new(9222));
//! let reg = Arc::new(reg);
//! ```

pub mod action;
pub mod backend;
pub mod backends;
pub mod error;
pub mod negotiator;

pub use action::register_actions;
pub use backend::{InteractionBackend, ResolvedTarget, Tier, UiNode};
pub use error::{BackendError, DispatchError};
pub use negotiator::BackendRegistry;
