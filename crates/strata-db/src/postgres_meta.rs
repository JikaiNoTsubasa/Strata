// SPDX-License-Identifier: GPL-3.0-or-later
//! PostgreSQL catalogue queries (NAV-1 to NAV-6). Every query is
//! parameterised; identifiers in generated DDL are quoted by the dialect.

use std::fmt::Write as _;

use strata_core::{
    ColumnInfo, ConstraintInfo, ConstraintKind, IndexInfo, ObjectInfo, ObjectKind, ObjectRef,
    Reference, TableDetails, TriggerInfo,
};
use strata_sql::{Dialect, PostgresDialect};
use tokio_postgres::Client;
use tokio_postgres::types::Oid;

use crate::postgres::map_error;
use crate::{DbError, Result};

/// Schemas hidden from the tree (system and temporary schemas).
const HIDDEN_SCHEMAS: &str = "n.nspname NOT LIKE 'pg\\_%' AND n.nspname <> 'information_schema'";

pub(crate) async fn list_databases(client: &Client) -> Result<Vec<String>> {
    let rows = client
        .query(
            "SELECT datname::text FROM pg_database \
             WHERE datallowconn AND NOT datistemplate ORDER BY datname",
            &[],
        )
        .await
        .map_err(map_error)?;
    rows.iter()
        .map(|r| r.try_get(0).map_err(map_error))
        .collect()
}

pub(crate) async fn list_schemas(client: &Client) -> Result<Vec<String>> {
    let rows = client
        .query(
            &format!(
                "SELECT n.nspname::text FROM pg_namespace n WHERE {HIDDEN_SCHEMAS} ORDER BY n.nspname"
            ),
            &[],
        )
        .await
        .map_err(map_error)?;
    rows.iter()
        .map(|r| r.try_get(0).map_err(map_error))
        .collect()
}

pub(crate) async fn list_objects(
    client: &Client,
    schema: &str,
    kind: ObjectKind,
) -> Result<Vec<ObjectInfo>> {
    let rows = match kind {
        ObjectKind::Function | ObjectKind::Procedure => {
            let prokind = if kind == ObjectKind::Function {
                "f"
            } else {
                "p"
            };
            client
                .query(
                    "SELECT p.proname::text, pg_get_function_identity_arguments(p.oid), \
                            obj_description(p.oid, 'pg_proc') \
                     FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                     WHERE n.nspname = $1 AND p.prokind::text = $2 \
                     ORDER BY p.proname, 2",
                    &[&schema, &prokind],
                )
                .await
        }
        _ => {
            let relkinds: &[&str] = match kind {
                ObjectKind::Table => &["r", "p", "f"],
                ObjectKind::View => &["v"],
                ObjectKind::MaterializedView => &["m"],
                _ => &["S"],
            };
            client
                .query(
                    "SELECT c.relname::text, NULL::text, obj_description(c.oid, 'pg_class') \
                     FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname = $1 AND c.relkind::text = ANY($2) AND NOT c.relispartition \
                     ORDER BY c.relname",
                    &[&schema, &relkinds],
                )
                .await
        }
    }
    .map_err(map_error)?;
    rows.iter()
        .map(|r| {
            Ok(ObjectInfo {
                name: r.try_get(0).map_err(map_error)?,
                kind,
                signature: r.try_get(1).map_err(map_error)?,
                comment: r.try_get(2).map_err(map_error)?,
            })
        })
        .collect()
}

async fn relation_oid(client: &Client, schema: &str, name: &str) -> Result<Oid> {
    client
        .query_opt(
            "SELECT c.oid FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2",
            &[&schema, &name],
        )
        .await
        .map_err(map_error)?
        .map(|r| r.try_get(0).map_err(map_error))
        .transpose()?
        .ok_or_else(|| DbError::NotFound(format!("{schema}.{name}")))
}

