//! Engine thread: the central event-processing loop.
//!
//! The engine thread owns [`EventRouter`], [`Registry`], and [`StateStore`].
//! It consumes two channels:
//!
//! - **command channel** (`EngineCommand`) — from the UI thread (or any controller)
//! - **event channel** (`Event`) — from hook provider threads via the returned [`EventSink`]
//!
//! For each event it calls [`EventRouter::dispatch`] and forwards the resulting
//! [`EngineEvent`]s to the caller-supplied `on_event` callback.
//! In M1.3, `on_event` wraps `slint::invoke_from_event_loop` to update the UI model.

use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{select, unbounded, Receiver};

use crate::engine::{EngineCommand, EngineEvent};
use crate::event::Event;
use crate::registry::Registry;
use crate::router::EventRouter;
use crate::state::StateStore;
use crate::traits::EventSink;

/// A running engine instance.
///
/// Dropping this handle sends [`EngineCommand::Shutdown`] and joins the engine thread.
/// Use [`send`](EngineHandle::send) to dispatch commands while the engine is running.
pub struct EngineHandle {
    cmd_tx: crossbeam_channel::Sender<EngineCommand>,
    thread: Option<JoinHandle<()>>,
}

impl EngineHandle {
    /// Sends a command to the engine thread without blocking.
    ///
    /// The send is fire-and-forget; if the engine has already shut down, the
    /// command is silently discarded.
    pub fn send(&self, cmd: EngineCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    /// Returns a cloneable [`Sender`] for dispatching [`EngineCommand`]s.
    ///
    /// Distribute clones of this sender to UI callbacks or threads that need
    /// engine access without holding a reference to `EngineHandle`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use std::sync::Arc;
    /// # use koakuma_core::engine_loop::start_engine;
    /// # use koakuma_core::engine::EngineCommand;
    /// # use koakuma_core::registry::Registry;
    /// # use koakuma_store::InMemoryStateStore;
    /// let registry = Arc::new(Registry::with_builtins());
    /// let store: Arc<dyn koakuma_core::state::StateStore> =
    ///     Arc::new(InMemoryStateStore::new());
    /// let (engine, _sink) = start_engine(registry, store, |_ev| {});
    /// let sender = engine.clone_sender();
    /// sender.send(EngineCommand::Shutdown).ok();
    /// ```
    pub fn clone_sender(&self) -> crossbeam_channel::Sender<EngineCommand> {
        self.cmd_tx.clone()
    }

    /// Sends [`EngineCommand::Shutdown`] and waits for the engine thread to exit.
    ///
    /// No-op if `stop` was already called (idempotent via `Option::take`).
    pub fn stop(&mut self) {
        let _ = self.cmd_tx.send(EngineCommand::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts the engine on a dedicated background thread and returns control handles.
///
/// # Parameters
///
/// - `registry` — shared factory registry; must be fully populated before this call
/// - `store` — shared state store; accessible from actions and constraints
/// - `on_event` — callback invoked **on the engine thread** for each [`EngineEvent`]
///
/// # Returns
///
/// - [`EngineHandle`] — send [`EngineCommand`]s and initiate shutdown
/// - [`EventSink`] — `clone()` this and pass each clone to a [`HookProvider::start`](crate::traits::HookProvider::start)
///
/// # Examples
///
/// ```rust,no_run
/// use std::sync::Arc;
/// use koakuma_core::engine_loop::start_engine;
/// use koakuma_core::engine::EngineCommand;
/// use koakuma_core::registry::Registry;
/// # use koakuma_store::InMemoryStateStore;
///
/// let registry = Arc::new(Registry::with_builtins());
/// let store: Arc<dyn koakuma_core::state::StateStore> =
///     Arc::new(InMemoryStateStore::new());
///
/// let (engine, _event_sink) = start_engine(
///     Arc::clone(&registry),
///     Arc::clone(&store),
///     |ev| eprintln!("[engine] {ev:?}"),
/// );
/// engine.send(EngineCommand::Shutdown);
/// ```
pub fn start_engine<F>(
    registry: Arc<Registry>,
    store: Arc<dyn StateStore>,
    on_event: F,
) -> (EngineHandle, EventSink)
where
    F: Fn(EngineEvent) + Send + 'static,
{
    let (cmd_tx, cmd_rx) = unbounded::<EngineCommand>();
    let (evt_tx, evt_rx) = unbounded::<Event>();
    let handle_tx = cmd_tx.clone();

    let thread = thread::Builder::new()
        .name("koakuma-engine".to_string())
        .spawn(move || engine_loop(cmd_rx, evt_rx, registry, store, on_event))
        .expect("failed to spawn engine thread");

    (EngineHandle { cmd_tx: handle_tx, thread: Some(thread) }, evt_tx)
}

// ── Engine loop ───────────────────────────────────────────────────────────────

fn engine_loop<F>(
    cmd_rx: Receiver<EngineCommand>,
    evt_rx: Receiver<Event>,
    registry: Arc<Registry>,
    store: Arc<dyn StateStore>,
    on_event: F,
) where
    F: Fn(EngineEvent),
{
    let mut router = EventRouter::new();

    loop {
        select! {
            recv(cmd_rx) -> msg => match msg {
                Ok(EngineCommand::AddMacro(m)) => {
                    router.add_macro(m);
                }
                Ok(EngineCommand::UpdateMacro(m)) => {
                    router.remove_macro(m.id);
                    router.add_macro(m);
                }
                Ok(EngineCommand::DeleteMacro(id)) => {
                    router.remove_macro(id);
                }
                Ok(EngineCommand::SetEnabled(id, enabled)) => {
                    router.set_enabled(id, enabled);
                }
                Ok(EngineCommand::TriggerManually(id)) => {
                    for ev in router.dispatch_manual_trigger(id, &registry, &store) {
                        on_event(ev);
                    }
                }
                Ok(EngineCommand::QuerySnapshot(tx)) => {
                    let _ = tx.send(router.snapshot());
                }
                Ok(EngineCommand::Shutdown) | Err(_) => return,
            },
            recv(evt_rx) -> msg => {
                if let Ok(event) = msg {
                    for ev in router.dispatch(&event, &registry, &store) {
                        on_event(ev);
                    }
                }
            },
        }
    }
}
