// SPDX-License-Identifier: GPL-3.0-or-later
//! Saved connections (`connections` table).

use std::path::PathBuf;

use rusqlite::{OptionalExtension, Row, named_params};
use strata_core::{
    ConnectionConfig, ConnectionId, Endpoint, PasswordPolicy, SshAuth, SshTunnel, TlsConfig,
};

use crate::{Result, Store, StoreError};

const COLUMNS: &str = "id, name, kind, host, port, socket_path, database, username, \
     tls_mode, tls_ca, tls_cert, tls_key, environment, color, group_id, read_only, \
     password_policy, record_history, ssh_host, ssh_port, ssh_user, ssh_auth, ssh_key_path, \
     ssh_secret_policy";

impl Store {
    /// Every saved connection, sorted by name.
    pub fn list_connections(&self) -> Result<Vec<ConnectionConfig>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM connections ORDER BY name COLLATE NOCASE, id"
        ))?;
        let rows = stmt.query_map([], |row| Ok(decode(row)))?;
        rows.map(|r| r?).collect()
    }

    /// The connection `id`, if it exists.
    pub fn get_connection(&self, id: ConnectionId) -> Result<Option<ConnectionConfig>> {
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM connections WHERE id = ?1"),
                [id.to_string()],
                |row| Ok(decode(row)),
            )
            .optional()?
            .transpose()
    }

    /// Inserts or updates a connection.
    ///
    /// `secret_ref` records that a keyring item exists for the connection
    /// (CDC §11); the password itself is never stored here.
    pub fn save_connection(&self, cfg: &ConnectionConfig) -> Result<()> {
        let (host, port, socket) = match &cfg.endpoint {
            Endpoint::Tcp { host, port } => (Some(host.as_str()), Some(*port), None),
            Endpoint::Socket { path } => (None, None, Some(path_str(path))),
        };
        let secret_ref =
            (cfg.password_policy == PasswordPolicy::Keyring).then(|| cfg.id.to_string());
        let ssh = cfg.ssh.as_ref();
        self.conn.execute(
            "INSERT INTO connections (id, name, kind, host, port, socket_path, database, \
                 username, tls_mode, tls_ca, tls_cert, tls_key, environment, color, group_id, \
                 read_only, password_policy, record_history, secret_ref, ssh_host, ssh_port, \
                 ssh_user, ssh_auth, ssh_key_path, ssh_secret_policy)
             VALUES (:id, :name, :kind, :host, :port, :socket, :database, :user, :tls_mode, \
                 :tls_ca, :tls_cert, :tls_key, :environment, :color, :group_id, :read_only, \
                 :policy, :history, :secret_ref, :ssh_host, :ssh_port, :ssh_user, :ssh_auth, \
                 :ssh_key_path, :ssh_secret_policy)
             ON CONFLICT (id) DO UPDATE SET
                 name = excluded.name, kind = excluded.kind, host = excluded.host,
                 port = excluded.port, socket_path = excluded.socket_path,
                 database = excluded.database, username = excluded.username,
                 tls_mode = excluded.tls_mode, tls_ca = excluded.tls_ca,
                 tls_cert = excluded.tls_cert, tls_key = excluded.tls_key,
                 environment = excluded.environment, color = excluded.color,
                 group_id = excluded.group_id, read_only = excluded.read_only,
                 password_policy = excluded.password_policy,
                 record_history = excluded.record_history, secret_ref = excluded.secret_ref,
                 ssh_host = excluded.ssh_host, ssh_port = excluded.ssh_port,
                 ssh_user = excluded.ssh_user, ssh_auth = excluded.ssh_auth,
                 ssh_key_path = excluded.ssh_key_path,
                 ssh_secret_policy = excluded.ssh_secret_policy,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
            named_params! {
                ":id": cfg.id.to_string(),
                ":name": cfg.name,
                ":kind": cfg.kind.as_str(),
                ":host": host,
                ":port": port,
                ":socket": socket,
                ":database": cfg.database,
                ":user": cfg.user,
                ":tls_mode": cfg.tls.mode.as_str(),
                ":tls_ca": cfg.tls.ca_cert.as_deref().map(path_str),
                ":tls_cert": cfg.tls.client_cert.as_deref().map(path_str),
                ":tls_key": cfg.tls.client_key.as_deref().map(path_str),
                ":environment": cfg.environment.map(|e| e.as_str()),
                ":color": cfg.color,
                ":group_id": cfg.group_id,
                ":read_only": cfg.read_only,
                ":policy": cfg.password_policy.as_str(),
                ":history": cfg.record_history,
                ":secret_ref": secret_ref,
                ":ssh_host": ssh.map(|t| t.host.as_str()),
                ":ssh_port": ssh.map(|t| t.port),
                ":ssh_user": ssh.map(|t| t.user.as_str()),
                ":ssh_auth": ssh.map(|t| t.auth.as_str()),
                ":ssh_key_path": ssh.and_then(|t| match &t.auth {
                    SshAuth::PrivateKey { path } => Some(path_str(path)),
                    _ => None,
                }),
                ":ssh_secret_policy": ssh.map(|t| t.secret_policy.as_str()),
            },
        )?;
        Ok(())
    }

    /// Deletes a connection; returns whether it existed.
    pub fn delete_connection(&self, id: ConnectionId) -> Result<bool> {
        let n = self
            .conn
            .execute("DELETE FROM connections WHERE id = ?1", [id.to_string()])?;
        Ok(n > 0)
    }
}

