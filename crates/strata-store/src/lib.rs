// SPDX-License-Identifier: GPL-3.0-or-later
//! Internal SQLite storage (see `docs/ADR/0009-internal-storage-sqlite.md`).
//!
//! The database lives in `$XDG_DATA_HOME/strata/strata.db`, created with
//! mode `0600` in a `0700` directory. The API is synchronous; the engine
//! calls it from blocking tasks.

mod connections;

use std::fs::{self, DirBuilder, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use rusqlite::Connection;

/// Ordered schema migrations; the schema version is the number applied
/// (`PRAGMA user_version`).
const MIGRATIONS: &[&str] = &[
    include_str!("migrations/0001_initial.sql"),
    include_str!("migrations/0002_ssh_tunnel.sql"),
];

/// Storage error.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// SQLite error.
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// File system error.
    #[error("file error: {0}")]
    Io(#[from] io::Error),
    /// The database was created by a newer Strata version.
    #[error("database schema version {found} is newer than supported version {supported}")]
    TooNew {
        /// Version found in the file.
        found: usize,
        /// Latest version known by this build.
        supported: usize,
    },
    /// A stored row cannot be decoded.
    #[error("invalid stored data: {0}")]
    Corrupt(String),
    /// No data directory could be determined.
    #[error("cannot determine the data directory (HOME is not set)")]
    NoDataDir,
}

/// Result alias for this crate.
pub type Result<T, E = StoreError> = std::result::Result<T, E>;

/// Handle on the internal database.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Default database path: `$XDG_DATA_HOME/strata/strata.db`.
    pub fn default_path() -> Result<PathBuf> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|p| !p.is_empty())
                    .map(|h| PathBuf::from(h).join(".local/share"))
            })
            .ok_or(StoreError::NoDataDir)?;
        Ok(base.join("strata").join("strata.db"))
    }

    /// Opens (creating it if needed) the database at `path` and migrates it.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
            // An existing directory keeps its mode; tighten it.
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
        // Create the file ourselves so SQLite never creates it with the umask.
        OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Self::init(Connection::open(path)?)
    }

    /// Opens a private in-memory database (tests).
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "secure_delete", true)?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    /// Current schema version.
    pub fn schema_version(&self) -> Result<usize> {
        let version: i64 = self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        usize::try_from(version).map_err(|_| StoreError::Corrupt("negative user_version".into()))
    }

    fn migrate(&mut self) -> Result<()> {
        let current = self.schema_version()?;
        if current > MIGRATIONS.len() {
            return Err(StoreError::TooNew {
                found: current,
                supported: MIGRATIONS.len(),
            });
        }
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            let version = i64::try_from(index + 1)
                .map_err(|_| StoreError::Corrupt("schema version".into()))?;
            tx.pragma_update(None, "user_version", version)?;
            tx.commit()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_private_files_and_migrates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("sub").join("strata.db");
        let store = Store::open(&path).expect("open");
        assert_eq!(store.schema_version().expect("version"), MIGRATIONS.len());

        let mode = |p: &Path| fs::metadata(p).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().expect("parent")), 0o700);
        drop(store);

        // Re-opening is idempotent.
        let store = Store::open(&path).expect("reopen");
        assert_eq!(store.schema_version().expect("version"), MIGRATIONS.len());
    }

    #[test]
    fn refuses_newer_schema() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("strata.db");
        drop(Store::open(&path).expect("open"));
        let conn = Connection::open(&path).expect("raw open");
        conn.pragma_update(None, "user_version", 99).expect("bump");
        drop(conn);
        assert!(matches!(
            Store::open(&path),
            Err(StoreError::TooNew { found: 99, .. })
        ));
    }
}
