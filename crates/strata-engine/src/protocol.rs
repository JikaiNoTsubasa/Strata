// SPDX-License-Identifier: GPL-3.0-or-later
//! Messages exchanged between the UI and the engine.

use std::time::Duration;

use strata_core::client_config::ImportCandidate;
use strata_core::{
    ConnectionConfig, ConnectionId, ConstraintKind, ErrorReport, ObjectKind, ObjectRef, Secret,
    ServerInfo, TableDetails, TablePart,
};

/// Identifier chosen by the UI to match a response with its request.
pub type RequestId = u64;

/// What to do with the stored password when saving a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordChange {
    /// Keep the stored password, if any.
    Keep,
    /// Store this password in the keyring.
    Set(Secret),
    /// Remove the stored password.
    Clear,
}

/// Level of the schema tree to load (NAV-1, NAV-2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MetaRequest {
    /// Databases of the server.
    Databases,
    /// Schemas of a database.
    Schemas {
        /// Database.
        database: String,
    },
    /// Objects of one kind in a schema.
    Objects {
        /// Database.
        database: String,
        /// Schema.
        schema: String,
        /// Kind of objects.
        kind: ObjectKind,
    },
    /// Columns, indexes, constraints or triggers of a table.
    TablePart {
        /// Database.
        database: String,
        /// Schema.
        schema: String,
        /// Table, view or materialized view.
        table: String,
        /// Part to list.
        part: TablePart,
    },
}

/// What a tree item represents, to pick its icon and children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaHint {
    /// A database; `current` for the one the main session uses.
    Database {
        /// Database of the main session.
        current: bool,
    },
    /// A schema.
    Schema,
    /// A schema object.
    Object(ObjectKind),
    /// A column.
    Column {
        /// Part of the primary key.
        primary_key: bool,
    },
    /// An index.
    Index {
        /// Unique index.
        unique: bool,
    },
    /// A constraint.
    Constraint(ConstraintKind),
    /// A trigger.
    Trigger,
}

/// One item of the schema tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaItem {
    /// Name.
    pub name: String,
    /// Nature of the item.
    pub hint: MetaHint,
    /// Short secondary text (column type, index method…).
    pub detail: Option<String>,
    /// Longer text for the tooltip (comment, definition).
    pub comment: Option<String>,
    /// Arguments of functions and procedures.
    pub signature: Option<String>,
}

/// Request sent by the UI to the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command {
    /// Stops the engine; pending operations are dropped.
    Shutdown,
    /// Reports whether the keyring can store passwords.
    CheckKeyring,
    /// Loads every saved connection.
    LoadConnections,
    /// Creates or updates a connection.
    SaveConnection {
        /// Settings to save.
        config: ConnectionConfig,
        /// Password update (keyring policy only).
        password: PasswordChange,
        /// SSH password or key passphrase update (keyring policy only).
        ssh_password: PasswordChange,
    },
    /// Deletes a connection, its stored password and its session.
    DeleteConnection(ConnectionId),
    /// Copies a connection (and its stored password) under a new name.
    DuplicateConnection {
        /// Connection to copy.
        id: ConnectionId,
        /// Name of the copy.
        name: String,
    },
    /// Tests settings that may not be saved yet (CON-3).
    TestConnection {
        /// Correlation identifier.
        request: RequestId,
        /// Settings to test.
        config: ConnectionConfig,
        /// Password typed by the user; otherwise resolved from the policy.
        password: Option<Secret>,
        /// SSH password or passphrase typed by the user.
        ssh_secret: Option<Secret>,
    },
    /// Opens a session on a saved connection.
    Connect {
        /// Connection to open.
        id: ConnectionId,
        /// Password typed by the user; otherwise resolved from the policy.
        password: Option<Secret>,
        /// SSH password or passphrase typed by the user.
        ssh_secret: Option<Secret>,
    },
    /// Trusts the SSH server key last offered by `host:port` (trust on first
    /// use); answered by [`UiEvent::HostKeyTrusted`].
    TrustHostKey {
        /// SSH server.
        host: String,
        /// SSH port.
        port: u16,
    },
    /// Closes the session of a connection.
    Disconnect(ConnectionId),
    /// Lists connections importable from `pg_service.conf`, `.pgpass` and
    /// `.my.cnf` (ADR-0014).
    ScanClientConfigs,
    /// Loads one level of the schema tree of a connected connection.
    LoadMetadata {
        /// Correlation identifier.
        request: RequestId,
        /// Connection.
        connection: ConnectionId,
        /// Level to load.
        what: MetaRequest,
    },
    /// Describes a table, view or materialized view (NAV-6).
    DescribeTable {
        /// Correlation identifier.
        request: RequestId,
        /// Connection.
        connection: ConnectionId,
        /// Object to describe.
        object: ObjectRef,
    },
    /// Generates the DDL of an object (NAV-5).
    LoadDdl {
        /// Correlation identifier.
        request: RequestId,
        /// Connection.
        connection: ConnectionId,
        /// Object.
        object: ObjectRef,
    },
    /// Closes physical sessions unused for this long (CON-11); `None`
    /// disables the timeout.
    SetIdleTimeout(Option<Duration>),
}

