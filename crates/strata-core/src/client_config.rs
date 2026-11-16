// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only access to the standard client configuration files
//! (`~/.pgpass`, `pg_service.conf`, `~/.my.cnf`) — see ADR-0014.
//!
//! Strata never writes these files and never logs their content.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::{ConnectionConfig, DbKind, Endpoint, PasswordPolicy, Secret, TlsMode};

/// Error while reading a client configuration file.
#[derive(Debug, thiserror::Error)]
pub enum ClientConfigError {
    /// The file is readable by other users; like libpq, it is ignored.
    #[error("{0} is readable by other users and is ignored (run: chmod 0600)")]
    InsecurePermissions(PathBuf),
    /// I/O error.
    #[error("cannot read {path}: {source}")]
    Io {
        /// File concerned.
        path: PathBuf,
        /// Underlying error.
        source: io::Error,
    },
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// Reads `path`; `Ok(None)` if it does not exist.
fn read_optional(path: &Path) -> Result<Option<String>, ClientConfigError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ClientConfigError::Io {
            path: path.to_owned(),
            source,
        }),
    }
}

/// Reads a file holding passwords, refusing it if group/others can read it.
fn read_private(path: &Path) -> Result<Option<String>, ClientConfigError> {
    match fs::metadata(path) {
        Ok(meta) if meta.permissions().mode() & 0o077 != 0 => {
            Err(ClientConfigError::InsecurePermissions(path.to_owned()))
        }
        Ok(_) => read_optional(path),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ClientConfigError::Io {
            path: path.to_owned(),
            source,
        }),
    }
}

// ---------------------------------------------------------------------------
// ~/.pgpass
// ---------------------------------------------------------------------------

/// One line of a `.pgpass` file. Fields may be the wildcard `*`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgPassEntry {
    /// Host pattern (`localhost` also matches Unix sockets).
    pub host: String,
    /// Port pattern.
    pub port: String,
    /// Database pattern.
    pub database: String,
    /// User pattern.
    pub user: String,
    /// Password.
    pub password: Secret,
}

/// Location of the `.pgpass` file (`$PGPASSFILE`, else `~/.pgpass`).
pub fn pgpass_path() -> Option<PathBuf> {
    std::env::var_os("PGPASSFILE")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".pgpass")))
}

/// Parses `.pgpass` content (libpq format, `\:` and `\\` escapes).
pub fn parse_pgpass(text: &str) -> Vec<PgPassEntry> {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| {
            let fields = split_escaped(line, ':');
            let [host, port, database, user, password]: [String; 5] = fields.try_into().ok()?;
            Some(PgPassEntry {
                host,
                port,
                database,
                user,
                password: Secret::new(password),
            })
        })
        .collect()
}

/// Splits on `sep`, honouring backslash escapes. The last field keeps any
/// further separators (passwords may contain `:` when escaped or not).
fn split_escaped(line: &str, sep: char) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let (Some(next), Some(field)) = (chars.next(), fields.last_mut()) {
                    field.push(next);
                }
            }
            c if c == sep && fields.len() < 5 => fields.push(String::new()),
            c => {
                if let Some(field) = fields.last_mut() {
                    field.push(c);
                }
            }
        }
    }
    fields
}

/// Finds the password for a PostgreSQL connection, with libpq rules.
pub fn pgpass_lookup<'a>(entries: &'a [PgPassEntry], cfg: &ConnectionConfig) -> Option<&'a Secret> {
    let (host, port) = match &cfg.endpoint {
        Endpoint::Tcp { host, port } => (host.as_str(), port.to_string()),
        Endpoint::Socket { path } => ("localhost", socket_port(path).to_string()),
    };
    // libpq defaults the database name to the user name.
    let database = cfg
        .database
        .as_deref()
        .filter(|d| !d.is_empty())
        .unwrap_or(&cfg.user);
    let matches = |pattern: &str, value: &str| pattern == "*" || pattern == value;
    entries
        .iter()
        .find(|e| {
            matches(&e.host, host)
                && matches(&e.port, &port)
                && matches(&e.database, database)
                && matches(&e.user, &cfg.user)
        })
        .map(|e| &e.password)
}

