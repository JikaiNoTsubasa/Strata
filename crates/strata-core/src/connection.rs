// SPDX-License-Identifier: GPL-3.0-or-later
//! Saved connection settings.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use uuid::Uuid;

/// Error returned when parsing an enumeration from its storage name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown {what} `{value}`")]
pub struct ParseEnumError {
    what: &'static str,
    value: String,
}

/// Implements `as_str`, `ALL`, `Display` and `FromStr` for a fieldless enum
/// with stable storage names.
macro_rules! storage_enum {
    ($ty:ident, $what:literal, { $($variant:ident => $name:literal),+ $(,)? }) => {
        impl $ty {
            /// Every variant, in display order.
            pub const ALL: &'static [$ty] = &[$($ty::$variant),+];

            /// Stable name used in storage.
            pub fn as_str(self) -> &'static str {
                match self { $($ty::$variant => $name),+ }
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $ty {
            type Err = ParseEnumError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($name => Ok($ty::$variant),)+
                    _ => Err(ParseEnumError { what: $what, value: s.to_owned() }),
                }
            }
        }
    };
}

/// Unique identifier of a saved connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnectionId(Uuid);

impl ConnectionId {
    /// Generates a new random identifier.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ConnectionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for ConnectionId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self)
    }
}

/// Database product family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DbKind {
    /// PostgreSQL.
    Postgres,
    /// MySQL or MariaDB.
    MySql,
}

storage_enum!(DbKind, "database kind", { Postgres => "postgres", MySql => "mysql" });

impl DbKind {
    /// Default TCP port of the product.
    pub fn default_port(self) -> u16 {
        match self {
            Self::Postgres => 5432,
            Self::MySql => 3306,
        }
    }
}

/// How to reach the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// TCP connection.
    Tcp {
        /// Host name or IP address.
        host: String,
        /// TCP port.
        port: u16,
    },
    /// Unix domain socket.
    ///
    /// For PostgreSQL, either the socket directory (e.g. `/run/postgresql`,
    /// port 5432 assumed) or the socket file itself (`…/.s.PGSQL.5433`).
    /// For MySQL/MariaDB, the socket file (e.g. `/run/mysqld/mysqld.sock`).
    Socket {
        /// Absolute path of the socket (or of its directory, PostgreSQL only).
        path: PathBuf,
    },
}

impl Endpoint {
    /// Whether the server runs on this machine (socket or loopback address).
    ///
    /// Used to warn about unencrypted connections to remote hosts.
    pub fn is_local(&self) -> bool {
        match self {
            Self::Socket { .. } => true,
            Self::Tcp { host, .. } => {
                let host = host.trim().trim_start_matches('[').trim_end_matches(']');
                host.eq_ignore_ascii_case("localhost")
                    || host
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            }
        }
    }
}

/// TLS mode, with libpq semantics (CON-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TlsMode {
    /// Never use TLS.
    Disable,
    /// Use TLS if the server supports it, without verifying the certificate.
    #[default]
    Prefer,
    /// Require TLS, without verifying the certificate.
    Require,
    /// Require TLS and verify the certificate chain, not the host name.
    VerifyCa,
    /// Require TLS and verify the certificate chain and the host name.
    VerifyFull,
}

storage_enum!(TlsMode, "TLS mode", {
    Disable => "disable",
    Prefer => "prefer",
    Require => "require",
    VerifyCa => "verify-ca",
    VerifyFull => "verify-full",
});

/// TLS settings of a connection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TlsConfig {
    /// TLS mode.
    pub mode: TlsMode,
    /// CA certificate (PEM) used for verification; system roots if `None`.
    pub ca_cert: Option<PathBuf>,
    /// Client certificate (PEM) for mutual TLS.
    pub client_cert: Option<PathBuf>,
    /// Private key (PEM) of the client certificate.
    pub client_key: Option<PathBuf>,
}

/// Environment label (CON-8). Production enables extra safeguards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Environment {
    /// Development.
    Development,
    /// Staging / pre-production.
    Staging,
    /// Production.
    Production,
}

storage_enum!(Environment, "environment", {
    Development => "dev",
    Staging => "staging",
    Production => "prod",
});

/// Where the password comes from (CON-2, ADR-0014).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PasswordPolicy {
    /// Stored in the system keyring.
    #[default]
    Keyring,
    /// Asked at every connection, kept in memory only for the session.
    AskEveryTime,
    /// Read from `~/.pgpass` (PostgreSQL) or `~/.my.cnf` (MySQL) at each
    /// connection; never copied.
    ClientConfigFile,
    /// No password (peer/trust authentication, client certificate).
    None,
}

storage_enum!(PasswordPolicy, "password policy", {
    Keyring => "keyring",
    AskEveryTime => "ask",
    ClientConfigFile => "client-config",
    None => "none",
});