/// Successful connection test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestReport {
    /// Server description.
    pub server: ServerInfo,
    /// Time taken to connect and query the server.
    pub duration: Duration,
}

/// State of the session of a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// Connection in progress.
    Connecting,
    /// Connected.
    Connected(ServerInfo),
    /// The connection was lost; reconnecting (CON-10).
    Reconnecting,
    /// Not connected.
    Disconnected,
    /// The connection failed.
    Failed {
        /// Cause.
        error: ErrorReport,
        /// Whether asking the user for a password may help.
        needs_password: bool,
    },
}

/// Notification sent by the engine to the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UiEvent {
    /// The engine is running and accepts commands.
    Ready,
    /// The internal database cannot be opened; connections cannot be saved.
    StoreUnavailable(ErrorReport),
    /// Whether the keyring can store passwords.
    KeyringStatus {
        /// `false` when no Secret Service / portal is reachable.
        available: bool,
    },
    /// Every saved connection, sorted by name.
    ConnectionsLoaded(Vec<ConnectionConfig>),
    /// A connection was created or updated.
    ConnectionSaved(ConnectionConfig),
    /// A connection was deleted.
    ConnectionDeleted(ConnectionId),
    /// Result of [`Command::TestConnection`].
    TestFinished {
        /// Correlation identifier.
        request: RequestId,
        /// Outcome.
        result: Result<TestReport, ErrorReport>,
    },
    /// The session state of a connection changed.
    SessionState {
        /// Connection concerned.
        id: ConnectionId,
        /// New state.
        state: SessionState,
    },
    /// Result of [`Command::ScanClientConfigs`].
    ImportCandidates {
        /// Importable connections.
        candidates: Vec<ImportCandidate>,
        /// Files that could not be used (e.g. insecure permissions).
        warnings: Vec<String>,
    },
    /// A command failed.
    OperationFailed(ErrorReport),
    /// Result of [`Command::LoadMetadata`].
    MetadataLoaded {
        /// Correlation identifier.
        request: RequestId,
        /// Items, in display order.
        result: Result<Vec<MetaItem>, ErrorReport>,
    },
    /// The SSH server key of `host:port` is now trusted.
    HostKeyTrusted {
        /// SSH server.
        host: String,
        /// SSH port.
        port: u16,
    },
    /// Result of [`Command::DescribeTable`].
    TableDescribed {
        /// Correlation identifier.
        request: RequestId,
        /// Description.
        result: Result<TableDetails, ErrorReport>,
    },
    /// Result of [`Command::LoadDdl`].
    DdlLoaded {
        /// Correlation identifier.
        request: RequestId,
        /// SQL text.
        result: Result<String, ErrorReport>,
    },
}
