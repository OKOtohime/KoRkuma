//! Core domain model, engine protocol, and extension traits for Koakuma.
//!
//! This crate defines all cross-platform shared abstractions. Platform-specific
//! crates (`koakuma-hooks`, `koakuma-actions`, `koakuma-constraints`) implement
//! the traits defined here and register their implementations through
//! [`registry::Registry`].
//!
//! # Pipeline
//!
//! Events flow through:
//! [`traits::HookProvider`] → [`event::Event`] → [`router::EventRouter`] →
//! [`traits::TriggerSpec`] → [`traits::Constraint`] → [`traits::Action`]
//!
//! # Examples
//!
//! ```rust
//! use koakuma_core::registry::Registry;
//! use koakuma_core::router::EventRouter;
//!
//! let _registry = Registry::with_builtins();
//! let _router = EventRouter::new();
//! ```

pub mod builtins;
pub mod context;
pub mod schema;
pub mod domain;
pub mod engine;
pub mod engine_loop;
pub mod error;
pub mod event;
pub mod permission;
pub mod registry;
pub mod router;
pub mod scheduler;
pub mod state;
pub mod traits;
pub mod value;
pub mod workflow;
