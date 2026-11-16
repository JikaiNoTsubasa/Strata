// SPDX-License-Identifier: GPL-3.0-or-later
//! Error report exchanged between the engine and the UI.

/// Category of a failure; the UI maps it to a translated, human-readable
/// sentence, while [`ErrorReport::detail`] keeps the technical text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The host name could not be resolved.
    HostNotFound,
    /// The server refused the connection or is unreachable.
    ConnectionRefused,
    /// The operation did not complete in time.
    Timeout,
    /// Wrong user name or password.
    Authentication,
    /// A password is required but none is available.
    MissingPassword,
    /// TLS negotiation or certificate verification failed.
    Tls,
    /// The server reported an error (see the detail for its SQLSTATE).
    Server,
    /// The connection settings are invalid.
    Configuration,
    /// The system keyring failed or is unavailable.
    Keyring,
    /// Strata's internal storage failed.
    Storage,
    /// The feature is not available for this database type yet.
    Unsupported,
    /// The connection is not open.
    NotConnected,
    /// The requested object no longer exists.
    NotFound,
    /// The SSH server cannot be reached.
    SshConnection,
    /// SSH authentication failed (password, key or agent refused).
    SshAuthentication,
    /// The SSH password or key passphrase is required.
    MissingSshSecret,
    /// The SSH server key is unknown; the detail holds its fingerprint.
    UnknownHostKey,
    /// The SSH server key differs from the recorded one (possible attack).
    HostKeyMismatch,
    /// Any other failure.
    Other,
}

/// A failure, as shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorReport {
    /// Category, used to pick the user-facing message.
    pub kind: ErrorKind,
    /// Technical detail (driver message, SQLSTATE…). Never contains secrets.
    pub detail: String,
}

impl ErrorReport {
    /// Builds a report.
    pub fn new(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}
