// SPDX-License-Identifier: GPL-3.0-or-later
//! SSH tunnels (`ssh -L` equivalent, in process — CDC §5.4 forbids sidecars).
//!
//! A tunnel listens on `127.0.0.1:<random port>`; each accepted connection is
//! relayed through a `direct-tcpip` channel to the database server, as seen
//! from the SSH server. Server keys are checked against `~/.ssh/known_hosts`
//! (read only) and Strata's own `known_hosts` file, where keys accepted by
//! the user are recorded.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use russh::Disconnect;
use russh::client::{self, Handle, Handler};
use russh::keys::PublicKeyOrCertificate;
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::AgentClient;
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKey};
use strata_core::{ErrorKind, ErrorReport, Secret, SshAuth, SshTunnel};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Timeout of the SSH connection and authentication.
const SSH_TIMEOUT: Duration = Duration::from_secs(20);
/// Keep-alive interval, to detect dead tunnels.
const KEEPALIVE: Duration = Duration::from_secs(30);

/// Known server keys.
#[derive(Debug, Clone)]
pub(crate) struct KnownHosts {
    /// Strata's file (read-write), `$XDG_DATA_HOME/strata/known_hosts`.
    pub(crate) strata: PathBuf,
    /// The user's OpenSSH file (read only), `~/.ssh/known_hosts`.
    pub(crate) user: Option<PathBuf>,
}

/// Result of a server key check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyStatus {
    Trusted,
    Unknown,
    Changed,
}

impl KnownHosts {
    /// Default locations.
    pub(crate) fn default_paths() -> Self {
        let data = std::env::var_os("XDG_DATA_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_default();
        Self {
            strata: data.join("strata").join("known_hosts"),
            user: std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".ssh/known_hosts")),
        }
    }

    fn check(&self, host: &str, port: u16, key: &PublicKey) -> KeyStatus {
        let mut status = KeyStatus::Unknown;
        for path in std::iter::once(&self.strata).chain(self.user.as_ref()) {
            if !path.exists() {
                continue;
            }
            match russh::keys::check_known_hosts_path(host, port, key, path) {
                Ok(true) => status = KeyStatus::Trusted,
                Ok(false) => {}
                // A different key recorded for this host: never connect.
                Err(russh::keys::Error::KeyChanged { .. }) => return KeyStatus::Changed,
                Err(err) => tracing::debug!(%err, "unreadable known_hosts file"),
            }
        }
        status
    }

    /// Records `key` as trusted for `host:port` in Strata's file.
    pub(crate) fn learn(&self, host: &str, port: u16, key: &PublicKey) -> Result<(), ErrorReport> {
        russh::keys::known_hosts::learn_known_hosts_path(host, port, key, &self.strata)
            .map_err(|e| ErrorReport::new(ErrorKind::Storage, e.to_string()))?;
        restrict_permissions(&self.strata);
        Ok(())
    }
}

fn restrict_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

/// Human-readable fingerprint, as printed by OpenSSH.
pub(crate) fn fingerprint(key: &PublicKey) -> String {
    format!("{} {}", key.algorithm(), key.fingerprint(HashAlg::Sha256))
}

/// Checks server keys during the handshake and remembers what it saw.
struct ClientHandler {
    host: String,
    port: u16,
    known: Arc<KnownHosts>,
    seen: Arc<StdMutex<Option<(PublicKey, KeyStatus)>>>,
}

impl Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = key else {
            // Host certificates are not supported yet.
            return Ok(false);
        };
        let status = self.known.check(&self.host, self.port, key);
        if let Ok(mut seen) = self.seen.lock() {
            *seen = Some((key.clone(), status));
        }
        Ok(status == KeyStatus::Trusted)
    }
}

/// Why a tunnel could not be opened.
#[derive(Debug)]
pub(crate) struct TunnelFailure {
    pub(crate) report: ErrorReport,
    /// Unknown server key, to be trusted by the user (trust on first use).
    pub(crate) unknown_key: Option<Box<PublicKey>>,
}

impl From<ErrorReport> for TunnelFailure {
    fn from(report: ErrorReport) -> Self {
        Self {
            report,
            unknown_key: None,
        }
    }
}

/// An open tunnel; closed when dropped.
pub(crate) struct Tunnel {
    handle: Arc<Handle<ClientHandler>>,
    local: SocketAddr,
    accept: JoinHandle<()>,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.accept.abort();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let handle = Arc::clone(&self.handle);
            runtime.spawn(async move {
                let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
            });
        }
    }
}

impl Tunnel {
    /// Local address the database driver connects to.
    pub(crate) fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// Whether the SSH connection is gone.
    pub(crate) fn is_closed(&self) -> bool {
        self.handle.is_closed() || self.accept.is_finished()
    }

