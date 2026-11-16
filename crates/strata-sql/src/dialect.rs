// SPDX-License-Identifier: GPL-3.0-or-later
//! SQL dialect differences.

use strata_core::DbKind;

/// Dialect-specific SQL generation.
pub trait Dialect: Send + Sync {
    /// Quotes `ident` if needed so that it is read back verbatim.
    fn quote_identifier(&self, ident: &str) -> String;

    /// `schema.name`, each part quoted if needed.
    fn qualified_name(&self, schema: &str, name: &str) -> String {
        format!(
            "{}.{}",
            self.quote_identifier(schema),
            self.quote_identifier(name)
        )
    }
}

/// Dialect of `kind`.
pub fn dialect_for(kind: DbKind) -> &'static dyn Dialect {
    match kind {
        DbKind::Postgres => &PostgresDialect,
        DbKind::MySql => &MySqlDialect,
    }
}

/// PostgreSQL: `"double quotes"`, unquoted identifiers fold to lower case.
#[derive(Debug, Clone, Copy, Default)]
pub struct PostgresDialect;

/// Reserved key words that cannot be used unquoted as identifiers
/// (PostgreSQL documentation, appendix C, "reserved" and
/// "reserved (can be function or type)").
const PG_RESERVED: &[&str] = &[
    "all",
    "analyse",
    "analyze",
    "and",
    "any",
    "array",
    "as",
    "asc",
    "asymmetric",
    "authorization",
    "binary",
    "both",
    "case",
    "cast",
    "check",
    "collate",
    "collation",
    "column",
    "concurrently",
    "constraint",
    "create",
    "cross",
    "current_catalog",
    "current_date",
    "current_role",
    "current_schema",
    "current_time",
    "current_timestamp",
    "current_user",
    "default",
    "deferrable",
    "desc",
    "distinct",
    "do",
    "else",
    "end",
    "except",
    "false",
    "fetch",
    "for",
    "foreign",
    "freeze",
    "from",
    "full",
    "grant",
    "group",
    "having",
    "ilike",
    "in",
    "initially",
    "inner",
    "intersect",
    "into",
    "is",
    "isnull",
    "join",
    "lateral",
    "leading",
    "left",
    "like",
    "limit",
    "localtime",
    "localtimestamp",
    "natural",
    "not",
    "notnull",
    "null",
    "offset",
    "on",
    "only",
    "or",
    "order",
    "outer",
    "overlaps",
    "placing",
    "primary",
    "references",
    "returning",
    "right",
    "select",
    "session_user",
    "similar",
    "some",
    "symmetric",
    "system_user",
    "table",
    "tablesample",
    "then",
    "to",
    "trailing",
    "true",
    "union",
    "unique",
    "user",
    "using",
    "variadic",
    "verbose",
    "when",
    "where",
    "window",
    "with",
];

impl Dialect for PostgresDialect {
    fn quote_identifier(&self, ident: &str) -> String {
        let mut chars = ident.chars();
        let simple = chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '$');
        if simple && !PG_RESERVED.contains(&ident) {
            ident.to_owned()
        } else {
            format!("\"{}\"", ident.replace('"', "\"\""))
        }
    }
}

/// MySQL / MariaDB: `` `backticks` ``.
#[derive(Debug, Clone, Copy, Default)]
pub struct MySqlDialect;

impl Dialect for MySqlDialect {
    fn quote_identifier(&self, ident: &str) -> String {
        format!("`{}`", ident.replace('`', "``"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_quotes_only_when_needed() {
        let d = PostgresDialect;
        assert_eq!(d.quote_identifier("users"), "users");
        assert_eq!(d.quote_identifier("_tmp1$"), "_tmp1$");
        assert_eq!(d.quote_identifier("Users"), "\"Users\"");
        assert_eq!(d.quote_identifier("order"), "\"order\"");
        assert_eq!(d.quote_identifier("my table"), "\"my table\"");
        assert_eq!(d.quote_identifier("1abc"), "\"1abc\"");
        assert_eq!(d.quote_identifier("a\"b"), "\"a\"\"b\"");
        assert_eq!(d.quote_identifier(""), "\"\"");
        assert_eq!(d.qualified_name("public", "Order"), "public.\"Order\"");
    }

    #[test]
    fn mysql_always_quotes() {
        assert_eq!(MySqlDialect.quote_identifier("a`b"), "`a``b`");
    }
}
