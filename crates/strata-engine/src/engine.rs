// SPDX-License-Identifier: GPL-3.0-or-later
//! Engine state and command handlers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use strata_core::client_config;
use strata_core::{
    ConnectionConfig, ConnectionId, Endpoint, ErrorKind, ErrorReport, PasswordPolicy, Secret,
    SshAuth, SshTunnel,
};
use strata_db::DbSession;
use strata_secrets::{SecretKind, SecretStore};
use strata_store::Store;
use tokio::sync::Mutex;

use crate::pool::{Live, Op, OpOutput};
use crate::protocol::{Command, PasswordChange, SessionState, TestReport, UiEvent};
use crate::tunnel::{KnownHosts, Tunnel};
use crate::{EngineConfig, StoreLocation};

type Outcome<T> = Result<T, ErrorReport>;

/// Default idle time after which physical sessions are closed (CON-11).
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// How often idle sessions are looked for.
const IDLE_CHECK_PERIOD: Duration = Duration::from_secs(30);

/// State shared by the command handlers.
struct Shared {
    /// Internal database; `None` if it could not be opened.
    store: Option<Arc<StdMutex<Store>>>,
    secrets: Arc<dyn SecretStore>,
    events: async_channel::Sender<UiEvent>,
    /// Connected connections.
    live: Mutex<HashMap<ConnectionId, Arc<Live>>>,
    /// Idle timeout of physical sessions; `None` disables it.
    idle_timeout: StdMutex<Option<Duration>>,
    /// Known SSH server keys.
    known_hosts: Arc<KnownHosts>,
    /// SSH server keys offered but not trusted yet, by `(host, port)`.
    pending_host_keys: StdMutex<HashMap<(String, u16), russh::keys::PublicKey>>,
}

/// Engine main loop: dispatches commands until shutdown.
pub(crate) async fn run(
    config: EngineConfig,
    commands: async_channel::Receiver<Command>,
    events: async_channel::Sender<UiEvent>,
) {
    tracing::debug!("engine started");
    let store = match open_store(config.store).await {
        Ok(store) => Some(Arc::new(StdMutex::new(store))),
        Err(report) => {
            tracing::error!(detail = %report.detail, "internal database unavailable");
            let _ = events.send(UiEvent::StoreUnavailable(report)).await;
            None
        }
    };
    let shared = Arc::new(Shared {
        store,
        secrets: config.secrets,
        events: events.clone(),
        live: Mutex::new(HashMap::new()),
        idle_timeout: StdMutex::new(Some(DEFAULT_IDLE_TIMEOUT)),
        known_hosts: Arc::new(match config.known_hosts {
            Some(path) => KnownHosts {
                strata: path,
                user: None,
            },
            None => KnownHosts::default_paths(),
        }),
        pending_host_keys: StdMutex::new(HashMap::new()),
    });
    let idle_task = tokio::spawn({
        let shared = Arc::clone(&shared);
        async move {
            loop {
                tokio::time::sleep(IDLE_CHECK_PERIOD).await;
                shared.close_idle_sessions().await;
            }
        }
    });
    if events.send(UiEvent::Ready).await.is_err() {
        return;
    }

    while let Ok(command) = commands.recv().await {
        if command == Command::Shutdown {
            break;
        }
        // Each command runs in its own task so a slow server never delays
        // the others.
        let shared = Arc::clone(&shared);
        tokio::spawn(async move { shared.handle(command).await });
    }

    commands.close();
    idle_task.abort();
    shared.live.lock().await.clear();
    events.close();
    tracing::debug!("engine stopped");
}

async fn open_store(location: StoreLocation) -> Outcome<Store> {
    tokio::task::spawn_blocking(move || match location {
        StoreLocation::InMemory => Store::open_in_memory(),
        StoreLocation::Path(path) => Store::open(&path),
        StoreLocation::Default => Store::default_path().and_then(|p| Store::open(&p)),
    })
    .await
    .map_err(|e| ErrorReport::new(ErrorKind::Storage, e.to_string()))?
    .map_err(|e| ErrorReport::new(ErrorKind::Storage, e.to_string()))
}