    /// Opens a tunnel to `target_host:target_port` through `ssh`.
    ///
    /// `secret` is the SSH password or key passphrase, when needed.
    pub(crate) async fn open(
        ssh: &SshTunnel,
        target_host: &str,
        target_port: u16,
        secret: Option<&Secret>,
        known: Arc<KnownHosts>,
    ) -> Result<Self, TunnelFailure> {
        let seen = Arc::new(StdMutex::new(None));
        let handler = ClientHandler {
            host: ssh.host.clone(),
            port: ssh.port,
            known,
            seen: Arc::clone(&seen),
        };
        let config = Arc::new(client::Config {
            keepalive_interval: Some(KEEPALIVE),
            inactivity_timeout: None,
            ..client::Config::default()
        });
        let connecting = tokio::time::timeout(
            SSH_TIMEOUT,
            client::connect(config, (ssh.host.as_str(), ssh.port), handler),
        )
        .await;
        let mut handle = match connecting {
            Ok(Ok(handle)) => handle,
            Ok(Err(err)) => {
                let seen = seen.lock().ok().and_then(|s| s.clone());
                return Err(match seen {
                    Some((key, KeyStatus::Unknown)) => TunnelFailure {
                        report: ErrorReport::new(ErrorKind::UnknownHostKey, fingerprint(&key)),
                        unknown_key: Some(Box::new(key)),
                    },
                    Some((key, KeyStatus::Changed)) => ErrorReport::new(
                        ErrorKind::HostKeyMismatch,
                        format!(
                            "the key of {}:{} changed: {}",
                            ssh.host,
                            ssh.port,
                            fingerprint(&key)
                        ),
                    )
                    .into(),
                    _ => ErrorReport::new(ErrorKind::SshConnection, err.to_string()).into(),
                });
            }
            Err(_) => {
                return Err(ErrorReport::new(
                    ErrorKind::SshConnection,
                    "the SSH server did not answer",
                )
                .into());
            }
        };

        tokio::time::timeout(SSH_TIMEOUT, authenticate(&mut handle, ssh, secret))
            .await
            .map_err(|_| {
                ErrorReport::new(ErrorKind::SshConnection, "SSH authentication timed out")
            })??;

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| ErrorReport::new(ErrorKind::Other, e.to_string()))?;
        let local = listener
            .local_addr()
            .map_err(|e| ErrorReport::new(ErrorKind::Other, e.to_string()))?;
        let handle = Arc::new(handle);
        let accept = tokio::spawn(forward(
            listener,
            Arc::clone(&handle),
            target_host.to_owned(),
            target_port,
        ));
        tracing::debug!(%local, "SSH tunnel open");
        Ok(Self {
            handle,
            local,
            accept,
        })
    }
}

/// Relays every local connection through a new SSH channel.
async fn forward(
    listener: TcpListener,
    handle: Arc<Handle<ClientHandler>>,
    target_host: String,
    target_port: u16,
) {
    loop {
        let Ok((mut socket, peer)) = listener.accept().await else {
            return;
        };
        if handle.is_closed() {
            return;
        }
        let handle = Arc::clone(&handle);
        let target_host = target_host.clone();
        tokio::spawn(async move {
            match handle
                .channel_open_direct_tcpip(
                    target_host,
                    u32::from(target_port),
                    "127.0.0.1",
                    u32::from(peer.port()),
                )
                .await
            {
                Ok(channel) => {
                    let mut stream = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
                }
                Err(err) => tracing::debug!(%err, "SSH channel refused"),
            }
        });
    }
}

async fn authenticate(
    handle: &mut Handle<ClientHandler>,
    ssh: &SshTunnel,
    secret: Option<&Secret>,
) -> Result<(), TunnelFailure> {
    let refused = || {
        ErrorReport::new(
            ErrorKind::SshAuthentication,
            "the SSH server refused the credentials",
        )
    };
    let failure = |e: russh::Error| ErrorReport::new(ErrorKind::SshConnection, e.to_string());
    let success = match &ssh.auth {
        SshAuth::Password => {
            let secret = secret.ok_or_else(|| {
                ErrorReport::new(ErrorKind::MissingSshSecret, "SSH password required")
            })?;
            handle
                .authenticate_password(&ssh.user, secret.expose())
                .await
                .map_err(failure)?
                .success()
        }
        SshAuth::PrivateKey { path } => {
            let key =
                russh::keys::load_secret_key(path, secret.map(Secret::expose)).map_err(|err| {
                    let text = err.to_string();
                    if secret.is_none() && text.to_lowercase().contains("encrypted") {
                        ErrorReport::new(ErrorKind::MissingSshSecret, "key passphrase required")
                    } else if secret.is_some() && text.to_lowercase().contains("crypt") {
                        ErrorReport::new(ErrorKind::SshAuthentication, "wrong key passphrase")
                    } else {
                        ErrorReport::new(
                            ErrorKind::Configuration,
                            format!("{}: {text}", path.display()),
                        )
                    }
                })?;
            let hash = handle
                .best_supported_rsa_hash()
                .await
                .map_err(failure)?
                .flatten();
            handle
                .authenticate_publickey(&ssh.user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                .await
                .map_err(failure)?
                .success()
        }
        SshAuth::Agent => {
            let mut agent = AgentClient::connect_env().await.map_err(|e| {
                ErrorReport::new(
                    ErrorKind::SshAuthentication,
                    format!("no SSH agent available: {e}"),
                )
            })?;
            if let Some(binding) = handle.agent_session_binding() {
                let _ = agent.bind_session(binding, false).await;
            }
            let identities = agent
                .request_identities()
                .await
                .map_err(|e| ErrorReport::new(ErrorKind::SshAuthentication, e.to_string()))?;
            let hash = handle
                .best_supported_rsa_hash()
                .await
                .map_err(failure)?
                .flatten();
            let mut success = false;
            for identity in identities {
                let AgentIdentity::PublicKey { key, .. } = identity else {
                    continue;
                };
                match handle
                    .authenticate_publickey_with(&ssh.user, key, hash, &mut agent)
                    .await
                {
                    Ok(result) if result.success() => {
                        success = true;
                        break;
                    }
                    Ok(_) => {}
                    Err(err) => tracing::debug!(%err, "agent key refused"),
                }
            }
            success
        }
    };
    if success {
        Ok(())
    } else {
        Err(refused().into())
    }
}
