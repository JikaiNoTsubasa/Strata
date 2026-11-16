// SPDX-License-Identifier: GPL-3.0-or-later
//! SSH tunnel scenarios (Docker required):
//! `cargo test -p strata-engine --test ssh -- --include-ignored`.

// Test helpers may panic: a failure is the expected outcome of a broken test.
#![allow(clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use strata_core::{
    ConnectionConfig, DbKind, Endpoint, ErrorKind, PasswordPolicy, Secret, SshAuth, SshTunnel,
    TlsMode,
};
use strata_engine::{
    Command, EngineConfig, EngineHandle, MetaRequest, PasswordChange, SessionState, StoreLocation,
    UiEvent,
};
use strata_secrets::MemoryStore;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::{GenericImage, ImageExt};

const DB_PASSWORD: &str = "strata-test-password";
const SSH_PASSWORD: &str = "strata-ssh-password";

/// Minimal OpenSSH server allowing password logins and TCP forwarding.
const SSHD_SCRIPT: &str = "apk add --no-cache openssh >/dev/null \\
    && ssh-keygen -A >/dev/null \\
    && adduser -D tunnel && echo 'tunnel:strata-ssh-password' | chpasswd \\
    && sed -i -e '/AllowTcpForwarding/d' -e '/PasswordAuthentication/d' /etc/ssh/sshd_config \\
    && printf 'PasswordAuthentication yes\\nAllowTcpForwarding yes\\n' >> /etc/ssh/sshd_config \\
    && exec /usr/sbin/sshd -D -e";

async fn wait_for(handle: &EngineHandle, pred: impl Fn(&UiEvent) -> bool) -> UiEvent {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(30), handle.events().recv())
            .await
            .expect("engine did not answer in time")
            .expect("engine stopped");
        if pred(&event) {
            return event;
        }
    }
}

async fn test(
    handle: &EngineHandle,
    config: &ConnectionConfig,
    ssh_secret: Option<&str>,
) -> Result<(), ErrorKind> {
    assert!(handle.send(Command::TestConnection {
        request: 1,
        config: config.clone(),
        password: None,
        ssh_secret: ssh_secret.map(Secret::new),
    }));
    match wait_for(handle, |e| matches!(e, UiEvent::TestFinished { .. })).await {
        UiEvent::TestFinished { result: Ok(_), .. } => Ok(()),
        UiEvent::TestFinished { result: Err(e), .. } => {
            eprintln!("test failed: {e:?}");
            Err(e.kind)
        }
        _ => unreachable!(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker"]
async fn connect_through_ssh_tunnel() {
    let pg = Postgres::default()
        .with_password(DB_PASSWORD)
        .with_tag("17-alpine")
        .start()
        .await
        .expect("PostgreSQL container");
    // Reachable from the SSH server only (Docker bridge address).
    let pg_ip = pg.get_bridge_ip_address().await.expect("bridge address");

    let sshd = GenericImage::new("alpine", "3.20")
        .with_exposed_port(22.tcp())
        .with_wait_for(WaitFor::message_on_stderr("Server listening"))
        .with_cmd(["sh", "-c", SSHD_SCRIPT])
        .start()
        .await
        .expect("SSH container");
    let ssh_port = sshd.get_host_port_ipv4(22).await.expect("ssh port");

    let dir = tempfile::tempdir().expect("tempdir");
    let handle = strata_engine::spawn(
        &tokio::runtime::Handle::current(),
        EngineConfig {
            store: StoreLocation::InMemory,
            secrets: Arc::new(MemoryStore::new()),
            known_hosts: Some(dir.path().join("known_hosts")),
        },
    );
    wait_for(&handle, |e| *e == UiEvent::Ready).await;

    let mut cfg = ConnectionConfig::new("tunnelled", DbKind::Postgres);
    cfg.user = "postgres".into();
    cfg.tls.mode = TlsMode::Disable;
    cfg.endpoint = Endpoint::Tcp {
        host: pg_ip.to_string(),
        port: 5432,
    };
    cfg.ssh = Some(SshTunnel {
        host: "127.0.0.1".into(),
        port: ssh_port,
        user: "tunnel".into(),
        auth: SshAuth::Password,
        secret_policy: PasswordPolicy::Keyring,
    });
    assert!(handle.send(Command::SaveConnection {
        config: cfg.clone(),
        password: PasswordChange::Set(Secret::new(DB_PASSWORD)),
        ssh_password: PasswordChange::Set(Secret::new(SSH_PASSWORD)),
    }));
    wait_for(&handle, |e| matches!(e, UiEvent::ConnectionSaved(_))).await;

    // Unknown server key: nothing is sent before the user trusts it.
    assert_eq!(
        test(&handle, &cfg, None).await,
        Err(ErrorKind::UnknownHostKey)
    );
    assert!(handle.send(Command::TrustHostKey {
        host: "127.0.0.1".into(),
        port: ssh_port,
    }));
    wait_for(&handle, |e| matches!(e, UiEvent::HostKeyTrusted { .. })).await;
    assert!(
        std::fs::read_to_string(dir.path().join("known_hosts"))
            .expect("known_hosts written")
            .contains(&format!("[127.0.0.1]:{ssh_port}"))
    );

    // Wrong SSH password, then the stored one.
    assert_eq!(
        test(&handle, &cfg, Some("wrong")).await,
        Err(ErrorKind::SshAuthentication)
    );
    assert_eq!(test(&handle, &cfg, None).await, Ok(()));

    // Connect and browse through the tunnel.
    assert!(handle.send(Command::Connect {
        id: cfg.id,
        password: None,
        ssh_secret: None,
    }));
    let event = wait_for(&handle, |e| {
        matches!(
            e,
            UiEvent::SessionState {
                state: SessionState::Connected(_) | SessionState::Failed { .. },
                ..
            }
        )
    })
    .await;
    assert!(
        matches!(
            event,
            UiEvent::SessionState {
                state: SessionState::Connected(_),
                ..
            }
        ),
        "{event:?}"
    );
    assert!(handle.send(Command::LoadMetadata {
        request: 2,
        connection: cfg.id,
        what: MetaRequest::Schemas {
            database: "postgres".into(),
        },
    }));
    let event = wait_for(&handle, |e| matches!(e, UiEvent::MetadataLoaded { .. })).await;
    assert!(
        matches!(&event, UiEvent::MetadataLoaded { result: Ok(items), .. } if items.iter().any(|i| i.name == "public")),
        "{event:?}"
    );
    assert!(handle.send(Command::Disconnect(cfg.id)));
}