impl Shared {
    async fn emit(&self, event: UiEvent) {
        if self.events.send(event).await.is_err() {
            tracing::debug!("UI gone, event dropped");
        }
    }

    async fn handle(&self, command: Command) {
        match command {
            Command::Shutdown => {}
            Command::CheckKeyring => {
                let available = self.secrets.is_available().await;
                self.emit(UiEvent::KeyringStatus { available }).await;
            }
            Command::LoadConnections => {
                let event = match self.with_store(|s| s.list_connections()).await {
                    Ok(list) => UiEvent::ConnectionsLoaded(list),
                    Err(e) => UiEvent::OperationFailed(e),
                };
                self.emit(event).await;
            }
            Command::SaveConnection {
                config,
                password,
                ssh_password,
            } => {
                let event = match self.save(config, password, ssh_password).await {
                    Ok(cfg) => UiEvent::ConnectionSaved(cfg),
                    Err(e) => UiEvent::OperationFailed(e),
                };
                self.emit(event).await;
            }
            Command::DeleteConnection(id) => {
                let event = match self.delete(id).await {
                    Ok(()) => UiEvent::ConnectionDeleted(id),
                    Err(e) => UiEvent::OperationFailed(e),
                };
                self.emit(event).await;
            }
            Command::DuplicateConnection { id, name } => {
                let event = match self.duplicate(id, name).await {
                    Ok(cfg) => UiEvent::ConnectionSaved(cfg),
                    Err(e) => UiEvent::OperationFailed(e),
                };
                self.emit(event).await;
            }
            Command::TestConnection {
                request,
                config,
                password,
                ssh_secret,
            } => {
                let result = self.test(&config, password, ssh_secret).await;
                self.emit(UiEvent::TestFinished { request, result }).await;
            }
            Command::Connect {
                id,
                password,
                ssh_secret,
            } => self.connect(id, password, ssh_secret).await,
            Command::TrustHostKey { host, port } => {
                let key = self
                    .pending_host_keys
                    .lock()
                    .ok()
                    .and_then(|mut keys| keys.remove(&(host.clone(), port)));
                let event = match key {
                    Some(key) => match self.known_hosts.learn(&host, port, &key) {
                        Ok(()) => UiEvent::HostKeyTrusted { host, port },
                        Err(e) => UiEvent::OperationFailed(e),
                    },
                    None => UiEvent::OperationFailed(ErrorReport::new(
                        ErrorKind::Other,
                        "no pending host key",
                    )),
                };
                self.emit(event).await;
            }
            Command::Disconnect(id) => {
                self.live.lock().await.remove(&id);
                self.emit(UiEvent::SessionState {
                    id,
                    state: SessionState::Disconnected,
                })
                .await;
            }
            Command::ScanClientConfigs => {
                let event = match tokio::task::spawn_blocking(client_config::import_candidates)
                    .await
                {
                    Ok((candidates, errors)) => UiEvent::ImportCandidates {
                        candidates,
                        warnings: errors.iter().map(ToString::to_string).collect(),
                    },
                    Err(e) => {
                        UiEvent::OperationFailed(ErrorReport::new(ErrorKind::Other, e.to_string()))
                    }
                };
                self.emit(event).await;
            }
            Command::LoadMetadata {
                request,
                connection,
                what,
            } => {
                let result = match self.run_op(connection, Op::Meta(what)).await {
                    Ok(OpOutput::Items(items)) => Ok(items),
                    Ok(_) => Err(unexpected()),
                    Err(e) => Err(e),
                };
                self.emit(UiEvent::MetadataLoaded { request, result }).await;
            }
            Command::DescribeTable {
                request,
                connection,
                object,
            } => {
                let result = match self.run_op(connection, Op::Describe(object)).await {
                    Ok(OpOutput::Table(table)) => Ok(*table),
                    Ok(_) => Err(unexpected()),
                    Err(e) => Err(e),
                };
                self.emit(UiEvent::TableDescribed { request, result }).await;
            }
            Command::LoadDdl {
                request,
                connection,
                object,
            } => {
                let result = match self.run_op(connection, Op::Ddl(object)).await {
                    Ok(OpOutput::Ddl(ddl)) => Ok(ddl),
                    Ok(_) => Err(unexpected()),
                    Err(e) => Err(e),
                };
                self.emit(UiEvent::DdlLoaded { request, result }).await;
            }
            Command::SetIdleTimeout(timeout) => {
                if let Ok(mut current) = self.idle_timeout.lock() {
                    *current = timeout;
                }
            }
        }
    }