fn path_str(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

fn corrupt(what: &str, err: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt(format!("{what}: {err}"))
}

/// Decodes a row selected with [`COLUMNS`].
fn decode(row: &Row<'_>) -> Result<ConnectionConfig> {
    let id: String = row.get(0)?;
    let kind: String = row.get(2)?;
    let host: Option<String> = row.get(3)?;
    let port: Option<u16> = row.get(4)?;
    let socket: Option<String> = row.get(5)?;
    let tls_mode: String = row.get(8)?;
    let environment: Option<String> = row.get(12)?;
    let policy: String = row.get(16)?;

    let endpoint = match (host, port, socket) {
        (Some(host), Some(port), _) => Endpoint::Tcp { host, port },
        (_, _, Some(path)) => Endpoint::Socket {
            path: PathBuf::from(path),
        },
        _ => return Err(StoreError::Corrupt("connection without endpoint".into())),
    };

    Ok(ConnectionConfig {
        id: id.parse().map_err(|e| corrupt("connection id", e))?,
        name: row.get(1)?,
        kind: kind.parse().map_err(|e| corrupt("kind", e))?,
        endpoint,
        database: row.get(6)?,
        user: row.get(7)?,
        tls: TlsConfig {
            mode: tls_mode.parse().map_err(|e| corrupt("tls_mode", e))?,
            ca_cert: row.get::<_, Option<String>>(9)?.map(PathBuf::from),
            client_cert: row.get::<_, Option<String>>(10)?.map(PathBuf::from),
            client_key: row.get::<_, Option<String>>(11)?.map(PathBuf::from),
        },
        environment: environment
            .map(|e| e.parse().map_err(|err| corrupt("environment", err)))
            .transpose()?,
        color: row.get(13)?,
        group_id: row.get(14)?,
        read_only: row.get(15)?,
        password_policy: policy.parse().map_err(|e| corrupt("password_policy", e))?,
        record_history: row.get(17)?,
        ssh: decode_ssh(row)?,
    })
}

fn decode_ssh(row: &Row<'_>) -> Result<Option<SshTunnel>> {
    let Some(host) = row.get::<_, Option<String>>(18)? else {
        return Ok(None);
    };
    let auth: Option<String> = row.get(21)?;
    let key: Option<String> = row.get(22)?;
    let auth = match (auth.as_deref(), key) {
        (Some("key"), Some(path)) => SshAuth::PrivateKey {
            path: PathBuf::from(path),
        },
        (Some("password"), _) => SshAuth::Password,
        (Some("agent"), _) => SshAuth::Agent,
        (other, _) => return Err(corrupt("ssh_auth", format!("{other:?}"))),
    };
    let policy: Option<String> = row.get(23)?;
    Ok(Some(SshTunnel {
        host,
        port: row
            .get::<_, Option<u16>>(19)?
            .unwrap_or(SshTunnel::DEFAULT_PORT),
        user: row.get::<_, Option<String>>(20)?.unwrap_or_default(),
        auth,
        secret_policy: policy
            .as_deref()
            .unwrap_or("none")
            .parse()
            .map_err(|e| corrupt("ssh_secret_policy", e))?,
    }))
}

#[cfg(test)]
mod tests {
    use strata_core::{DbKind, Environment, TlsMode};

    use super::*;

    fn sample() -> ConnectionConfig {
        let mut cfg = ConnectionConfig::new("Shop (prod)", DbKind::Postgres);
        cfg.user = "app".into();
        cfg.database = Some("shop".into());
        cfg.environment = Some(Environment::Production);
        cfg.color = Some("#e01b24".into());
        cfg.read_only = true;
        cfg.tls = TlsConfig {
            mode: TlsMode::VerifyFull,
            ca_cert: Some("/etc/ssl/ca.pem".into()),
            client_cert: None,
            client_key: None,
        };
        cfg
    }

    #[test]
    fn round_trip() {
        let store = Store::open_in_memory().expect("store");
        let tcp = sample();
        let mut socket = ConnectionConfig::new("Local socket", DbKind::MySql);
        socket.user = "me".into();
        socket.endpoint = Endpoint::Socket {
            path: "/run/mysqld/mysqld.sock".into(),
        };
        socket.password_policy = PasswordPolicy::AskEveryTime;
        store.save_connection(&tcp).expect("save");
        store.save_connection(&socket).expect("save");

        assert_eq!(
            store.get_connection(tcp.id).expect("get"),
            Some(tcp.clone())
        );
        let names: Vec<_> = store
            .list_connections()
            .expect("list")
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, ["Local socket", "Shop (prod)"]);
        assert_eq!(store.get_connection(socket.id).expect("get"), Some(socket));
    }

    #[test]
    fn ssh_tunnel_round_trip() {
        let store = Store::open_in_memory().expect("store");
        let mut cfg = sample();
        cfg.ssh = Some(SshTunnel {
            host: "bastion.example".into(),
            port: 2222,
            user: "ops".into(),
            auth: SshAuth::PrivateKey {
                path: "/home/me/.ssh/id_ed25519".into(),
            },
            secret_policy: PasswordPolicy::AskEveryTime,
        });
        store.save_connection(&cfg).expect("save");
        assert_eq!(
            store.get_connection(cfg.id).expect("get"),
            Some(cfg.clone())
        );
        cfg.ssh = None;
        store.save_connection(&cfg).expect("update");
        assert_eq!(store.get_connection(cfg.id).expect("get"), Some(cfg));
    }

    #[test]
    fn update_and_delete() {
        let store = Store::open_in_memory().expect("store");
        let mut cfg = sample();
        store.save_connection(&cfg).expect("save");
        cfg.name = "Renamed".into();
        cfg.environment = None;
        store.save_connection(&cfg).expect("update");
        assert_eq!(store.list_connections().expect("list"), vec![cfg.clone()]);

        assert!(store.delete_connection(cfg.id).expect("delete"));
        assert!(!store.delete_connection(cfg.id).expect("delete again"));
        assert_eq!(store.get_connection(cfg.id).expect("get"), None);
    }

    #[test]
    fn secret_ref_only_for_keyring_policy() {
        let store = Store::open_in_memory().expect("store");
        let mut cfg = sample();
        store.save_connection(&cfg).expect("save");
        let secret_ref = |store: &Store| -> Option<String> {
            store
                .conn
                .query_row("SELECT secret_ref FROM connections", [], |r| r.get(0))
                .expect("query")
        };
        assert_eq!(secret_ref(&store), Some(cfg.id.to_string()));
        cfg.password_policy = PasswordPolicy::AskEveryTime;
        store.save_connection(&cfg).expect("save");
        assert_eq!(secret_ref(&store), None);
    }
}
