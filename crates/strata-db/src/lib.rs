// SPDX-License-Identifier: GPL-3.0-or-later
//! Database abstraction and backends (see `docs/ADR/0002-database-drivers.md`).
//!
//! A [`DbDriver`] opens [`DbSession`]s; a session is one physical
//! connection with its own transaction state.

mod error;
mod postgres;
mod postgres_meta;
mod tls;

use std::net::SocketAddr;

use async_trait::async_trait;
use strata_core::{
    ConnectionConfig, DbKind, ObjectInfo, ObjectKind, ObjectRef, Secret, ServerInfo, TableDetails,
};

pub use error::{DbError, Result};

/// Entry point of a database backend.
#[async_trait]
pub trait DbDriver: Send + Sync {
    /// Database family handled by the driver.
    fn kind(&self) -> DbKind;

    /// Opens a session. `password` is `None` when no password applies.
    ///
    /// `via` is the local end of an SSH tunnel: the driver connects there
    /// while still using the configured host name for TLS verification.
    async fn connect(
        &self,
        cfg: &ConnectionConfig,
        password: Option<&Secret>,
        via: Option<SocketAddr>,
    ) -> Result<Box<dyn DbSession>>;
}

/// One open connection to a server.
#[async_trait]
pub trait DbSession: Send {
    /// Checks that the connection is alive.
    async fn ping(&mut self) -> Result<()>;

    /// Describes the server.
    async fn server_info(&mut self) -> Result<ServerInfo>;

    /// Whether the connection has been closed (by the server or the network).
    fn is_closed(&self) -> bool;

    /// Databases of the server.
    async fn list_databases(&mut self) -> Result<Vec<String>>;

    /// Schemas of the session's database (system schemas excluded).
    async fn list_schemas(&mut self) -> Result<Vec<String>>;

    /// Objects of `kind` in `schema`.
    async fn list_objects(&mut self, schema: &str, kind: ObjectKind) -> Result<Vec<ObjectInfo>>;

    /// Columns, keys, indexes, constraints and statistics of a table, view or
    /// materialized view.
    async fn describe_table(&mut self, schema: &str, name: &str) -> Result<TableDetails>;

    /// `CREATE` statement(s) of an object.
    async fn object_ddl(&mut self, object: &ObjectRef) -> Result<String>;
}

/// Returns the driver for `kind`.
pub fn driver(kind: DbKind) -> Result<&'static dyn DbDriver> {
    match kind {
        DbKind::Postgres => Ok(&postgres::PostgresDriver),
        DbKind::MySql => Err(DbError::Unsupported(
            "MySQL/MariaDB support is planned for milestone J4".into(),
        )),
    }
}