/// Port encoded in a PostgreSQL socket path (`.s.PGSQL.<port>`), else 5432.
pub fn socket_port(path: &Path) -> u16 {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix(".s.PGSQL."))
        .and_then(|p| p.parse().ok())
        .unwrap_or(5432)
}

/// Reads the `.pgpass` file; empty if it does not exist.
pub fn read_pgpass() -> Result<Vec<PgPassEntry>, ClientConfigError> {
    let Some(path) = pgpass_path() else {
        return Ok(Vec::new());
    };
    Ok(read_private(&path)?
        .map(|t| parse_pgpass(&t))
        .unwrap_or_default())
}

// ---------------------------------------------------------------------------
// INI files (pg_service.conf, my.cnf)
// ---------------------------------------------------------------------------

/// A section of an INI file: name and `key=value` pairs in file order.
type IniSection = (String, Vec<(String, String)>);

/// Minimal INI parser: `[section]`, `key = value`, `key` (no value),
/// `#`/`;` comments, optional quotes around values. `!include` is ignored.
fn parse_ini(text: &str) -> Vec<IniSection> {
    let mut sections: Vec<IniSection> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';', '!']) {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.trim().to_owned(), Vec::new()));
            continue;
        }
        let Some((_, entries)) = sections.last_mut() else {
            continue;
        };
        let (key, value) = match line.split_once('=') {
            Some((k, v)) => (k.trim(), unquote(v.trim())),
            None => (line, String::new()),
        };
        entries.push((key.to_ascii_lowercase().replace('_', "-"), value));
    }
    sections
}

fn unquote(value: &str) -> String {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|v| v.strip_suffix(quote))
        {
            return inner.to_owned();
        }
    }
    value.to_owned()
}

// ---------------------------------------------------------------------------
// pg_service.conf
// ---------------------------------------------------------------------------

/// A PostgreSQL connection service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgService {
    /// Service name.
    pub name: String,
    /// Parameters (`host`, `port`, `dbname`, `user`, `sslmode`…).
    /// A `password` parameter, if any, is dropped.
    pub params: BTreeMap<String, String>,
}

/// Candidate locations of the service file, in lookup order.
pub fn pg_service_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(p) = std::env::var_os("PGSERVICEFILE").filter(|p| !p.is_empty()) {
        paths.push(PathBuf::from(p));
    }
    if let Some(h) = home() {
        paths.push(h.join(".pg_service.conf"));
    }
    if let Some(dir) = std::env::var_os("PGSYSCONFDIR").filter(|p| !p.is_empty()) {
        paths.push(PathBuf::from(dir).join("pg_service.conf"));
    }
    paths.push(PathBuf::from("/etc/postgresql-common/pg_service.conf"));
    paths
}

/// Parses service file content.
pub fn parse_pg_service(text: &str) -> Vec<PgService> {
    parse_ini(text)
        .into_iter()
        .map(|(name, entries)| PgService {
            name,
            params: entries
                .into_iter()
                .filter(|(k, _)| k != "password")
                .collect(),
        })
        .collect()
}

/// Reads every service file; a service defined twice keeps its first
/// definition.
pub fn read_pg_services() -> Result<Vec<PgService>, ClientConfigError> {
    let mut services: Vec<PgService> = Vec::new();
    for path in pg_service_paths() {
        for service in read_optional(&path)?
            .map(|t| parse_pg_service(&t))
            .unwrap_or_default()
        {
            if !services.iter().any(|s| s.name == service.name) {
                services.push(service);
            }
        }
    }
    Ok(services)
}

