// SPDX-License-Identifier: GPL-3.0-or-later
//! PostgreSQL integration tests.
//!
//! Tests marked `#[ignore]` need Docker (testcontainers); run them with
//! `cargo test -p strata-db -- --include-ignored`.

use strata_core::{ConnectionConfig, DbKind, Endpoint, Secret, TlsMode};
use strata_db::{DbError, driver};
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::ImageExt;
use testcontainers_modules::testcontainers::runners::AsyncRunner;

/// PostgreSQL image used by the tests (CDC §12 targets 14 to 17).
const IMAGE_TAG: &str = "17-alpine";

/// Throw-away credentials of the ephemeral test container.
const PASSWORD: &str = "strata-test-password";

fn config(port: u16, tls: TlsMode) -> ConnectionConfig {
    let mut cfg = ConnectionConfig::new("test", DbKind::Postgres);
    cfg.endpoint = Endpoint::Tcp {
        host: "127.0.0.1".into(),
        port,
    };
    cfg.user = "postgres".into();
    cfg.database = Some("postgres".into());
    cfg.tls.mode = tls;
    cfg
}

async fn connect(
    cfg: &ConnectionConfig,
    password: &str,
) -> Result<Box<dyn strata_db::DbSession>, DbError> {
    driver(DbKind::Postgres)?
        .connect(cfg, Some(&Secret::new(password)), None)
        .await
}

#[tokio::test]
async fn refused_connection_is_reported() {
    // Port 1 is never a PostgreSQL server.
    let err = connect(&config(1, TlsMode::Disable), "x").await.err();
    assert!(
        matches!(err, Some(DbError::ConnectionRefused(_))),
        "{err:?}"
    );
}

#[tokio::test]
async fn unknown_host_is_reported() {
    let mut cfg = config(5432, TlsMode::Disable);
    cfg.endpoint = Endpoint::Tcp {
        host: "strata-test.invalid".into(),
        port: 5432,
    };
    let err = connect(&cfg, "x").await.err();
    assert!(matches!(err, Some(DbError::HostNotFound(_))), "{err:?}");
}

