// SPDX-License-Identifier: GPL-3.0-or-later
//! Messages exchanged between the UI and the engine.

/// Request sent by the UI to the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command {
    /// Stops the engine; pending operations are dropped.
    Shutdown,
}

/// Notification sent by the engine to the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UiEvent {
    /// The engine is running and accepts commands.
    Ready,
}