impl PgService {
    /// Converts the service into connection settings.
    pub fn to_config(&self) -> ConnectionConfig {
        let mut cfg = ConnectionConfig::new(self.name.clone(), DbKind::Postgres);
        let port = self
            .params
            .get("port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(5432);
        let host = self
            .params
            .get("host")
            .or_else(|| self.params.get("hostaddr"))
            .cloned()
            .unwrap_or_else(|| "localhost".to_owned());
        cfg.endpoint = if host.starts_with('/') {
            Endpoint::Socket {
                path: if port == 5432 {
                    PathBuf::from(host)
                } else {
                    PathBuf::from(host).join(format!(".s.PGSQL.{port}"))
                },
            }
        } else {
            Endpoint::Tcp { host, port }
        };
        cfg.database = self.params.get("dbname").cloned();
        cfg.user = self
            .params
            .get("user")
            .cloned()
            .or_else(|| std::env::var("USER").ok())
            .unwrap_or_default();
        if let Some(mode) = self.params.get("sslmode") {
            cfg.tls.mode = match mode.as_str() {
                "disable" | "allow" => TlsMode::Disable,
                "require" => TlsMode::Require,
                "verify-ca" => TlsMode::VerifyCa,
                "verify-full" => TlsMode::VerifyFull,
                _ => TlsMode::Prefer,
            };
        }
        cfg.tls.ca_cert = self.params.get("sslrootcert").map(PathBuf::from);
        cfg.tls.client_cert = self.params.get("sslcert").map(PathBuf::from);
        cfg.tls.client_key = self.params.get("sslkey").map(PathBuf::from);
        cfg
    }
}

// ---------------------------------------------------------------------------
// ~/.my.cnf
// ---------------------------------------------------------------------------

/// Client options of a MySQL option file (`[client]` then `[mysql]`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MyCnf {
    /// Host.
    pub host: Option<String>,
    /// Port.
    pub port: Option<u16>,
    /// Unix socket.
    pub socket: Option<PathBuf>,
    /// User.
    pub user: Option<String>,
    /// Password.
    pub password: Option<Secret>,
    /// Default database.
    pub database: Option<String>,
}

/// Location of the MySQL option file.
pub fn my_cnf_path() -> Option<PathBuf> {
    home().map(|h| h.join(".my.cnf"))
}

/// Parses option file content; `[mysql]` overrides `[client]`.
pub fn parse_my_cnf(text: &str) -> MyCnf {
    let sections = parse_ini(text);
    let mut cnf = MyCnf::default();
    for wanted in ["client", "mysql"] {
        for (_, entries) in sections.iter().filter(|(name, _)| name == wanted) {
            for (key, value) in entries {
                match key.as_str() {
                    "host" => cnf.host = Some(value.clone()),
                    "port" => cnf.port = value.parse().ok(),
                    "socket" => cnf.socket = Some(PathBuf::from(value)),
                    "user" => cnf.user = Some(value.clone()),
                    "password" => cnf.password = Some(Secret::new(value.clone())),
                    "database" => cnf.database = Some(value.clone()),
                    _ => {}
                }
            }
        }
    }
    cnf
}

/// Reads `~/.my.cnf`; `None` if it does not exist.
pub fn read_my_cnf() -> Result<Option<MyCnf>, ClientConfigError> {
    let Some(path) = my_cnf_path() else {
        return Ok(None);
    };
    Ok(read_private(&path)?.map(|t| parse_my_cnf(&t)))
}

impl MyCnf {
    /// Whether the file defines anything useful to connect.
    pub fn is_empty(&self) -> bool {
        self.host.is_none() && self.socket.is_none() && self.user.is_none()
    }

    /// Converts the options into connection settings named `name`.
    pub fn to_config(&self, name: &str) -> ConnectionConfig {
        let mut cfg = ConnectionConfig::new(name, DbKind::MySql);
        cfg.endpoint = match (&self.socket, &self.host) {
            (Some(path), None) => Endpoint::Socket { path: path.clone() },
            (_, host) => Endpoint::Tcp {
                host: host.clone().unwrap_or_else(|| "localhost".to_owned()),
                port: self.port.unwrap_or(3306),
            },
        };
        cfg.user = self
            .user
            .clone()
            .or_else(|| std::env::var("USER").ok())
            .unwrap_or_default();
        cfg.database = self.database.clone();
        cfg.password_policy = if self.password.is_some() {
            PasswordPolicy::ClientConfigFile
        } else {
            PasswordPolicy::AskEveryTime
        };
        cfg
    }
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// Origin of an import candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportSource {
    /// A `pg_service.conf` service.
    PgService,
    /// A `.pgpass` line.
    PgPass,
    /// `~/.my.cnf`.
    MyCnf,
}

