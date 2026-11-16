// SPDX-License-Identifier: GPL-3.0-or-later
//! Open connections and their physical sessions.
//!
//! A connected connection ([`Live`]) keeps one physical session per database
//! being browsed (PostgreSQL sessions are bound to a database). Sessions are
//! closed when idle (CON-11) and reopened transparently on next use;
//! a lost session is reopened once (CON-10).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use strata_core::{ConnectionConfig, ObjectRef, Secret, ServerInfo, TableDetails, TablePart};
use strata_db::DbSession;
use tokio::sync::Mutex;

use crate::protocol::{MetaHint, MetaItem, MetaRequest};
use crate::tunnel::Tunnel;

/// A physical session and its last use.
pub(crate) struct Slot {
    pub(crate) session: Option<Box<dyn DbSession>>,
    pub(crate) last_used: Instant,
}

/// A connected connection.
pub(crate) struct Live {
    pub(crate) cfg: ConnectionConfig,
    /// Password used to open the sessions, kept in memory (zeroized on drop)
    /// to reopen them.
    pub(crate) password: Option<Secret>,
    /// SSH password or passphrase, kept to reopen the tunnel.
    pub(crate) ssh_secret: Option<Secret>,
    /// SSH tunnel, reopened when it dies.
    pub(crate) tunnel: Mutex<Option<Tunnel>>,
    /// Server description from the main session.
    pub(crate) info: ServerInfo,
    slots: Mutex<HashMap<String, Arc<Mutex<Slot>>>>,
}

impl Live {
    pub(crate) fn new(
        cfg: ConnectionConfig,
        password: Option<Secret>,
        ssh_secret: Option<Secret>,
        tunnel: Option<Tunnel>,
        info: ServerInfo,
        session: Box<dyn DbSession>,
    ) -> Self {
        let mut slots = HashMap::new();
        slots.insert(
            info.database.clone(),
            Arc::new(Mutex::new(Slot {
                session: Some(session),
                last_used: Instant::now(),
            })),
        );
        Self {
            cfg,
            password,
            ssh_secret,
            tunnel: Mutex::new(tunnel),
            info,
            slots: Mutex::new(slots),
        }
    }

    /// Slot of `database`, created empty if needed.
    pub(crate) async fn slot(&self, database: &str) -> Arc<Mutex<Slot>> {
        let mut slots = self.slots.lock().await;
        Arc::clone(slots.entry(database.to_owned()).or_insert_with(|| {
            Arc::new(Mutex::new(Slot {
                session: None,
                last_used: Instant::now(),
            }))
        }))
    }

    /// Closes the sessions unused for `timeout`; busy sessions are skipped.
    pub(crate) async fn close_idle(&self, timeout: Duration) {
        let slots: Vec<_> = self.slots.lock().await.values().cloned().collect();
        for slot in slots {
            if let Ok(mut slot) = slot.try_lock()
                && slot.session.is_some()
                && slot.last_used.elapsed() >= timeout
            {
                tracing::debug!("closing idle session");
                slot.session = None;
            }
        }
    }
}

/// Operation run on a session.
#[derive(Debug, Clone)]
pub(crate) enum Op {
    Meta(MetaRequest),
    Describe(ObjectRef),
    Ddl(ObjectRef),
}

/// Result of an [`Op`].
pub(crate) enum OpOutput {
    Items(Vec<MetaItem>),
    Table(Box<TableDetails>),
    Ddl(String),
}

impl Op {
    /// Database the operation needs a session on.
    pub(crate) fn database<'a>(&'a self, live: &'a Live) -> &'a str {
        match self {
            Op::Meta(MetaRequest::Databases) => &live.info.database,
            Op::Meta(
                MetaRequest::Schemas { database }
                | MetaRequest::Objects { database, .. }
                | MetaRequest::TablePart { database, .. },
            ) => database,
            Op::Describe(object) | Op::Ddl(object) => &object.database,
        }
    }

    pub(crate) async fn run(
        &self,
        session: &mut dyn DbSession,
        main_database: &str,
    ) -> strata_db::Result<OpOutput> {
        match self {
            Op::Meta(request) => run_meta(session, request, main_database)
                .await
                .map(OpOutput::Items),
            Op::Describe(object) => session
                .describe_table(&object.schema, &object.name)
                .await
                .map(|t| OpOutput::Table(Box::new(t))),
            Op::Ddl(object) => session.object_ddl(object).await.map(OpOutput::Ddl),
        }
    }
}

fn item(name: String, hint: MetaHint) -> MetaItem {
    MetaItem {
        name,
        hint,
        detail: None,
        comment: None,
        signature: None,
    }
}

async fn run_meta(
    session: &mut dyn DbSession,
    request: &MetaRequest,
    main_database: &str,
) -> strata_db::Result<Vec<MetaItem>> {
    Ok(match request {
        MetaRequest::Databases => session
            .list_databases()
            .await?
            .into_iter()
            .map(|name| {
                let current = name == main_database;
                item(name, MetaHint::Database { current })
            })
            .collect(),
        MetaRequest::Schemas { .. } => session
            .list_schemas()
            .await?
            .into_iter()
            .map(|name| item(name, MetaHint::Schema))
            .collect(),
        MetaRequest::Objects { schema, kind, .. } => session
            .list_objects(schema, *kind)
            .await?
            .into_iter()
            .map(|o| MetaItem {
                name: o.name,
                hint: MetaHint::Object(o.kind),
                detail: None,
                comment: o.comment,
                signature: o.signature,
            })
            .collect(),
        MetaRequest::TablePart {
            schema,
            table,
            part,
            ..
        } => {
            let t = session.describe_table(schema, table).await?;
            match part {
                TablePart::Columns => t
                    .columns
                    .into_iter()
                    .map(|c| MetaItem {
                        detail: Some(if c.nullable {
                            c.data_type
                        } else {
                            format!("{} NOT NULL", c.data_type)
                        }),
                        hint: MetaHint::Column {
                            primary_key: c.primary_key,
                        },
                        name: c.name,
                        comment: c.comment,
                        signature: None,
                    })
                    .collect(),
                TablePart::Indexes => t
                    .indexes
                    .into_iter()
                    .map(|i| MetaItem {
                        // "CREATE INDEX x ON t USING btree (a)" → "btree (a)"
                        detail: i
                            .definition
                            .split_once(" USING ")
                            .map(|(_, rest)| rest.to_owned()),
                        hint: MetaHint::Index { unique: i.unique },
                        name: i.name,
                        comment: Some(i.definition),
                        signature: None,
                    })
                    .collect(),
                TablePart::Constraints => t
                    .constraints
                    .into_iter()
                    .map(|c| MetaItem {
                        detail: Some(c.definition.clone()),
                        hint: MetaHint::Constraint(c.kind),
                        name: c.name,
                        comment: Some(c.definition),
                        signature: None,
                    })
                    .collect(),
                TablePart::Triggers => t
                    .triggers
                    .into_iter()
                    .map(|tr| MetaItem {
                        hint: MetaHint::Trigger,
                        name: tr.name,
                        detail: None,
                        comment: Some(tr.definition),
                        signature: None,
                    })
                    .collect(),
            }
        }
    })
}
