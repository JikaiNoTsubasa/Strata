// SPDX-License-Identifier: GPL-3.0-or-later
//! Strata engine: every asynchronous service (database sessions, query
//! execution, result paging, export) runs here, on the Tokio runtime.
//!
//! The engine never depends on GTK. The UI talks to it exclusively through
//! typed messages (see `docs/ADR/0003-async-model.md`):
//!
//! * [`Command`] — UI → engine;
//! * [`UiEvent`] — engine → UI.

mod engine;
mod pool;
mod protocol;
mod tunnel;

use std::path::PathBuf;
use std::sync::Arc;

pub use protocol::{
    Command, MetaHint, MetaItem, MetaRequest, PasswordChange, RequestId, SessionState, TestReport,
    UiEvent,
};
use strata_secrets::SecretStore;
use tokio::runtime::Handle;

/// Capacity of the UI → engine command channel.
const COMMAND_CAPACITY: usize = 256;
/// Capacity of the engine → UI event channel.
const EVENT_CAPACITY: usize = 1024;

/// Where the internal database lives.
#[derive(Debug, Clone)]
pub enum StoreLocation {
    /// `$XDG_DATA_HOME/strata/strata.db`.
    Default,
    /// An explicit file.
    Path(PathBuf),
    /// A private in-memory database (tests).
    InMemory,
}

/// Engine dependencies.
pub struct EngineConfig {
    /// Internal database location.
    pub store: StoreLocation,
    /// Password storage.
    pub secrets: Arc<dyn SecretStore>,
    /// Strata's SSH `known_hosts` file; `None` for the default location
    /// (`$XDG_DATA_HOME/strata/known_hosts`, plus `~/.ssh/known_hosts` read
    /// only).
    pub known_hosts: Option<PathBuf>,
}

/// UI-side endpoints of the engine channels.
///
/// Dropping every handle (or sending [`Command::Shutdown`]) stops the engine.
#[derive(Debug, Clone)]
pub struct EngineHandle {
    commands: async_channel::Sender<Command>,
    events: async_channel::Receiver<UiEvent>,
}

impl EngineHandle {
    /// Sends a command to the engine without blocking.
    ///
    /// Returns `false` if the engine is gone or its queue is full; the caller
    /// is expected to report the failure to the user.
    pub fn send(&self, command: Command) -> bool {
        match self.commands.try_send(command) {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!(full = err.is_full(), "engine command dropped");
                false
            }
        }
    }

    /// Receiver of the events emitted by the engine.
    ///
    /// The UI drains it from a task spawned on the GLib main context.
    pub fn events(&self) -> async_channel::Receiver<UiEvent> {
        self.events.clone()
    }
}

/// Starts the engine on the given Tokio runtime and returns its UI endpoints.
pub fn spawn(runtime: &Handle, config: EngineConfig) -> EngineHandle {
    let (cmd_tx, cmd_rx) = async_channel::bounded(COMMAND_CAPACITY);
    let (ev_tx, ev_rx) = async_channel::bounded(EVENT_CAPACITY);
    runtime.spawn(engine::run(config, cmd_rx, ev_tx));
    EngineHandle {
        commands: cmd_tx,
        events: ev_rx,
    }
}