/// A connection that can be imported from the client configuration files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportCandidate {
    /// Proposed settings (never carries a password).
    pub config: ConnectionConfig,
    /// Origin.
    pub source: ImportSource,
}

/// Collects import candidates from every client configuration file.
///
/// Unreadable or insecure files are reported in the second element and
/// skipped; the other files are still used.
pub fn import_candidates() -> (Vec<ImportCandidate>, Vec<ClientConfigError>) {
    let mut errors = Vec::new();
    let pgpass = read_pgpass().unwrap_or_else(|e| {
        errors.push(e);
        Vec::new()
    });
    let services = read_pg_services().unwrap_or_else(|e| {
        errors.push(e);
        Vec::new()
    });
    let my_cnf = read_my_cnf().unwrap_or_else(|e| {
        errors.push(e);
        None
    });
    (candidates_from(&services, &pgpass, my_cnf.as_ref()), errors)
}

/// Builds candidates from already parsed files (pure, for tests).
pub fn candidates_from(
    services: &[PgService],
    pgpass: &[PgPassEntry],
    my_cnf: Option<&MyCnf>,
) -> Vec<ImportCandidate> {
    let mut out: Vec<ImportCandidate> = Vec::new();
    let with_pg_policy = |mut cfg: ConnectionConfig| {
        cfg.password_policy = if pgpass_lookup(pgpass, &cfg).is_some() {
            PasswordPolicy::ClientConfigFile
        } else {
            PasswordPolicy::AskEveryTime
        };
        cfg
    };
    for service in services {
        out.push(ImportCandidate {
            config: with_pg_policy(service.to_config()),
            source: ImportSource::PgService,
        });
    }
    for entry in pgpass {
        if entry.host == "*" || entry.user == "*" {
            continue;
        }
        let mut cfg =
            ConnectionConfig::new(format!("{}@{}", entry.user, entry.host), DbKind::Postgres);
        cfg.endpoint = Endpoint::Tcp {
            host: entry.host.clone(),
            port: entry.port.parse().unwrap_or(5432),
        };
        cfg.user = entry.user.clone();
        cfg.database = (entry.database != "*").then(|| entry.database.clone());
        cfg.password_policy = PasswordPolicy::ClientConfigFile;
        let duplicate = out.iter().any(|c| {
            c.config.kind == cfg.kind
                && c.config.endpoint == cfg.endpoint
                && c.config.user == cfg.user
                && c.config.database == cfg.database
        });
        if !duplicate {
            out.push(ImportCandidate {
                config: cfg,
                source: ImportSource::PgPass,
            });
        }
    }
    if let Some(cnf) = my_cnf.filter(|c| !c.is_empty()) {
        out.push(ImportCandidate {
            config: cnf.to_config("MySQL (~/.my.cnf)"),
            source: ImportSource::MyCnf,
        });
    }
    out
}

