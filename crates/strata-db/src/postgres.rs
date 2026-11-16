// SPDX-License-Identifier: GPL-3.0-or-later
//! PostgreSQL backend (`tokio-postgres`).

use std::error::Error as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use strata_core::{
    ConnectionConfig, DbKind, Endpoint, ObjectInfo, ObjectKind, ObjectRef, Secret, ServerInfo,
    TableDetails, TlsMode,
};
use tokio::task::JoinHandle;
use tokio_postgres::config::SslMode;
use tokio_postgres::{Client, NoTls};
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::{DbDriver, DbError, DbSession, Result, postgres_meta, tls};

/// Timeout of the TCP connection itself.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Timeout of the whole handshake (TCP + TLS + authentication).
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
/// `application_name` reported to the server.
const APPLICATION_NAME: &str = "Strata";

/// PostgreSQL driver.
#[derive(Debug)]
pub(crate) struct PostgresDriver;

#[async_trait]
impl DbDriver for PostgresDriver {
    fn kind(&self) -> DbKind {
        DbKind::Postgres
    }

    async fn connect(
        &self,
        cfg: &ConnectionConfig,
        password: Option<&Secret>,
        via: Option<std::net::SocketAddr>,
    ) -> Result<Box<dyn DbSession>> {
        let mut pg = tokio_postgres::Config::new();
        pg.user(&cfg.user)
            .application_name(APPLICATION_NAME)
            .connect_timeout(CONNECT_TIMEOUT);
        if let Some(password) = password {
            pg.password(password.expose());
        }
        if let Some(db) = cfg.database.as_deref().filter(|d| !d.is_empty()) {
            pg.dbname(db);
        }

        let connecting = match &cfg.endpoint {
            Endpoint::Socket { path } => {
                // TLS is never used over Unix sockets (libpq behaviour).
                let (dir, port) = split_socket_path(path);
                pg.host_path(dir).port(port).ssl_mode(SslMode::Disable);
                tokio::time::timeout(HANDSHAKE_TIMEOUT, connect(pg, NoTls)).await
            }
            Endpoint::Tcp { host, port } => {
                match via {
                    // Through an SSH tunnel: connect to its local end, keep the
                    // real host name for TLS verification.
                    Some(addr) => pg.host(host).hostaddr(addr.ip()).port(addr.port()),
                    None => pg.host(host).port(*port),
                };
                match cfg.tls.mode {
                    TlsMode::Disable => {
                        pg.ssl_mode(SslMode::Disable);
                        tokio::time::timeout(HANDSHAKE_TIMEOUT, connect(pg, NoTls)).await
                    }
                    mode => {
                        pg.ssl_mode(if mode == TlsMode::Prefer {
                            SslMode::Prefer
                        } else {
                            SslMode::Require
                        });
                        let tls = MakeRustlsConnect::new(tls::client_config(&cfg.tls)?);
                        tokio::time::timeout(HANDSHAKE_TIMEOUT, connect(pg, tls)).await
                    }
                }
            }
        };
        let (client, task) = connecting
            .map_err(|_| DbError::Timeout("the server did not complete the handshake".into()))??;

        let mut session = PostgresSession {
            client,
            task,
            encrypted: None,
        };
        session.encrypted = session.query_encrypted().await;
        if cfg.read_only {
            session
                .client
                .batch_execute("SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY")
                .await
                .map_err(map_error)?;
        }
        Ok(Box::new(session))
    }
}

/// Connects and spawns the connection task on the current runtime.
async fn connect<T>(config: tokio_postgres::Config, tls: T) -> Result<(Client, JoinHandle<()>)>
where
    T: tokio_postgres::tls::MakeTlsConnect<tokio_postgres::Socket>,
    T::Stream: Send + 'static,
{
    let (client, connection) = config.connect(tls).await.map_err(map_error)?;
    let task = tokio::spawn(async move {
        if let Err(err) = connection.await {
            tracing::debug!(%err, "PostgreSQL connection ended");
        }
    });
    Ok((client, task))
}

/// Splits a socket path into (directory, port): `…/.s.PGSQL.5433` gives the
/// port explicitly; a directory means the default port.
fn split_socket_path(path: &Path) -> (PathBuf, u16) {
    let port = strata_core::client_config::socket_port(path);
    let is_socket_file = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with(".s.PGSQL."));
    match path.parent() {
        Some(dir) if is_socket_file => (dir.to_path_buf(), port),
        _ => (path.to_path_buf(), port),
    }
}

/// An open PostgreSQL session.
struct PostgresSession {
    client: Client,
    task: JoinHandle<()>,
    encrypted: Option<bool>,
}

