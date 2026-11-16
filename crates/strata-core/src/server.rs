// SPDX-License-Identifier: GPL-3.0-or-later
//! Information reported by a database server.

/// Description of the server a session is connected to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    /// Product name, e.g. `PostgreSQL`, `MySQL`, `MariaDB`.
    pub product: String,
    /// Server version string, e.g. `17.2`.
    pub version: String,
    /// Whether the session is encrypted (TLS), when known.
    pub encrypted: Option<bool>,
    /// Database the session is connected to.
    pub database: String,
}
