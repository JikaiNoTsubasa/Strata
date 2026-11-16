// SPDX-License-Identifier: GPL-3.0-or-later
//! Engine scenarios against a real PostgreSQL (Docker required):
//! `cargo test -p strata-engine -- --include-ignored`.

// Test helpers may panic: a failure is the expected outcome of a broken test.
#![allow(clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use strata_core::{
    ConnectionConfig, DbKind, Endpoint, ObjectKind, ObjectRef, Secret, TablePart, TlsMode,
};
use strata_engine::{
    Command, EngineConfig, EngineHandle, MetaHint, MetaRequest, PasswordChange, SessionState,
    StoreLocation, UiEvent,
};
use strata_secrets::MemoryStore;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::ImageExt;
use testcontainers_modules::testcontainers::runners::AsyncRunner;

const PASSWORD: &str = "strata-test-password";

async fn next(handle: &EngineHandle) -> UiEvent {
    tokio::time::timeout(Duration::from_secs(20), handle.events().recv())
        .await
        .expect("engine did not answer in time")
        .expect("engine stopped")
}

/// Waits for an event matching `pred`, skipping the others.
async fn wait_for(handle: &EngineHandle, pred: impl Fn(&UiEvent) -> bool) -> UiEvent {
    loop {
        let event = next(handle).await;
        if pred(&event) {
            return event;
        }
    }
}

async fn metadata(
    handle: &EngineHandle,
    id: strata_core::ConnectionId,
    what: MetaRequest,
) -> Vec<String> {
    assert!(handle.send(Command::LoadMetadata {
        request: 1,
        connection: id,
        what
    }));
    match wait_for(handle, |e| matches!(e, UiEvent::MetadataLoaded { .. })).await {
        UiEvent::MetadataLoaded {
            result: Ok(items), ..
        } => items.into_iter().map(|i| i.name).collect(),
        other => panic!("metadata failed: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker"]
async fn browse_and_reconnect() {
    let node = Postgres::default()
        .with_password(PASSWORD)
        .with_tag("17-alpine")
        .start()
        .await
        .expect("container");
    let port = node.get_host_port_ipv4(5432).await.expect("port");
    let (admin, connection) = tokio_postgres::connect(
        &format!("host=127.0.0.1 port={port} user=postgres password={PASSWORD}"),
        tokio_postgres::NoTls,
    )
    .await
    .expect("admin connection");
    tokio::spawn(connection);
    for sql in [
        "CREATE TABLE public.item (id int PRIMARY KEY, label text NOT NULL)",
        "CREATE DATABASE other",
    ] {
        admin.batch_execute(sql).await.expect("fixture");
    }

    let handle = strata_engine::spawn(
        &tokio::runtime::Handle::current(),
        EngineConfig {
            store: StoreLocation::InMemory,
            secrets: Arc::new(MemoryStore::new()),
            known_hosts: None,
        },
    );
    assert_eq!(next(&handle).await, UiEvent::Ready);

    let mut cfg = ConnectionConfig::new("pg", DbKind::Postgres);
    cfg.user = "postgres".into();
    cfg.tls.mode = TlsMode::Disable;
    cfg.endpoint = Endpoint::Tcp {
        host: "127.0.0.1".into(),
        port,
    };
    let id = cfg.id;
    assert!(handle.send(Command::SaveConnection {
        config: cfg,
        password: PasswordChange::Set(Secret::new(PASSWORD)),
        ssh_password: PasswordChange::Keep,
    }));
    wait_for(&handle, |e| matches!(e, UiEvent::ConnectionSaved(_))).await;

    // Metadata before connecting is refused.
    assert!(handle.send(Command::LoadMetadata {
        request: 9,
        connection: id,
        what: MetaRequest::Databases
    }));
    let event = wait_for(&handle, |e| matches!(e, UiEvent::MetadataLoaded { .. })).await;
    assert!(matches!(
        event,
        UiEvent::MetadataLoaded { result: Err(_), .. }
    ));

    assert!(handle.send(Command::Connect {
        id,
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

    // Databases, with the current one flagged.
    assert!(handle.send(Command::LoadMetadata {
        request: 2,
        connection: id,
        what: MetaRequest::Databases
    }));
    let UiEvent::MetadataLoaded {
        result: Ok(dbs), ..
    } = wait_for(&handle, |e| matches!(e, UiEvent::MetadataLoaded { .. })).await
    else {
        panic!("databases");
    };
    assert!(
        dbs.iter()
            .any(|d| d.name == "postgres" && d.hint == MetaHint::Database { current: true })
    );
    assert!(
        dbs.iter()
            .any(|d| d.name == "other" && d.hint == MetaHint::Database { current: false })
    );

    // Browsing another database opens a session on it.
    let schemas = metadata(
        &handle,
        id,
        MetaRequest::Schemas {
            database: "other".into(),
        },
    )
    .await;
    assert_eq!(schemas, ["public"]);

    let tables = metadata(
        &handle,
        id,
        MetaRequest::Objects {
            database: "postgres".into(),
            schema: "public".into(),
            kind: ObjectKind::Table,
        },
    )
    .await;
    assert_eq!(tables, ["item"]);
    let columns = metadata(
        &handle,
        id,
        MetaRequest::TablePart {
            database: "postgres".into(),
            schema: "public".into(),
            table: "item".into(),
            part: TablePart::Columns,
        },
    )
    .await;
    assert_eq!(columns, ["id", "label"]);

    // Kill Strata's sessions server-side: the next request reconnects (CON-10).
    admin
        .batch_execute(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE application_name = 'Strata' AND pid <> pg_backend_pid()",
        )
        .await
        .expect("terminate");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let object = ObjectRef {
        database: "postgres".into(),
        schema: "public".into(),
        name: "item".into(),
        kind: ObjectKind::Table,
        signature: None,
    };
    assert!(handle.send(Command::LoadDdl {
        request: 3,
        connection: id,
        object
    }));
    let mut saw_reconnecting = false;
    let ddl = loop {
        match next(&handle).await {
            UiEvent::SessionState {
                state: SessionState::Reconnecting,
                ..
            } => saw_reconnecting = true,
            UiEvent::DdlLoaded { result, .. } => break result.expect("ddl after reconnect"),
            _ => {}
        }
    };
    assert!(saw_reconnecting);
    assert!(ddl.starts_with("CREATE TABLE public.item ("), "{ddl}");

    assert!(handle.send(Command::Disconnect(id)));
    wait_for(&handle, |e| {
        matches!(
            e,
            UiEvent::SessionState {
                state: SessionState::Disconnected,
                ..
            }
        )
    })
    .await;
}