pub(crate) async fn describe_table(
    client: &Client,
    database: &str,
    schema: &str,
    name: &str,
) -> Result<TableDetails> {
    let oid = relation_oid(client, schema, name).await?;

    let header = client
        .query_one(
            "SELECT c.relkind::text, \
                    CASE WHEN c.reltuples < 0 THEN NULL ELSE c.reltuples::bigint END, \
                    pg_total_relation_size(c.oid), obj_description(c.oid, 'pg_class') \
             FROM pg_class c WHERE c.oid = $1",
            &[&oid],
        )
        .await
        .map_err(map_error)?;
    let relkind: String = header.try_get(0).map_err(map_error)?;
    let kind = match relkind.as_str() {
        "v" => ObjectKind::View,
        "m" => ObjectKind::MaterializedView,
        "S" => ObjectKind::Sequence,
        _ => ObjectKind::Table,
    };

    let constraints = constraints(client, oid).await?;
    let pk_columns: Vec<String> = constraints
        .iter()
        .filter(|c| c.kind == ConstraintKind::PrimaryKey)
        .flat_map(|c| c.columns.clone())
        .collect();

    let columns = client
        .query(
            "SELECT a.attname::text, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull, \
                    CASE \
                      WHEN a.attidentity = 'a' THEN 'GENERATED ALWAYS AS IDENTITY' \
                      WHEN a.attidentity = 'd' THEN 'GENERATED BY DEFAULT AS IDENTITY' \
                      WHEN a.attgenerated = 's' \
                        THEN 'GENERATED ALWAYS AS (' || pg_get_expr(d.adbin, d.adrelid) || ') STORED' \
                      ELSE pg_get_expr(d.adbin, d.adrelid) \
                    END, \
                    col_description(a.attrelid, a.attnum) \
             FROM pg_attribute a \
             LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
             WHERE a.attrelid = $1 AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
            &[&oid],
        )
        .await
        .map_err(map_error)?
        .iter()
        .map(|r| {
            let name: String = r.try_get(0).map_err(map_error)?;
            Ok(ColumnInfo {
                primary_key: pk_columns.contains(&name),
                name,
                data_type: r.try_get(1).map_err(map_error)?,
                nullable: r.try_get(2).map_err(map_error)?,
                default: r.try_get(3).map_err(map_error)?,
                comment: r.try_get(4).map_err(map_error)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let indexes = client
        .query(
            "SELECT i.relname::text, pg_get_indexdef(i.oid), x.indisunique, x.indisprimary, \
                    EXISTS (SELECT 1 FROM pg_constraint k WHERE k.conindid = i.oid \
                            AND k.contype IN ('p', 'u', 'x')) \
             FROM pg_index x JOIN pg_class i ON i.oid = x.indexrelid \
             WHERE x.indrelid = $1 ORDER BY i.relname",
            &[&oid],
        )
        .await
        .map_err(map_error)?
        .iter()
        .map(|r| {
            Ok(IndexInfo {
                name: r.try_get(0).map_err(map_error)?,
                definition: r.try_get(1).map_err(map_error)?,
                unique: r.try_get(2).map_err(map_error)?,
                primary: r.try_get(3).map_err(map_error)?,
                constraint_backed: r.try_get(4).map_err(map_error)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let triggers = client
        .query(
            "SELECT tgname::text, pg_get_triggerdef(oid, true) FROM pg_trigger \
             WHERE tgrelid = $1 AND NOT tgisinternal ORDER BY tgname",
            &[&oid],
        )
        .await
        .map_err(map_error)?
        .iter()
        .map(|r| {
            Ok(TriggerInfo {
                name: r.try_get(0).map_err(map_error)?,
                definition: r.try_get(1).map_err(map_error)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(TableDetails {
        object: ObjectRef {
            database: database.to_owned(),
            schema: schema.to_owned(),
            name: name.to_owned(),
            kind,
            signature: None,
        },
        columns,
        indexes,
        constraints,
        triggers,
        comment: header.try_get(3).map_err(map_error)?,
        estimated_rows: header.try_get(1).map_err(map_error)?,
        total_size: header.try_get(2).map_err(map_error)?,
    })
}

async fn constraints(client: &Client, oid: Oid) -> Result<Vec<ConstraintInfo>> {
    let rows = client
        .query(
            "SELECT con.conname::text, con.contype::text, pg_get_constraintdef(con.oid, true), \
                    ARRAY(SELECT a.attname::text FROM unnest(con.conkey) WITH ORDINALITY k(n, o) \
                          JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.n \
                          ORDER BY k.o), \
                    fn.nspname::text, fc.relname::text, \
                    ARRAY(SELECT a.attname::text FROM unnest(con.confkey) WITH ORDINALITY k(n, o) \
                          JOIN pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = k.n \
                          ORDER BY k.o) \
             FROM pg_constraint con \
             LEFT JOIN pg_class fc ON fc.oid = con.confrelid \
             LEFT JOIN pg_namespace fn ON fn.oid = fc.relnamespace \
             WHERE con.conrelid = $1 \
             ORDER BY CASE con.contype WHEN 'p' THEN 0 WHEN 'f' THEN 1 WHEN 'u' THEN 2 ELSE 3 END, \
                      con.conname",
            &[&oid],
        )
        .await
        .map_err(map_error)?;
    rows.iter()
        .map(|r| {
            let contype: String = r.try_get(1).map_err(map_error)?;
            let kind = match contype.as_str() {
                "p" => ConstraintKind::PrimaryKey,
                "f" => ConstraintKind::ForeignKey,
                "u" => ConstraintKind::Unique,
                "c" => ConstraintKind::Check,
                "x" => ConstraintKind::Exclusion,
                _ => ConstraintKind::Other,
            };
            let ref_schema: Option<String> = r.try_get(4).map_err(map_error)?;
            let ref_table: Option<String> = r.try_get(5).map_err(map_error)?;
            let references = match (ref_schema, ref_table) {
                (Some(schema), Some(table)) => Some(Reference {
                    schema,
                    table,
                    columns: r.try_get(6).map_err(map_error)?,
                }),
                _ => None,
            };
            Ok(ConstraintInfo {
                name: r.try_get(0).map_err(map_error)?,
                kind,
                definition: r.try_get(2).map_err(map_error)?,
                columns: r.try_get(3).map_err(map_error)?,
                references,
            })
        })
        .collect()
}

/// DDL of an object (NAV-5).
pub(crate) async fn object_ddl(client: &Client, object: &ObjectRef) -> Result<String> {
    let d = PostgresDialect;
    let qualified = d.qualified_name(&object.schema, &object.name);
    match object.kind {
        ObjectKind::Table => {
            let details =
                describe_table(client, &object.database, &object.schema, &object.name).await?;
            Ok(table_ddl(&details))
        }
        ObjectKind::View | ObjectKind::MaterializedView => {
            let oid = relation_oid(client, &object.schema, &object.name).await?;
            let row = client
                .query_one(
                    "SELECT pg_get_viewdef($1::oid, true), obj_description($1::oid, 'pg_class')",
                    &[&oid],
                )
                .await
                .map_err(map_error)?;
            let body: String = row.try_get(0).map_err(map_error)?;
            let comment: Option<String> = row.try_get(1).map_err(map_error)?;
            let (create, object_type) = if object.kind == ObjectKind::View {
                ("CREATE OR REPLACE VIEW", "VIEW")
            } else {
                ("CREATE MATERIALIZED VIEW", "MATERIALIZED VIEW")
            };
            let mut ddl = format!("{create} {qualified} AS\n{}", body.trim_end());
            if !ddl.ends_with(';') {
                ddl.push(';');
            }
            ddl.push('\n');
            if let Some(comment) = comment {
                let _ = writeln!(
                    ddl,
                    "\nCOMMENT ON {object_type} {qualified} IS {};",
                    literal(&comment)
                );
            }
            Ok(ddl)
        }
        ObjectKind::Sequence => {
            let row = client
                .query_opt(
                    "SELECT data_type::text, start_value, min_value, max_value, increment_by, \
                            cycle, cache_size \
                     FROM pg_sequences WHERE schemaname = $1 AND sequencename = $2",
                    &[&object.schema, &object.name],
                )
                .await
                .map_err(map_error)?
                .ok_or_else(|| DbError::NotFound(qualified.clone()))?;
            let get = |i: usize| -> Result<i64> { row.try_get(i).map_err(map_error) };
            let cycle: bool = row.try_get(5).map_err(map_error)?;
            let data_type: String = row.try_get(0).map_err(map_error)?;
            Ok(format!(
                "CREATE SEQUENCE {qualified}\n    AS {data_type}\n    INCREMENT BY {}\n    \
                 MINVALUE {}\n    MAXVALUE {}\n    START WITH {}\n    CACHE {}\n    {}CYCLE;\n",
                get(4)?,
                get(2)?,
                get(3)?,
                get(1)?,
                get(6)?,
                if cycle { "" } else { "NO " }
            ))
        }
        ObjectKind::Function | ObjectKind::Procedure => {
            let signature = object.signature.clone().unwrap_or_default();
            let row = client
                .query_opt(
                    "SELECT pg_get_functiondef(p.oid) \
                     FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                     WHERE n.nspname = $1 AND p.proname = $2 \
                       AND pg_get_function_identity_arguments(p.oid) = $3",
                    &[&object.schema, &object.name, &signature],
                )
                .await
                .map_err(map_error)?
                .ok_or_else(|| DbError::NotFound(format!("{qualified}({signature})")))?;
            let mut ddl: String = row.try_get(0).map_err(map_error)?;
            let trimmed = ddl.trim_end().len();
            ddl.truncate(trimmed);
            ddl.push_str(";\n");
            Ok(ddl)
        }
    }
}

/// SQL string literal.
fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Rebuilds `CREATE TABLE` and its dependent objects from the catalogue.
pub(crate) fn table_ddl(t: &TableDetails) -> String {
    let d = PostgresDialect;
    let table = d.qualified_name(&t.object.schema, &t.object.name);
    let mut lines: Vec<String> = t
        .columns
        .iter()
        .map(|c| {
            let mut line = format!("    {} {}", d.quote_identifier(&c.name), c.data_type);
            match c.default.as_deref() {
                Some(g) if g.starts_with("GENERATED") => {
                    line.push(' ');
                    line.push_str(g);
                }
                Some(default) => {
                    line.push_str(" DEFAULT ");
                    line.push_str(default);
                }
                None => {}
            }
            if !c.nullable {
                line.push_str(" NOT NULL");
            }
            line
        })
        .collect();
    lines.extend(
        t.constraints
            .iter()
            .filter(|c| c.kind != ConstraintKind::Other)
            .map(|c| {
                format!(
                    "    CONSTRAINT {} {}",
                    d.quote_identifier(&c.name),
                    c.definition
                )
            }),
    );

    let mut ddl = format!("CREATE TABLE {table} (\n{}\n);\n", lines.join(",\n"));
    for index in t.indexes.iter().filter(|i| !i.constraint_backed) {
        let _ = writeln!(ddl, "\n{};", index.definition);
    }
    for trigger in &t.triggers {
        let _ = writeln!(ddl, "\n{};", trigger.definition);
    }
    if let Some(comment) = &t.comment {
        let _ = writeln!(ddl, "\nCOMMENT ON TABLE {table} IS {};", literal(comment));
    }
    for column in &t.columns {
        if let Some(comment) = &column.comment {
            let _ = writeln!(
                ddl,
                "COMMENT ON COLUMN {table}.{} IS {};",
                d.quote_identifier(&column.name),
                literal(comment)
            );
        }
    }
    ddl
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_ddl_layout() {
        let t = TableDetails {
            object: ObjectRef {
                database: "db".into(),
                schema: "public".into(),
                name: "Order".into(),
                kind: ObjectKind::Table,
                signature: None,
            },
            columns: vec![
                ColumnInfo {
                    name: "id".into(),
                    data_type: "bigint".into(),
                    nullable: false,
                    default: Some("GENERATED ALWAYS AS IDENTITY".into()),
                    comment: None,
                    primary_key: true,
                },
                ColumnInfo {
                    name: "label".into(),
                    data_type: "text".into(),
                    nullable: true,
                    default: Some("'n/a'::text".into()),
                    comment: Some("it's a label".into()),
                    primary_key: false,
                },
            ],
            indexes: vec![
                IndexInfo {
                    name: "Order_pkey".into(),
                    definition: "CREATE UNIQUE INDEX ...".into(),
                    unique: true,
                    primary: true,
                    constraint_backed: true,
                },
                IndexInfo {
                    name: "order_label_idx".into(),
                    definition:
                        "CREATE INDEX order_label_idx ON public.\"Order\" USING btree (label)"
                            .into(),
                    unique: false,
                    primary: false,
                    constraint_backed: false,
                },
            ],
            constraints: vec![ConstraintInfo {
                name: "Order_pkey".into(),
                kind: ConstraintKind::PrimaryKey,
                columns: vec!["id".into()],
                references: None,
                definition: "PRIMARY KEY (id)".into(),
            }],
            triggers: vec![],
            comment: None,
            estimated_rows: None,
            total_size: None,
        };
        assert_eq!(
            table_ddl(&t),
            "CREATE TABLE public.\"Order\" (\n    id bigint GENERATED ALWAYS AS IDENTITY NOT NULL,\n    \
             label text DEFAULT 'n/a'::text,\n    CONSTRAINT \"Order_pkey\" PRIMARY KEY (id)\n);\n\n\
             CREATE INDEX order_label_idx ON public.\"Order\" USING btree (label);\n\
             COMMENT ON COLUMN public.\"Order\".label IS 'it''s a label';\n"
        );
    }
}