/// Password for `cfg` from the client configuration files (ADR-0014).
pub fn lookup_password(cfg: &ConnectionConfig) -> Result<Option<Secret>, ClientConfigError> {
    match cfg.kind {
        DbKind::Postgres => Ok(pgpass_lookup(&read_pgpass()?, cfg).cloned()),
        DbKind::MySql => Ok(read_my_cnf()?.and_then(|c| c.password)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pg(user: &str, host: &str, port: u16, db: Option<&str>) -> ConnectionConfig {
        let mut cfg = ConnectionConfig::new("t", DbKind::Postgres);
        cfg.user = user.to_owned();
        cfg.endpoint = Endpoint::Tcp {
            host: host.to_owned(),
            port,
        };
        cfg.database = db.map(str::to_owned);
        cfg
    }

    #[test]
    fn pgpass_parsing_and_escapes() {
        let entries = parse_pgpass(
            "# comment\n\
             db.example:5432:app:alice:s3cr\\:et\n\
             *:*:*:bob:pass:with:colons\n\
             broken:line\n",
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].password.expose(), "s3cr:et");
        assert_eq!(entries[1].host, "*");
        assert_eq!(entries[1].password.expose(), "pass:with:colons");
    }

    #[test]
    fn pgpass_lookup_follows_libpq_rules() {
        let entries = parse_pgpass(
            "db.example:5432:app:alice:first\n\
             *:*:*:alice:fallback\n\
             localhost:5433:*:carol:sock\n",
        );
        let found =
            |cfg: &ConnectionConfig| pgpass_lookup(&entries, cfg).map(|s| s.expose().to_owned());
        assert_eq!(
            found(&pg("alice", "db.example", 5432, Some("app"))).as_deref(),
            Some("first")
        );
        assert_eq!(
            found(&pg("alice", "other", 5432, Some("app"))).as_deref(),
            Some("fallback")
        );
        assert_eq!(found(&pg("dave", "db.example", 5432, None)), None);

        let mut socket = pg("carol", "", 0, None);
        socket.endpoint = Endpoint::Socket {
            path: "/run/postgresql/.s.PGSQL.5433".into(),
        };
        assert_eq!(found(&socket).as_deref(), Some("sock"));
    }

    #[test]
    fn database_defaults_to_user_name() {
        let entries = parse_pgpass("h:5432:alice:alice:pw\n");
        assert!(pgpass_lookup(&entries, &pg("alice", "h", 5432, None)).is_some());
    }

    #[test]
    fn insecure_pgpass_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("pgpass");
        fs::write(&path, "h:1:d:u:p\n").expect("write");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("chmod");
        assert!(matches!(
            read_private(&path),
            Err(ClientConfigError::InsecurePermissions(_))
        ));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("chmod");
        assert!(read_private(&path).expect("read").is_some());
        assert!(
            read_private(&dir.path().join("missing"))
                .expect("read")
                .is_none()
        );
    }

    #[test]
    fn pg_service_conversion() {
        let services = parse_pg_service(
            "[prod]\nhost=db.example\nport=6432\ndbname=shop\nuser=app\nsslmode=verify-full\npassword=dropped\n\
             [local]\nhost=/run/postgresql\nport=5433\nuser=me\n",
        );
        assert_eq!(services.len(), 2);
        assert!(!services[0].params.contains_key("password"));
        let prod = services[0].to_config();
        assert_eq!(prod.name, "prod");
        assert_eq!(
            prod.endpoint,
            Endpoint::Tcp {
                host: "db.example".into(),
                port: 6432
            }
        );
        assert_eq!(prod.tls.mode, TlsMode::VerifyFull);
        assert_eq!(prod.database.as_deref(), Some("shop"));
        let local = services[1].to_config();
        assert_eq!(
            local.endpoint,
            Endpoint::Socket {
                path: "/run/postgresql/.s.PGSQL.5433".into()
            }
        );
    }

    #[test]
    fn my_cnf_mysql_section_overrides_client() {
        let cnf = parse_my_cnf(
            "[client]\nuser = root\npassword = \"p w\"\nport=3307\n\
             [mysqldump]\nuser = ignored\n\
             [mysql]\nuser=app\ndatabase=shop\nskip-auto-rehash\n",
        );
        assert_eq!(cnf.user.as_deref(), Some("app"));
        assert_eq!(cnf.password.as_ref().map(Secret::expose), Some("p w"));
        assert_eq!(cnf.port, Some(3307));
        let cfg = cnf.to_config("MySQL");
        assert_eq!(cfg.password_policy, PasswordPolicy::ClientConfigFile);
        assert_eq!(
            cfg.endpoint,
            Endpoint::Tcp {
                host: "localhost".into(),
                port: 3307
            }
        );
    }

    #[test]
    fn candidates_skip_wildcards_and_duplicates() {
        let services = parse_pg_service("[svc]\nhost=h\nport=5432\nuser=u\ndbname=d\n");
        let pgpass = parse_pgpass("h:5432:d:u:pw\n*:*:*:x:pw\nother:5432:*:v:pw\n");
        let out = candidates_from(&services, &pgpass, None);
        let names: Vec<_> = out.iter().map(|c| c.config.name.as_str()).collect();
        assert_eq!(names, ["svc", "v@other"]);
        assert_eq!(
            out[0].config.password_policy,
            PasswordPolicy::ClientConfigFile
        );
        assert_eq!(out[1].config.database, None);
    }
}