/// How to authenticate on the SSH server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshAuth {
    /// Keys of the running SSH agent (`SSH_AUTH_SOCK`).
    Agent,
    /// Password.
    Password,
    /// Private key file, optionally protected by a passphrase.
    PrivateKey {
        /// Path of the key (OpenSSH or PEM format).
        path: PathBuf,
    },
}

impl SshAuth {
    /// Stable name used in storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Password => "password",
            Self::PrivateKey { .. } => "key",
        }
    }
}

/// SSH tunnel used to reach the database server.
///
/// The database host and port are resolved from the SSH server, like
/// `ssh -L`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTunnel {
    /// SSH server.
    pub host: String,
    /// SSH port.
    pub port: u16,
    /// SSH user.
    pub user: String,
    /// Authentication method.
    pub auth: SshAuth,
    /// Where the SSH password or key passphrase comes from (`Keyring`,
    /// `AskEveryTime` or `None`).
    pub secret_policy: PasswordPolicy,
}

impl SshTunnel {
    /// Default SSH port.
    pub const DEFAULT_PORT: u16 = 22;

    /// `user@host[:port]`.
    pub fn summary(&self) -> String {
        if self.port == Self::DEFAULT_PORT {
            format!("{}@{}", self.user, self.host)
        } else {
            format!("{}@{}:{}", self.user, self.host, self.port)
        }
    }
}

/// A saved connection. Never contains a password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionConfig {
    /// Identifier.
    pub id: ConnectionId,
    /// Display name.
    pub name: String,
    /// Database type.
    pub kind: DbKind,
    /// Server address.
    pub endpoint: Endpoint,
    /// Initial database; the server default if `None`.
    pub database: Option<String>,
    /// User name.
    pub user: String,
    /// TLS settings.
    pub tls: TlsConfig,
    /// Environment label.
    pub environment: Option<Environment>,
    /// Label colour, `#rrggbb`.
    pub color: Option<String>,
    /// Group (folder) the connection belongs to.
    pub group_id: Option<i64>,
    /// Read-only connection (CON-12).
    pub read_only: bool,
    /// Password source.
    pub password_policy: PasswordPolicy,
    /// Whether executed queries are recorded in the history (HIS-4).
    pub record_history: bool,
    /// SSH tunnel, if the server is reached through one.
    pub ssh: Option<SshTunnel>,
}

/// A problem found by [`ConnectionConfig::validate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationError {
    /// The name is empty.
    EmptyName,
    /// The host is empty.
    EmptyHost,
    /// The port is 0.
    InvalidPort,
    /// The socket path is not absolute.
    RelativeSocketPath,
    /// The user name is empty.
    EmptyUser,
    /// The colour is not `#rrggbb`.
    InvalidColor,
    /// A client certificate was given without its key, or the reverse.
    IncompleteClientCertificate,
    /// The SSH host is empty.
    EmptySshHost,
    /// The SSH user is empty.
    EmptySshUser,
    /// The SSH port is 0.
    InvalidSshPort,
    /// The SSH private key path is not absolute.
    RelativeSshKeyPath,
    /// SSH tunnels only reach TCP endpoints, not Unix sockets.
    SshWithSocket,
}

impl ConnectionConfig {
    /// A new PostgreSQL connection to `localhost` with default settings.
    pub fn new(name: impl Into<String>, kind: DbKind) -> Self {
        Self {
            id: ConnectionId::new(),
            name: name.into(),
            kind,
            endpoint: Endpoint::Tcp {
                host: "localhost".to_owned(),
                port: kind.default_port(),
            },
            database: None,
            user: String::new(),
            tls: TlsConfig::default(),
            environment: None,
            color: None,
            group_id: None,
            read_only: false,
            password_policy: PasswordPolicy::default(),
            record_history: true,
            ssh: None,
        }
    }

