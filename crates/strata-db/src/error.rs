// SPDX-License-Identifier: GPL-3.0-or-later
//! Backend-independent database errors.

use strata_core::{ErrorKind, ErrorReport};

/// Database error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DbError {
    /// The host name cannot be resolved.
    #[error("host not found: {0}")]
    HostNotFound(String),
    /// The server is unreachable or refused the connection.
    #[error("connection refused: {0}")]
    ConnectionRefused(String),
    /// The operation timed out.
    #[error("timed out: {0}")]
    Timeout(String),
    /// Authentication failed.
    #[error("authentication failed: {0}")]
    Authentication(String),
    /// TLS negotiation or verification failed.
    #[error("TLS error: {0}")]
    Tls(String),
    /// The server reported an error.
    #[error("{message} (SQLSTATE {sqlstate})")]
    Server {
        /// SQLSTATE code.
        sqlstate: String,
        /// Primary message.
        message: String,
        /// Optional detail.
        detail: Option<String>,
        /// Optional hint.
        hint: Option<String>,
    },
    /// Invalid connection settings (e.g. unreadable certificate).
    #[error("invalid configuration: {0}")]
    Config(String),
    /// Not supported by this backend yet.
    #[error("not supported: {0}")]
    Unsupported(String),
    /// The connection is closed.
    #[error("connection closed")]
    Closed,
    /// The object does not exist (any more).
    #[error("not found: {0}")]
    NotFound(String),
    /// Other I/O or protocol failure.
    #[error("{0}")]
    Other(String),
}

/// Result alias for this crate.
pub type Result<T, E = DbError> = std::result::Result<T, E>;

impl DbError {
    /// Whether the connection was lost (reconnecting may help, CON-10).
    pub fn is_connection_lost(&self) -> bool {
        matches!(
            self,
            Self::Closed | Self::ConnectionRefused(_) | Self::Timeout(_)
        )
    }

    /// Converts the error into a report for the UI.
    pub fn report(&self) -> ErrorReport {
        let kind = match self {
            Self::HostNotFound(_) => ErrorKind::HostNotFound,
            Self::ConnectionRefused(_) | Self::Closed => ErrorKind::ConnectionRefused,
            Self::Timeout(_) => ErrorKind::Timeout,
            Self::Authentication(_) => ErrorKind::Authentication,
            Self::Tls(_) => ErrorKind::Tls,
            Self::Server { .. } => ErrorKind::Server,
            Self::Config(_) => ErrorKind::Configuration,
            Self::Unsupported(_) => ErrorKind::Unsupported,
            Self::NotFound(_) => ErrorKind::NotFound,
            Self::Other(_) => ErrorKind::Other,
        };
        let mut detail = self.to_string();
        if let Self::Server {
            detail: Some(d), ..
        } = self
        {
            detail.push_str("\nDETAIL: ");
            detail.push_str(d);
        }
        if let Self::Server { hint: Some(h), .. } = self {
            detail.push_str("\nHINT: ");
            detail.push_str(h);
        }
        ErrorReport::new(kind, detail)
    }
}