impl Drop for PostgresSession {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl PostgresSession {
    async fn database(&self) -> Result<String> {
        self.client
            .query_one("SELECT current_database()::text", &[])
            .await
            .map_err(map_error)?
            .try_get(0)
            .map_err(map_error)
    }

    /// Whether this session uses TLS (`pg_stat_ssl`, PostgreSQL ≥ 9.5).
    async fn query_encrypted(&self) -> Option<bool> {
        self.client
            .query_opt(
                "SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()",
                &[],
            )
            .await
            .ok()
            .flatten()
            .and_then(|row| row.try_get::<_, bool>(0).ok())
    }
}

#[async_trait]
impl DbSession for PostgresSession {
    async fn ping(&mut self) -> Result<()> {
        self.client
            .simple_query("SELECT 1")
            .await
            .map(drop)
            .map_err(map_error)
    }

    async fn server_info(&mut self) -> Result<ServerInfo> {
        let row = self
            .client
            .query_one(
                "SELECT current_setting('server_version'), current_database()::text",
                &[],
            )
            .await
            .map_err(map_error)?;
        Ok(ServerInfo {
            product: "PostgreSQL".to_owned(),
            version: row.try_get(0).map_err(map_error)?,
            encrypted: self.encrypted,
            database: row.try_get(1).map_err(map_error)?,
        })
    }

    fn is_closed(&self) -> bool {
        self.client.is_closed()
    }

    async fn list_databases(&mut self) -> Result<Vec<String>> {
        postgres_meta::list_databases(&self.client).await
    }

    async fn list_schemas(&mut self) -> Result<Vec<String>> {
        postgres_meta::list_schemas(&self.client).await
    }

    async fn list_objects(&mut self, schema: &str, kind: ObjectKind) -> Result<Vec<ObjectInfo>> {
        postgres_meta::list_objects(&self.client, schema, kind).await
    }

    async fn describe_table(&mut self, schema: &str, name: &str) -> Result<TableDetails> {
        let database = self.database().await?;
        postgres_meta::describe_table(&self.client, &database, schema, name).await
    }

    async fn object_ddl(&mut self, object: &ObjectRef) -> Result<String> {
        postgres_meta::object_ddl(&self.client, object).await
    }
}

/// Maps a driver error to a [`DbError`], keeping the full cause chain.
pub(crate) fn map_error(err: tokio_postgres::Error) -> DbError {
    if let Some(db) = err.as_db_error() {
        let code = db.code().code();
        // 28000 invalid_authorization_specification, 28P01 invalid_password
        if code == "28000" || code == "28P01" {
            return DbError::Authentication(db.message().to_owned());
        }
        return DbError::Server {
            sqlstate: code.to_owned(),
            message: db.message().to_owned(),
            detail: db.detail().map(str::to_owned),
            hint: db.hint().map(str::to_owned),
        };
    }
    if err.is_closed() {
        return DbError::Closed;
    }

    let chain = cause_chain(&err);
    let mut source = err.source();
    while let Some(cause) = source {
        if cause.downcast_ref::<rustls::Error>().is_some() {
            return DbError::Tls(chain);
        }
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            use std::io::ErrorKind as K;
            match io.kind() {
                K::ConnectionRefused | K::NotFound | K::HostUnreachable | K::NetworkUnreachable => {
                    return DbError::ConnectionRefused(chain);
                }
                K::TimedOut => return DbError::Timeout(chain),
                _ => {}
            }
        }
        source = cause.source();
    }
    let lower = chain.to_lowercase();
    if lower.contains("lookup address") || lower.contains("name or service not known") {
        DbError::HostNotFound(chain)
    } else if lower.contains("tls") || lower.contains("ssl") {
        DbError::Tls(chain)
    } else if lower.contains("timeout") || lower.contains("timed out") {
        DbError::Timeout(chain)
    } else if lower.contains("password") {
        // e.g. "password missing" when the server asks for one.
        DbError::Authentication(chain)
    } else {
        DbError::Other(chain)
    }
}

/// `error: cause: cause…` — tokio-postgres' own messages are terse.
fn cause_chain(err: &dyn std::error::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !text.contains(&cause_text) {
            text.push_str(": ");
            text.push_str(&cause_text);
        }
        source = cause.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_forms() {
        assert_eq!(
            split_socket_path(Path::new("/run/postgresql")),
            (PathBuf::from("/run/postgresql"), 5432)
        );
        assert_eq!(
            split_socket_path(Path::new("/tmp/.s.PGSQL.5433")),
            (PathBuf::from("/tmp"), 5433)
        );
    }
}