#[tokio::test]
async fn mysql_is_not_supported_yet() {
    assert!(matches!(
        driver(DbKind::MySql),
        Err(DbError::Unsupported(_))
    ));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_scenarios() {
    let node = Postgres::default()
        .with_password(PASSWORD)
        .with_tag(IMAGE_TAG)
        .start()
        .await
        .expect("start PostgreSQL container");
    let port = node.get_host_port_ipv4(5432).await.expect("mapped port");

    // Plain connection.
    let mut session = connect(&config(port, TlsMode::Disable), PASSWORD)
        .await
        .expect("connect");
    session.ping().await.expect("ping");
    let info = session.server_info().await.expect("server info");
    assert_eq!(info.product, "PostgreSQL");
    assert!(!info.version.is_empty());
    assert_eq!(info.encrypted, Some(false));
    assert!(!session.is_closed());

    // `prefer` falls back to plain text on a server without TLS.
    let mut session = connect(&config(port, TlsMode::Prefer), PASSWORD)
        .await
        .expect("connect with prefer");
    assert_eq!(
        session.server_info().await.expect("info").encrypted,
        Some(false)
    );

    // `require` fails on a server without TLS.
    let err = connect(&config(port, TlsMode::Require), PASSWORD)
        .await
        .err();
    assert!(matches!(err, Some(DbError::Tls(_))), "{err:?}");

    // Wrong password.
    let err = connect(&config(port, TlsMode::Disable), "wrong")
        .await
        .err();
    assert!(matches!(err, Some(DbError::Authentication(_))), "{err:?}");

    // Unknown database: a server error with its SQLSTATE.
    let mut cfg = config(port, TlsMode::Disable);
    cfg.database = Some("missing_db".into());
    let err = connect(&cfg, PASSWORD).await.err();
    assert!(
        matches!(&err, Some(DbError::Server { sqlstate, .. }) if sqlstate == "3D000"),
        "{err:?}"
    );

    // The read-only setting is applied at connection time (CON-12); write
    // rejection is tested once query execution exists (J3).
    let mut cfg = config(port, TlsMode::Disable);
    cfg.read_only = true;
    let session = connect(&cfg, PASSWORD).await.expect("read-only connect");
    drop(session);
}

/// Fixture schema exercising every kind of object of the tree.
const FIXTURE: &str = r#"
CREATE SCHEMA shop;
CREATE TABLE shop.customer (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    email text NOT NULL UNIQUE,
    "Display Name" varchar(80) DEFAULT 'anonymous'
);
COMMENT ON TABLE shop.customer IS 'People who buy';
COMMENT ON COLUMN shop.customer.email IS 'Login';
CREATE TABLE shop."order" (
    id serial PRIMARY KEY,
    customer_id bigint NOT NULL REFERENCES shop.customer (id) ON DELETE CASCADE,
    total numeric(10, 2) CHECK (total >= 0)
);
CREATE INDEX order_customer_idx ON shop."order" (customer_id);
CREATE FUNCTION shop.touch() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$;
CREATE TRIGGER order_touch BEFORE UPDATE ON shop."order" FOR EACH ROW EXECUTE FUNCTION shop.touch();
CREATE VIEW shop.big_orders AS SELECT * FROM shop."order" WHERE total > 100;
CREATE MATERIALIZED VIEW shop.order_count AS SELECT count(*) AS n FROM shop."order";
CREATE SEQUENCE shop.invoice_no START 1000;
CREATE FUNCTION shop.add(a integer, b integer) RETURNS integer LANGUAGE sql AS 'SELECT a + b';
CREATE FUNCTION shop.add(a text, b text) RETURNS text LANGUAGE sql AS 'SELECT a || b';
CREATE PROCEDURE shop.reset() LANGUAGE sql AS 'DELETE FROM shop."order"';
"#;

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_metadata() {
    use strata_core::{ConstraintKind, ObjectKind, ObjectRef};

    let node = Postgres::default()
        .with_password(PASSWORD)
        .with_tag(IMAGE_TAG)
        .start()
        .await
        .expect("start PostgreSQL container");
    let port = node.get_host_port_ipv4(5432).await.expect("mapped port");

    let (client, connection) = tokio_postgres::connect(
        &format!("host=127.0.0.1 port={port} user=postgres password={PASSWORD} dbname=postgres"),
        tokio_postgres::NoTls,
    )
    .await
    .expect("fixture connection");
    tokio::spawn(connection);
    client.batch_execute(FIXTURE).await.expect("fixture");

    let mut s = connect(&config(port, TlsMode::Disable), PASSWORD)
        .await
        .expect("connect");
    assert_eq!(s.server_info().await.expect("info").database, "postgres");
    assert!(
        s.list_databases()
            .await
            .expect("dbs")
            .contains(&"postgres".to_owned())
    );
    let schemas = s.list_schemas().await.expect("schemas");
    assert!(schemas.contains(&"shop".to_owned()) && schemas.contains(&"public".to_owned()));
    assert!(
        !schemas
            .iter()
            .any(|n| n.starts_with("pg_") || n == "information_schema")
    );

    let names = |objects: Vec<strata_core::ObjectInfo>| -> Vec<String> {
        objects.into_iter().map(|o| o.name).collect()
    };
    assert_eq!(
        names(
            s.list_objects("shop", ObjectKind::Table)
                .await
                .expect("tables")
        ),
        ["customer", "order"]
    );
    assert_eq!(
        names(
            s.list_objects("shop", ObjectKind::View)
                .await
                .expect("views")
        ),
        ["big_orders"]
    );
    assert_eq!(
        names(
            s.list_objects("shop", ObjectKind::MaterializedView)
                .await
                .expect("mv")
        ),
        ["order_count"]
    );
    let sequences = names(
        s.list_objects("shop", ObjectKind::Sequence)
            .await
            .expect("seq"),
    );
    assert!(
        sequences.contains(&"invoice_no".to_owned()),
        "{sequences:?}"
    );
    let functions = s
        .list_objects("shop", ObjectKind::Function)
        .await
        .expect("fn");
    let signatures: Vec<_> = functions
        .iter()
        .map(|f| format!("{}({})", f.name, f.signature.clone().unwrap_or_default()))
        .collect();
    assert_eq!(
        signatures,
        [
            "add(a integer, b integer)",
            "add(a text, b text)",
            "touch()"
        ]
    );
    assert_eq!(
        names(
            s.list_objects("shop", ObjectKind::Procedure)
                .await
                .expect("proc")
        ),
        ["reset"]
    );

    let customer = s
        .describe_table("shop", "customer")
        .await
        .expect("describe");
    assert_eq!(customer.comment.as_deref(), Some("People who buy"));
    let cols: Vec<_> = customer.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(cols, ["id", "email", "Display Name"]);
    assert!(customer.columns[0].primary_key);
    assert_eq!(
        customer.columns[0].default.as_deref(),
        Some("GENERATED ALWAYS AS IDENTITY")
    );
    assert_eq!(customer.columns[1].comment.as_deref(), Some("Login"));
    assert!(!customer.columns[1].nullable);
    assert_eq!(customer.columns[2].data_type, "character varying(80)");

    let order = s
        .describe_table("shop", "order")
        .await
        .expect("describe order");
    let fk = order
        .constraints
        .iter()
        .find(|c| c.kind == ConstraintKind::ForeignKey)
        .expect("foreign key");
    assert_eq!(fk.columns, ["customer_id"]);
    let reference = fk.references.as_ref().expect("reference");
    assert_eq!(
        (reference.schema.as_str(), reference.table.as_str()),
        ("shop", "customer")
    );
    assert!(
        order
            .constraints
            .iter()
            .any(|c| c.kind == ConstraintKind::Check)
    );
    assert!(
        order
            .indexes
            .iter()
            .any(|i| i.name == "order_customer_idx" && !i.constraint_backed)
    );
    assert_eq!(order.triggers.len(), 1);

    // The generated DDL recreates an identical table in another schema.
    let object = |name: &str, kind, signature: Option<&str>| ObjectRef {
        database: "postgres".into(),
        schema: "shop".into(),
        name: name.into(),
        kind,
        signature: signature.map(str::to_owned),
    };
    let ddl = s
        .object_ddl(&object("customer", ObjectKind::Table, None))
        .await
        .expect("table ddl");
    assert!(
        ddl.contains("\"Display Name\" character varying(80) DEFAULT"),
        "{ddl}"
    );
    client
        .batch_execute(&format!(
            "CREATE SCHEMA copy; {}",
            ddl.replace("shop.", "copy.")
                .replace("customer_pkey", "c_pkey")
                .replace("customer_email_key", "c_email_key")
        ))
        .await
        .expect("replay DDL");
    let copy = s
        .describe_table("copy", "customer")
        .await
        .expect("describe copy");
    assert_eq!(copy.columns, customer.columns);

    for (name, kind, signature, expected) in [
        (
            "big_orders",
            ObjectKind::View,
            None,
            "CREATE OR REPLACE VIEW shop.big_orders AS",
        ),
        (
            "order_count",
            ObjectKind::MaterializedView,
            None,
            "CREATE MATERIALIZED VIEW",
        ),
        ("invoice_no", ObjectKind::Sequence, None, "START WITH 1000"),
        (
            "add",
            ObjectKind::Function,
            Some("a text, b text"),
            "a || b",
        ),
        (
            "reset",
            ObjectKind::Procedure,
            Some(""),
            "CREATE OR REPLACE PROCEDURE",
        ),
    ] {
        let ddl = s
            .object_ddl(&object(name, kind, signature))
            .await
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(ddl.contains(expected), "{name}: {ddl}");
    }

    let missing = s.describe_table("shop", "nope").await.err();
    assert!(matches!(missing, Some(DbError::NotFound(_))), "{missing:?}");
}
