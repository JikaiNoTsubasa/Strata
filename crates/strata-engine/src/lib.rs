// SPDX-License-Identifier: GPL-3.0-or-later
//! Strata engine: every asynchronous service (database sessions, query
//! execution, result paging, export) runs here, on the Tokio runtime.
//!
//! The engine never depends on GTK. The UI talks to it exclusively through
//! typed messages (see `docs/ADR/0003-async-model.md`):
//!
//! * [`Command`] — UI → engine;
//! * [`UiEvent`] — engine → UI.

mod protocol;

pub use protocol::{Command, UiEvent};

use std::ops::ControlFlow;

use tokio::runtime::Handle;

/// Capacity of the UI → engine command channel.
const COMMAND_CAPACITY: usize = 256;
/// Capacity of the engine → UI event channel.
const EVENT_CAPACITY: usize = 1024;

/// UI-side endpoints of the engine channels.
///
/// Dropping the handle (or sending [`Command::Shutdown`]) stops the engine.
#[derive(Debug)]
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
pub fn spawn(runtime: &Handle) -> EngineHandle {
    let (cmd_tx, cmd_rx) = async_channel::bounded(COMMAND_CAPACITY);
    let (ev_tx, ev_rx) = async_channel::bounded(EVENT_CAPACITY);
    runtime.spawn(run(cmd_rx, ev_tx));
    EngineHandle {
        commands: cmd_tx,
        events: ev_rx,
    }
}

/// Engine main loop: dispatches commands until shutdown.
async fn run(commands: async_channel::Receiver<Command>, events: async_channel::Sender<UiEvent>) {
    tracing::debug!("engine started");
    if events.send(UiEvent::Ready).await.is_err() {
        return;
    }
    while let Ok(command) = commands.recv().await {
        if handle(command).is_break() {
            break;
        }
    }
    events.close();
    tracing::debug!("engine stopped");
}

/// Processes one command; `Break` stops the engine.
fn handle(command: Command) -> ControlFlow<()> {
    match command {
        Command::Shutdown => ControlFlow::Break(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    async fn next_event(handle: &EngineHandle) -> Option<UiEvent> {
        tokio::time::timeout(Duration::from_secs(1), handle.events().recv())
            .await
            .expect("engine did not answer in time")
            .ok()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn engine_reports_ready() {
        let handle = spawn(&Handle::current());
        assert_eq!(next_event(&handle).await, Some(UiEvent::Ready));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_closes_event_channel() {
        let handle = spawn(&Handle::current());
        assert_eq!(next_event(&handle).await, Some(UiEvent::Ready));
        assert!(handle.send(Command::Shutdown));
        assert_eq!(next_event(&handle).await, None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_fails_once_engine_stopped() {
        let handle = spawn(&Handle::current());
        assert!(handle.send(Command::Shutdown));
        while next_event(&handle).await.is_some() {}
        assert!(!handle.send(Command::Shutdown));
    }
}
