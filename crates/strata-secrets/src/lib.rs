// SPDX-License-Identifier: GPL-3.0-or-later
//! Password storage (see `docs/ADR/0004-secrets-storage.md`).
//!
//! [`KeyringStore`] uses the Secret Service on the host, or the Secret portal
//! inside Flatpak (through `oo7`). [`MemoryStore`] keeps secrets in memory
//! only: it backs the tests and the "ask every time" policy.

use std::collections::HashMap;

use async_trait::async_trait;
use strata_core::{ConnectionId, Secret};
use tokio::sync::{Mutex, OnceCell};

/// Attribute identifying Strata's items in the keyring.
const ATTR_APPLICATION: &str = "application";
/// Attribute holding the connection identifier.
const ATTR_CONNECTION: &str = "connection-id";
/// Attribute distinguishing the secrets of a connection. Items saved before
/// it existed have none and are database passwords.
const ATTR_KIND: &str = "secret-kind";

/// Which secret of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretKind {
    /// Database password.
    Database,
    /// SSH password or private key passphrase.
    Ssh,
}

impl SecretKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Database => "database",
            Self::Ssh => "ssh",
        }
    }
}

/// Secret storage error.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// No keyring service is reachable; passwords cannot be stored.
    #[error("the system keyring is unavailable: {0}")]
    Unavailable(String),
    /// The keyring is locked and the user did not unlock it.
    #[error("the system keyring is locked")]
    Locked,
    /// Any other keyring failure.
    #[error("keyring error: {0}")]
    Backend(String),
}

/// Result alias for this crate.
pub type Result<T, E = SecretError> = std::result::Result<T, E>;

/// Storage of connection passwords.
#[async_trait]
pub trait SecretStore: Send + Sync {
    /// Whether passwords can be stored (CDC §10.1).
    async fn is_available(&self) -> bool;
    /// Secret `kind` of connection `id`, if stored.
    async fn get(&self, id: ConnectionId, kind: SecretKind) -> Result<Option<Secret>>;
    /// Stores (or replaces) secret `kind` of connection `id`.
    /// `label` is shown by keyring managers such as Seahorse.
    async fn set(
        &self,
        id: ConnectionId,
        kind: SecretKind,
        label: &str,
        secret: &Secret,
    ) -> Result<()>;
    /// Deletes secret `kind` of connection `id` (no error if absent).
    async fn delete(&self, id: ConnectionId, kind: SecretKind) -> Result<()>;
}

/// Keyring-backed store (Secret Service or Secret portal).
pub struct KeyringStore {
    app_id: &'static str,
    keyring: OnceCell<std::result::Result<oo7::Keyring, String>>,
}

impl std::fmt::Debug for KeyringStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyringStore")
            .field("app_id", &self.app_id)
            .finish_non_exhaustive()
    }
}

impl KeyringStore {
    /// Creates a store whose items are tagged with `app_id`.
    ///
    /// The keyring connection is opened lazily, on first use.
    pub fn new(app_id: &'static str) -> Self {
        Self {
            app_id,
            keyring: OnceCell::new(),
        }
    }

    async fn keyring(&self) -> Result<&oo7::Keyring> {
        self.keyring
            .get_or_init(|| async {
                oo7::Keyring::new().await.map_err(|err| {
                    tracing::warn!(%err, "system keyring unavailable");
                    err.to_string()
                })
            })
            .await
            .as_ref()
            .map_err(|err| SecretError::Unavailable(err.clone()))
    }

    async fn unlocked_keyring(&self) -> Result<&oo7::Keyring> {
        let keyring = self.keyring().await?;
        if keyring.is_locked().await.map_err(backend)? {
            // Shows the desktop's unlock prompt.
            keyring.unlock().await.map_err(|_| SecretError::Locked)?;
        }
        Ok(keyring)
    }

    fn attributes(&self, id: ConnectionId) -> [(&'static str, String); 2] {
        [
            (ATTR_APPLICATION, self.app_id.to_owned()),
            (ATTR_CONNECTION, id.to_string()),
        ]
    }

    /// Items of connection `id` holding secret `kind`.
    async fn items(&self, id: ConnectionId, kind: SecretKind) -> Result<Vec<oo7::Item>> {
        let keyring = self.unlocked_keyring().await?;
        let mut matching = Vec::new();
        for item in keyring
            .search_items(&self.attributes(id))
            .await
            .map_err(backend)?
        {
            let attributes = item.attributes().await.map_err(backend)?;
            let item_kind = attributes.get(ATTR_KIND).map(String::as_str);
            let is_kind = match kind {
                SecretKind::Database => item_kind.is_none_or(|k| k == "database"),
                SecretKind::Ssh => item_kind == Some("ssh"),
            };
            if is_kind {
                matching.push(item);
            }
        }
        Ok(matching)
    }
}

fn backend(err: oo7::Error) -> SecretError {
    SecretError::Backend(err.to_string())
}

#[async_trait]
impl SecretStore for KeyringStore {
    async fn is_available(&self) -> bool {
        self.keyring().await.is_ok()
    }