    async fn close_idle_sessions(&self) {
        let timeout = self.idle_timeout.lock().ok().and_then(|t| *t);
        let Some(timeout) = timeout else {
            return;
        };
        let live: Vec<_> = self.live.lock().await.values().cloned().collect();
        for connection in live {
            connection.close_idle(timeout).await;
        }
    }

    /// Runs `op` on the right session of `id`, (re)opening it as needed.
    async fn run_op(&self, id: ConnectionId, op: Op) -> Outcome<OpOutput> {
        let live = self
            .live
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| ErrorReport::new(ErrorKind::NotConnected, "not connected"))?;
        let slot = live.slot(op.database(&live)).await;
        let mut slot = slot.lock().await;

        let mut lost = false;
        for attempt in 0..2 {
            let usable = slot.session.as_ref().is_some_and(|s| !s.is_closed());
            if !usable {
                // A session that existed and died means the connection was
                // lost; an empty slot is a first use or an idle close.
                lost |= slot.session.is_some();
                if lost {
                    self.emit(UiEvent::SessionState {
                        id,
                        state: SessionState::Reconnecting,
                    })
                    .await;
                }
                let mut cfg = live.cfg.clone();
                cfg.database = Some(op.database(&live).to_owned());
                match self.open_live_session(&live, &cfg).await {
                    Ok(session) => slot.session = Some(session),
                    Err(error) => {
                        slot.session = None;
                        if lost {
                            self.live.lock().await.remove(&id);
                            self.emit(UiEvent::SessionState {
                                id,
                                state: SessionState::Failed {
                                    error: error.clone(),
                                    needs_password: false,
                                },
                            })
                            .await;
                        }
                        return Err(error);
                    }
                }
                if lost {
                    self.emit(UiEvent::SessionState {
                        id,
                        state: SessionState::Connected(live.info.clone()),
                    })
                    .await;
                }
            }
            let Some(session) = slot.session.as_mut() else {
                continue;
            };
            match op.run(session.as_mut(), &live.info.database).await {
                Ok(output) => {
                    slot.last_used = Instant::now();
                    return Ok(output);
                }
                Err(e) if e.is_connection_lost() && attempt == 0 => {
                    tracing::info!("session lost, reconnecting");
                    lost = true;
                    slot.session = None;
                }
                Err(e) => return Err(e.report()),
            }
        }
        Err(ErrorReport::new(
            ErrorKind::ConnectionRefused,
            "connection lost",
        ))
    }

    /// Opens a session for `live`, through its SSH tunnel (reopened if dead).
    async fn open_live_session(
        &self,
        live: &Live,
        cfg: &ConnectionConfig,
    ) -> Outcome<Box<dyn DbSession>> {
        let via = match &live.cfg.ssh {
            None => None,
            Some(ssh) => {
                let mut tunnel = live.tunnel.lock().await;
                if tunnel.as_ref().is_none_or(Tunnel::is_closed) {
                    *tunnel = None;
                    *tunnel = Some(
                        self.open_tunnel(&live.cfg, ssh, live.ssh_secret.as_ref())
                            .await?,
                    );
                }
                tunnel.as_ref().map(Tunnel::local_addr)
            }
        };
        self.open(cfg, live.password.as_ref(), via).await
    }

    async fn open_tunnel(
        &self,
        cfg: &ConnectionConfig,
        ssh: &SshTunnel,
        secret: Option<&Secret>,
    ) -> Outcome<Tunnel> {
        let Endpoint::Tcp { host, port } = &cfg.endpoint else {
            return Err(ErrorReport::new(
                ErrorKind::Configuration,
                "SSH tunnels need a TCP endpoint",
            ));
        };
        Tunnel::open(ssh, host, *port, secret, Arc::clone(&self.known_hosts))
            .await
            .map_err(|failure| {
                if let (Some(key), Ok(mut keys)) =
                    (failure.unknown_key, self.pending_host_keys.lock())
                {
                    keys.insert((ssh.host.clone(), ssh.port), *key);
                }
                failure.report
            })
    }

    /// SSH secret for `cfg`: the typed one, else per its policy.
    async fn resolve_ssh_secret(
        &self,
        cfg: &ConnectionConfig,
        typed: Option<Secret>,
    ) -> Outcome<Option<Secret>> {
        let Some(ssh) = &cfg.ssh else {
            return Ok(None);
        };
        if typed.is_some() || ssh.auth == SshAuth::Agent {
            return Ok(typed);
        }
        match ssh.secret_policy {
            PasswordPolicy::Keyring => self
                .secrets
                .get(cfg.id, SecretKind::Ssh)
                .await
                .map_err(|e| ErrorReport::new(ErrorKind::Keyring, e.to_string())),
            PasswordPolicy::AskEveryTime if ssh.auth == SshAuth::Password => Err(ErrorReport::new(
                ErrorKind::MissingSshSecret,
                "SSH password required",
            )),
            // A key without passphrase; asked if it turns out encrypted.
            _ => Ok(None),
        }
    }

    /// Runs `f` on the store in a blocking task.
    async fn with_store<T, F>(&self, f: F) -> Outcome<T>
    where
        T: Send + 'static,
        F: FnOnce(&Store) -> strata_store::Result<T> + Send + 'static,
    {
        let store = self.store.clone().ok_or_else(|| {
            ErrorReport::new(ErrorKind::Storage, "the internal database is unavailable")
        })?;
        tokio::task::spawn_blocking(move || {
            let guard = store.lock().map_err(|_| {
                ErrorReport::new(ErrorKind::Storage, "internal database lock poisoned")
            })?;
            f(&guard).map_err(|e| ErrorReport::new(ErrorKind::Storage, e.to_string()))
        })
        .await
        .map_err(|e| ErrorReport::new(ErrorKind::Storage, e.to_string()))?
    }

    async fn get(&self, id: ConnectionId) -> Outcome<ConnectionConfig> {
        self.with_store(move |s| s.get_connection(id))
            .await?
            .ok_or_else(|| ErrorReport::new(ErrorKind::Configuration, "unknown connection"))
    }

    async fn save(
        &self,
        config: ConnectionConfig,
        password: PasswordChange,
        ssh_password: PasswordChange,
    ) -> Outcome<ConnectionConfig> {
        config
            .validate()
            .map_err(|errors| ErrorReport::new(ErrorKind::Configuration, format!("{errors:?}")))?;
        let id = config.id;
        let previous = self.with_store(move |s| s.get_connection(id)).await?;
        let had_secret = previous
            .as_ref()
            .is_some_and(|p| p.password_policy == PasswordPolicy::Keyring);
        let had_ssh_secret = previous
            .as_ref()
            .and_then(|p| p.ssh.as_ref())
            .is_some_and(|t| t.secret_policy == PasswordPolicy::Keyring);

        if config.password_policy == PasswordPolicy::Keyring {
            match password {
                // Store the password first: if the keyring refuses it, nothing
                // is saved and the user can pick another policy (CDC §10.1).
                PasswordChange::Set(secret) => self
                    .secrets
                    .set(id, SecretKind::Database, &keyring_label(&config), &secret)
                    .await
                    .map_err(|e| ErrorReport::new(ErrorKind::Keyring, e.to_string()))?,
                PasswordChange::Clear => self.forget_secret(id, SecretKind::Database).await,
                PasswordChange::Keep => {}
            }
        } else if had_secret {
            self.forget_secret(id, SecretKind::Database).await;
        }

        let ssh_in_keyring = config.ssh.as_ref().is_some_and(|t| {
            t.secret_policy == PasswordPolicy::Keyring && t.auth != SshAuth::Agent
        });
        if ssh_in_keyring {
            match ssh_password {
                PasswordChange::Set(secret) => self
                    .secrets
                    .set(
                        id,
                        SecretKind::Ssh,
                        &format!("{} (SSH)", keyring_label(&config)),
                        &secret,
                    )
                    .await
                    .map_err(|e| ErrorReport::new(ErrorKind::Keyring, e.to_string()))?,
                PasswordChange::Clear => self.forget_secret(id, SecretKind::Ssh).await,
                PasswordChange::Keep => {}
            }
        } else if had_ssh_secret {
            self.forget_secret(id, SecretKind::Ssh).await;
        }

        let saved = config.clone();
        self.with_store(move |s| s.save_connection(&saved)).await?;
        Ok(config)
    }

    /// Best-effort removal of a stored password.
    async fn forget_secret(&self, id: ConnectionId, kind: SecretKind) {
        if let Err(err) = self.secrets.delete(id, kind).await {
            tracing::warn!(%err, "could not remove a stored password");
        }
    }

    async fn delete(&self, id: ConnectionId) -> Outcome<()> {
        self.live.lock().await.remove(&id);
        let previous = self.get(id).await?;
        self.with_store(move |s| s.delete_connection(id)).await?;
        if previous.password_policy == PasswordPolicy::Keyring {
            self.forget_secret(id, SecretKind::Database).await;
        }
        if previous
            .ssh
            .is_some_and(|t| t.secret_policy == PasswordPolicy::Keyring)
        {
            self.forget_secret(id, SecretKind::Ssh).await;
        }
        Ok(())
    }

    async fn duplicate(&self, id: ConnectionId, name: String) -> Outcome<ConnectionConfig> {
        let original = self.get(id).await?;
        let mut copy = original.clone();
        copy.id = ConnectionId::new();
        copy.name = name;
        let password = if copy.password_policy == PasswordPolicy::Keyring {
            self.copied_secret(original.id, SecretKind::Database)
                .await?
        } else {
            PasswordChange::Keep
        };
        let ssh_password = if copy
            .ssh
            .as_ref()
            .is_some_and(|t| t.secret_policy == PasswordPolicy::Keyring)
        {
            self.copied_secret(original.id, SecretKind::Ssh).await?
        } else {
            PasswordChange::Keep
        };
        self.save(copy, password, ssh_password).await
    }

    async fn copied_secret(&self, id: ConnectionId, kind: SecretKind) -> Outcome<PasswordChange> {
        match self.secrets.get(id, kind).await {
            Ok(Some(secret)) => Ok(PasswordChange::Set(secret)),
            Ok(None) => Ok(PasswordChange::Keep),
            Err(e) => Err(ErrorReport::new(ErrorKind::Keyring, e.to_string())),
        }
    }

    /// Password to use for `cfg`: the one typed by the user, else the one
    /// given by the password policy.
    async fn resolve_password(
        &self,
        cfg: &ConnectionConfig,
        typed: Option<Secret>,
    ) -> Outcome<Option<Secret>> {
        if typed.is_some() {
            return Ok(typed);
        }
        match cfg.password_policy {
            PasswordPolicy::Keyring => self
                .secrets
                .get(cfg.id, SecretKind::Database)
                .await
                .map_err(|e| ErrorReport::new(ErrorKind::Keyring, e.to_string())),
            PasswordPolicy::AskEveryTime => Err(ErrorReport::new(
                ErrorKind::MissingPassword,
                "password required",
            )),
            PasswordPolicy::ClientConfigFile => {
                let cfg = cfg.clone();
                tokio::task::spawn_blocking(move || client_config::lookup_password(&cfg))
                    .await
                    .map_err(|e| ErrorReport::new(ErrorKind::Other, e.to_string()))?
                    .map_err(|e| ErrorReport::new(ErrorKind::Configuration, e.to_string()))
            }
            PasswordPolicy::None => Ok(None),
        }
    }

    async fn open(
        &self,
        cfg: &ConnectionConfig,
        password: Option<&Secret>,
        via: Option<std::net::SocketAddr>,
    ) -> Outcome<Box<dyn DbSession>> {
        strata_db::driver(cfg.kind)
            .map_err(|e| e.report())?
            .connect(cfg, password, via)
            .await
            .map_err(|e| e.report())
    }

    async fn test(
        &self,
        cfg: &ConnectionConfig,
        typed: Option<Secret>,
        typed_ssh: Option<Secret>,
    ) -> Outcome<TestReport> {
        cfg.validate()
            .map_err(|errors| ErrorReport::new(ErrorKind::Configuration, format!("{errors:?}")))?;
        let start = Instant::now();
        let ssh_secret = self.resolve_ssh_secret(cfg, typed_ssh).await?;
        let password = self.resolve_password(cfg, typed).await?;
        // The tunnel lives until the end of the test.
        let tunnel = match &cfg.ssh {
            Some(ssh) => Some(self.open_tunnel(cfg, ssh, ssh_secret.as_ref()).await?),
            None => None,
        };
        let mut session = self
            .open(
                cfg,
                password.as_ref(),
                tunnel.as_ref().map(Tunnel::local_addr),
            )
            .await?;
        let server = session.server_info().await.map_err(|e| e.report())?;
        drop(session);
        drop(tunnel);
        Ok(TestReport {
            server,
            duration: start.elapsed(),
        })
    }

    async fn connect(&self, id: ConnectionId, typed: Option<Secret>, typed_ssh: Option<Secret>) {
        let state = match self.try_connect(id, typed, typed_ssh).await {
            Ok(state) => state,
            Err((error, policy)) => SessionState::Failed {
                needs_password: matches!(
                    error.kind,
                    ErrorKind::Authentication | ErrorKind::MissingPassword
                ) && policy != Some(PasswordPolicy::None),
                error,
            },
        };
        self.emit(UiEvent::SessionState { id, state }).await;
    }

    async fn try_connect(
        &self,
        id: ConnectionId,
        typed: Option<Secret>,
        typed_ssh: Option<Secret>,
    ) -> Result<SessionState, (ErrorReport, Option<PasswordPolicy>)> {
        if let Some(live) = self.live.lock().await.get(&id) {
            return Ok(SessionState::Connected(live.info.clone()));
        }
        let cfg = self.get(id).await.map_err(|e| (e, None))?;
        let policy = Some(cfg.password_policy);
        self.emit(UiEvent::SessionState {
            id,
            state: SessionState::Connecting,
        })
        .await;

        let ssh_secret = self
            .resolve_ssh_secret(&cfg, typed_ssh)
            .await
            .map_err(|e| (e, policy))?;
        let password = self
            .resolve_password(&cfg, typed)
            .await
            .map_err(|e| (e, policy))?;
        let tunnel = match &cfg.ssh {
            Some(ssh) => Some(
                self.open_tunnel(&cfg, ssh, ssh_secret.as_ref())
                    .await
                    .map_err(|e| (e, policy))?,
            ),
            None => None,
        };
        let mut session = self
            .open(
                &cfg,
                password.as_ref(),
                tunnel.as_ref().map(Tunnel::local_addr),
            )
            .await
            .map_err(|e| (e, policy))?;
        let info = session
            .server_info()
            .await
            .map_err(|e| (e.report(), policy))?;

        self.live.lock().await.insert(
            id,
            Arc::new(Live::new(
                cfg,
                password,
                ssh_secret,
                tunnel,
                info.clone(),
                session,
            )),
        );
        Ok(SessionState::Connected(info))
    }
}

