// SPDX-License-Identifier: GPL-3.0-or-later
//! SQL tooling: dialects (identifier quoting…), and from milestone J3 script
//! splitting, classification, formatting and completion
//! (see `docs/ADR/0010-sql-splitting-and-completion.md`).

mod dialect;

pub use dialect::{Dialect, MySqlDialect, PostgresDialect, dialect_for};