    async fn get(&self, id: ConnectionId, kind: SecretKind) -> Result<Option<Secret>> {
        let items = self.items(id, kind).await?;
        let Some(item) = items.first() else {
            return Ok(None);
        };
        if item.is_locked().await.map_err(backend)? {
            item.unlock().await.map_err(|_| SecretError::Locked)?;
        }
        let secret = item.secret().await.map_err(backend)?;
        let text = std::str::from_utf8(secret.as_bytes())
            .map_err(|_| SecretError::Backend("stored password is not valid UTF-8".into()))?;
        Ok(Some(Secret::new(text)))
    }

    async fn set(
        &self,
        id: ConnectionId,
        kind: SecretKind,
        label: &str,
        secret: &Secret,
    ) -> Result<()> {
        // Replace any previous item, including those without a kind.
        self.delete(id, kind).await?;
        let keyring = self.unlocked_keyring().await?;
        let [app, conn] = self.attributes(id);
        let attributes = [app, conn, (ATTR_KIND, kind.as_str().to_owned())];
        keyring
            .create_item(label, &attributes, oo7::Secret::text(secret.expose()), true)
            .await
            .map_err(backend)
    }

    async fn delete(&self, id: ConnectionId, kind: SecretKind) -> Result<()> {
        for item in self.items(id, kind).await? {
            item.delete().await.map_err(backend)?;
        }
        Ok(())
    }
}

/// In-memory store: secrets live until the process exits or they are
/// deleted.
#[derive(Debug, Default)]
pub struct MemoryStore {
    secrets: Mutex<HashMap<(ConnectionId, SecretKind), Secret>>,
}

impl MemoryStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SecretStore for MemoryStore {
    async fn is_available(&self) -> bool {
        true
    }

    async fn get(&self, id: ConnectionId, kind: SecretKind) -> Result<Option<Secret>> {
        Ok(self.secrets.lock().await.get(&(id, kind)).cloned())
    }

    async fn set(
        &self,
        id: ConnectionId,
        kind: SecretKind,
        _label: &str,
        secret: &Secret,
    ) -> Result<()> {
        self.secrets.lock().await.insert((id, kind), secret.clone());
        Ok(())
    }

    async fn delete(&self, id: ConnectionId, kind: SecretKind) -> Result<()> {
        self.secrets.lock().await.remove(&(id, kind));
        Ok(())
    }
}

/// A store that is never available, to exercise the "keyring unavailable"
/// paths (CDC §10.1).
#[derive(Debug, Default)]
pub struct UnavailableStore;

#[async_trait]
impl SecretStore for UnavailableStore {
    async fn is_available(&self) -> bool {
        false
    }

    async fn get(&self, _id: ConnectionId, _kind: SecretKind) -> Result<Option<Secret>> {
        Err(SecretError::Unavailable("disabled".into()))
    }

    async fn set(
        &self,
        _id: ConnectionId,
        _kind: SecretKind,
        _label: &str,
        _secret: &Secret,
    ) -> Result<()> {
        Err(SecretError::Unavailable("disabled".into()))
    }

    async fn delete(&self, _id: ConnectionId, _kind: SecretKind) -> Result<()> {
        Err(SecretError::Unavailable("disabled".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_store_keeps_kinds_apart() {
        let store = MemoryStore::new();
        let id = ConnectionId::new();
        let db = SecretKind::Database;
        assert_eq!(store.get(id, db).await.expect("get"), None);
        store
            .set(id, db, "l", &Secret::new("pw"))
            .await
            .expect("set");
        store
            .set(id, SecretKind::Ssh, "l", &Secret::new("ssh"))
            .await
            .expect("set");
        let get = |kind| {
            let store = &store;
            async move {
                store
                    .get(id, kind)
                    .await
                    .expect("get")
                    .map(|s| s.expose().to_owned())
            }
        };
        assert_eq!(get(db).await.as_deref(), Some("pw"));
        store.delete(id, db).await.expect("delete");
        assert_eq!(get(db).await, None);
        assert_eq!(get(SecretKind::Ssh).await.as_deref(), Some("ssh"));
    }

    #[tokio::test]
    async fn unavailable_store_refuses_everything() {
        let store = UnavailableStore;
        assert!(!store.is_available().await);
        assert!(matches!(
            store
                .set(
                    ConnectionId::new(),
                    SecretKind::Database,
                    "l",
                    &Secret::new("x")
                )
                .await,
            Err(SecretError::Unavailable(_))
        ));
    }
}