fn unexpected() -> ErrorReport {
    ErrorReport::new(ErrorKind::Other, "unexpected response")
}

fn keyring_label(cfg: &ConnectionConfig) -> String {
    format!("Strata — {} ({})", cfg.name, cfg.summary())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use strata_core::{DbKind, Endpoint, TlsMode};
    use strata_secrets::{MemoryStore, UnavailableStore};
    use tokio::runtime::Handle;

    use super::*;
    use crate::{EngineHandle, spawn};

    fn engine_with(store: StoreLocation, secrets: Arc<dyn SecretStore>) -> EngineHandle {
        spawn(
            &Handle::current(),
            EngineConfig {
                store,
                secrets,
                known_hosts: None,
            },
        )
    }

    async fn next(handle: &EngineHandle) -> Option<UiEvent> {
        tokio::time::timeout(Duration::from_secs(10), handle.events().recv())
            .await
            .expect("engine did not answer in time")
            .ok()
    }

    async fn started(store: StoreLocation, secrets: Arc<dyn SecretStore>) -> EngineHandle {
        let handle = engine_with(store, secrets);
        assert_eq!(next(&handle).await, Some(UiEvent::Ready));
        handle
    }

    fn sample(name: &str) -> ConnectionConfig {
        let mut cfg = ConnectionConfig::new(name, DbKind::Postgres);
        cfg.user = "app".into();
        cfg
    }

    async fn request(handle: &EngineHandle, command: Command) -> UiEvent {
        assert!(handle.send(command));
        next(handle).await.expect("event")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_closes_event_channel() {
        let handle = started(StoreLocation::InMemory, Arc::new(MemoryStore::new())).await;
        assert!(handle.send(Command::Shutdown));
        assert_eq!(next(&handle).await, None);
        assert!(!handle.send(Command::Shutdown));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn save_load_duplicate_delete() {
        let secrets = Arc::new(MemoryStore::new());
        let handle = started(StoreLocation::InMemory, secrets.clone()).await;
        let cfg = sample("Main");

        let saved = request(
            &handle,
            Command::SaveConnection {
                config: cfg.clone(),
                password: PasswordChange::Set(Secret::new("pw")),
                ssh_password: PasswordChange::Keep,
            },
        )
        .await;
        assert_eq!(saved, UiEvent::ConnectionSaved(cfg.clone()));
        assert_eq!(
            secrets
                .get(cfg.id, SecretKind::Database)
                .await
                .expect("get")
                .as_ref()
                .map(Secret::expose),
            Some("pw")
        );

        let UiEvent::ConnectionSaved(copy) = request(
            &handle,
            Command::DuplicateConnection {
                id: cfg.id,
                name: "Main (copy)".into(),
            },
        )
        .await
        else {
            panic!("duplicate failed");
        };
        assert_ne!(copy.id, cfg.id);
        assert_eq!(
            secrets
                .get(copy.id, SecretKind::Database)
                .await
                .expect("get")
                .as_ref()
                .map(Secret::expose),
            Some("pw")
        );

        let UiEvent::ConnectionsLoaded(list) = request(&handle, Command::LoadConnections).await
        else {
            panic!("load failed");
        };
        assert_eq!(list.len(), 2);

        assert_eq!(
            request(&handle, Command::DeleteConnection(cfg.id)).await,
            UiEvent::ConnectionDeleted(cfg.id)
        );
        assert_eq!(
            secrets
                .get(cfg.id, SecretKind::Database)
                .await
                .expect("get"),
            None
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn leaving_keyring_policy_removes_the_password() {
        let secrets = Arc::new(MemoryStore::new());
        let handle = started(StoreLocation::InMemory, secrets.clone()).await;
        let mut cfg = sample("Main");
        request(
            &handle,
            Command::SaveConnection {
                config: cfg.clone(),
                password: PasswordChange::Set(Secret::new("pw")),
                ssh_password: PasswordChange::Keep,
            },
        )
        .await;
        cfg.password_policy = PasswordPolicy::AskEveryTime;
        request(
            &handle,
            Command::SaveConnection {
                config: cfg.clone(),
                password: PasswordChange::Keep,
                ssh_password: PasswordChange::Keep,
            },
        )
        .await;
        assert_eq!(
            secrets
                .get(cfg.id, SecretKind::Database)
                .await
                .expect("get"),
            None
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unavailable_keyring_refuses_to_save() {
        let handle = started(StoreLocation::InMemory, Arc::new(UnavailableStore)).await;
        let event = request(&handle, Command::CheckKeyring).await;
        assert_eq!(event, UiEvent::KeyringStatus { available: false });

        let cfg = sample("Main");
        let event = request(
            &handle,
            Command::SaveConnection {
                config: cfg,
                password: PasswordChange::Set(Secret::new("pw")),
                ssh_password: PasswordChange::Keep,
            },
        )
        .await;
        assert!(matches!(event, UiEvent::OperationFailed(ref e) if e.kind == ErrorKind::Keyring));
        assert_eq!(
            request(&handle, Command::LoadConnections).await,
            UiEvent::ConnectionsLoaded(Vec::new())
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn invalid_settings_are_rejected() {
        let handle = started(StoreLocation::InMemory, Arc::new(MemoryStore::new())).await;
        let mut cfg = sample("Main");
        cfg.user.clear();
        let event = request(
            &handle,
            Command::SaveConnection {
                config: cfg,
                password: PasswordChange::Keep,
                ssh_password: PasswordChange::Keep,
            },
        )
        .await;
        assert!(
            matches!(event, UiEvent::OperationFailed(ref e) if e.kind == ErrorKind::Configuration)
        );
    }

    /// CDC §16: no password in clear on disk.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn password_never_reaches_the_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let handle = started(
            StoreLocation::Path(dir.path().join("strata.db")),
            Arc::new(MemoryStore::new()),
        )
        .await;
        const PASSWORD: &str = "Unique-Passw0rd-4f9c2e";
        let event = request(
            &handle,
            Command::SaveConnection {
                config: sample("Main"),
                password: PasswordChange::Set(Secret::new(PASSWORD)),
                ssh_password: PasswordChange::Keep,
            },
        )
        .await;
        assert!(matches!(event, UiEvent::ConnectionSaved(_)));
        assert!(handle.send(Command::Shutdown));
        while next(&handle).await.is_some() {}

        for entry in std::fs::read_dir(dir.path()).expect("read dir") {
            let path = entry.expect("entry").path();
            // SQLite may remove its -wal/-shm files while we iterate.
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            assert!(
                !bytes
                    .windows(PASSWORD.len())
                    .any(|w| w == PASSWORD.as_bytes()),
                "password found in {}",
                path.display()
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn connect_failures_are_reported() {
        let handle = started(StoreLocation::InMemory, Arc::new(MemoryStore::new())).await;

        // "Ask every time" without a password: the UI must prompt.
        let mut ask = sample("Ask");
        ask.password_policy = PasswordPolicy::AskEveryTime;
        request(
            &handle,
            Command::SaveConnection {
                config: ask.clone(),
                password: PasswordChange::Keep,
                ssh_password: PasswordChange::Keep,
            },
        )
        .await;
        assert!(handle.send(Command::Connect {
            id: ask.id,
            password: None,
            ssh_secret: None,
        }));
        assert_eq!(
            next(&handle).await,
            Some(UiEvent::SessionState {
                id: ask.id,
                state: SessionState::Connecting
            })
        );
        let Some(UiEvent::SessionState {
            state:
                SessionState::Failed {
                    error,
                    needs_password,
                },
            ..
        }) = next(&handle).await
        else {
            panic!("expected a failure");
        };
        assert_eq!(error.kind, ErrorKind::MissingPassword);
        assert!(needs_password);

        // Unreachable server.
        let mut refused = sample("Refused");
        refused.password_policy = PasswordPolicy::None;
        refused.tls.mode = TlsMode::Disable;
        refused.endpoint = Endpoint::Tcp {
            host: "127.0.0.1".into(),
            port: 1,
        };
        let event = request(
            &handle,
            Command::TestConnection {
                request: 7,
                config: refused,
                password: None,
                ssh_secret: None,
            },
        )
        .await;
        assert!(matches!(
            event,
            UiEvent::TestFinished { request: 7, result: Err(ref e) } if e.kind == ErrorKind::ConnectionRefused
        ));
    }
}
