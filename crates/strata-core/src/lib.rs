// SPDX-License-Identifier: GPL-3.0-or-later
//! Domain types shared by every Strata crate.
//!
//! This crate has no UI and no database driver dependency.

pub mod client_config;
mod connection;
mod error;
mod metadata;
mod secret;
mod server;

pub use connection::{
    ConnectionConfig, ConnectionId, DbKind, Endpoint, Environment, ParseEnumError, PasswordPolicy,
    SshAuth, SshTunnel, TlsConfig, TlsMode, ValidationError,
};
pub use error::{ErrorKind, ErrorReport};
pub use metadata::{
    ColumnInfo, ConstraintInfo, ConstraintKind, IndexInfo, ObjectInfo, ObjectKind, ObjectRef,
    Reference, TableDetails, TablePart, TriggerInfo,
};
pub use secret::Secret;
pub use server::ServerInfo;