    /// Checks the settings; returns every problem found.
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();
        if self.name.trim().is_empty() {
            errors.push(ValidationError::EmptyName);
        }
        match &self.endpoint {
            Endpoint::Tcp { host, port } => {
                if host.trim().is_empty() {
                    errors.push(ValidationError::EmptyHost);
                }
                if *port == 0 {
                    errors.push(ValidationError::InvalidPort);
                }
            }
            Endpoint::Socket { path } => {
                if !path.is_absolute() {
                    errors.push(ValidationError::RelativeSocketPath);
                }
            }
        }
        if self.user.trim().is_empty() {
            errors.push(ValidationError::EmptyUser);
        }
        if self.color.as_deref().is_some_and(|c| !is_hex_color(c)) {
            errors.push(ValidationError::InvalidColor);
        }
        if self.tls.client_cert.is_some() != self.tls.client_key.is_some() {
            errors.push(ValidationError::IncompleteClientCertificate);
        }
        if let Some(ssh) = &self.ssh {
            if ssh.host.trim().is_empty() {
                errors.push(ValidationError::EmptySshHost);
            }
            if ssh.user.trim().is_empty() {
                errors.push(ValidationError::EmptySshUser);
            }
            if ssh.port == 0 {
                errors.push(ValidationError::InvalidSshPort);
            }
            if matches!(&ssh.auth, SshAuth::PrivateKey { path } if !path.is_absolute()) {
                errors.push(ValidationError::RelativeSshKeyPath);
            }
            if matches!(self.endpoint, Endpoint::Socket { .. }) {
                errors.push(ValidationError::SshWithSocket);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Short human-readable target, e.g. `alice@db.example:5432/app`.
    pub fn summary(&self) -> String {
        let target = match &self.endpoint {
            Endpoint::Tcp { host, port } => format!("{host}:{port}"),
            Endpoint::Socket { path } => path.display().to_string(),
        };
        let mut summary = match &self.database {
            Some(db) if !db.is_empty() => format!("{}@{target}/{db}", self.user),
            _ => format!("{}@{target}", self.user),
        };
        if let Some(ssh) = &self.ssh {
            summary.push_str(" via ");
            summary.push_str(&ssh.host);
        }
        summary
    }

    /// Whether this connection is labelled as production.
    pub fn is_production(&self) -> bool {
        self.environment == Some(Environment::Production)
    }
}

fn is_hex_color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> ConnectionConfig {
        let mut cfg = ConnectionConfig::new("Local", DbKind::Postgres);
        cfg.user = "app".to_owned();
        cfg
    }

    #[test]
    fn default_config_with_user_is_valid() {
        assert_eq!(valid().validate(), Ok(()));
    }

    #[test]
    fn validation_reports_every_problem() {
        let mut cfg = valid();
        cfg.name = "  ".to_owned();
        cfg.user.clear();
        cfg.endpoint = Endpoint::Tcp {
            host: String::new(),
            port: 0,
        };
        cfg.color = Some("red".to_owned());
        cfg.tls.client_cert = Some("/tmp/c.pem".into());
        assert_eq!(
            cfg.validate(),
            Err(vec![
                ValidationError::EmptyName,
                ValidationError::EmptyHost,
                ValidationError::InvalidPort,
                ValidationError::EmptyUser,
                ValidationError::InvalidColor,
                ValidationError::IncompleteClientCertificate,
            ])
        );
    }

    #[test]
    fn socket_path_must_be_absolute() {
        let mut cfg = valid();
        cfg.endpoint = Endpoint::Socket {
            path: "run/postgresql".into(),
        };
        assert_eq!(
            cfg.validate(),
            Err(vec![ValidationError::RelativeSocketPath])
        );
    }

    #[test]
    fn locality() {
        let tcp = |host: &str| Endpoint::Tcp {
            host: host.to_owned(),
            port: 5432,
        };
        assert!(tcp("localhost").is_local());
        assert!(tcp("127.0.0.1").is_local());
        assert!(tcp("[::1]").is_local());
        assert!(!tcp("db.example.org").is_local());
        assert!(!tcp("10.0.0.5").is_local());
        assert!(
            Endpoint::Socket {
                path: "/run/postgresql".into()
            }
            .is_local()
        );
    }

    #[test]
    fn enums_round_trip_through_storage_names() {
        for mode in TlsMode::ALL {
            assert_eq!(mode.as_str().parse::<TlsMode>(), Ok(*mode));
        }
        for policy in PasswordPolicy::ALL {
            assert_eq!(policy.as_str().parse::<PasswordPolicy>(), Ok(*policy));
        }
        for env in Environment::ALL {
            assert_eq!(env.as_str().parse::<Environment>(), Ok(*env));
        }
        assert!("oracle".parse::<DbKind>().is_err());
    }

    #[test]
    fn ssh_validation() {
        let mut cfg = valid();
        cfg.ssh = Some(SshTunnel {
            host: "bastion".into(),
            port: 22,
            user: "me".into(),
            auth: SshAuth::Agent,
            secret_policy: PasswordPolicy::None,
        });
        assert_eq!(cfg.validate(), Ok(()));
        assert_eq!(cfg.summary(), "app@localhost:5432 via bastion");
        cfg.endpoint = Endpoint::Socket {
            path: "/run/postgresql".into(),
        };
        if let Some(ssh) = cfg.ssh.as_mut() {
            ssh.host.clear();
            ssh.auth = SshAuth::PrivateKey {
                path: "id_ed25519".into(),
            };
        }
        assert_eq!(
            cfg.validate(),
            Err(vec![
                ValidationError::EmptySshHost,
                ValidationError::RelativeSshKeyPath,
                ValidationError::SshWithSocket,
            ])
        );
    }

    #[test]
    fn summary_formats() {
        let mut cfg = valid();
        assert_eq!(cfg.summary(), "app@localhost:5432");
        cfg.database = Some("shop".to_owned());
        assert_eq!(cfg.summary(), "app@localhost:5432/shop");
    }
}
