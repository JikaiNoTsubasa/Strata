// SPDX-License-Identifier: GPL-3.0-or-later
//! Schema metadata (NAV-1 to NAV-6).

/// Kind of a schema object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    /// Table (including partitioned and foreign tables).
    Table,
    /// View.
    View,
    /// Materialized view.
    MaterializedView,
    /// Sequence.
    Sequence,
    /// Function.
    Function,
    /// Procedure.
    Procedure,
}

impl ObjectKind {
    /// Every kind, in tree order.
    pub const ALL: [ObjectKind; 6] = [
        Self::Table,
        Self::View,
        Self::MaterializedView,
        Self::Sequence,
        Self::Function,
        Self::Procedure,
    ];

    /// Whether objects of this kind have columns.
    pub fn has_columns(self) -> bool {
        matches!(self, Self::Table | Self::View | Self::MaterializedView)
    }
}

/// Fully identified schema object.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectRef {
    /// Database.
    pub database: String,
    /// Schema.
    pub schema: String,
    /// Name.
    pub name: String,
    /// Kind.
    pub kind: ObjectKind,
    /// Argument types of functions/procedures (`integer, text`), which
    /// distinguish overloads.
    pub signature: Option<String>,
}

/// Object listed in a schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectInfo {
    /// Name.
    pub name: String,
    /// Kind.
    pub kind: ObjectKind,
    /// Arguments of functions/procedures.
    pub signature: Option<String>,
    /// Comment.
    pub comment: Option<String>,
}

/// Part of a table shown as a folder in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TablePart {
    /// Columns.
    Columns,
    /// Indexes.
    Indexes,
    /// Constraints (primary, foreign, unique, check, exclusion keys).
    Constraints,
    /// Triggers.
    Triggers,
}

/// A table column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    /// Name.
    pub name: String,
    /// Type as written in SQL (`character varying(80)`).
    pub data_type: String,
    /// Whether NULL is allowed.
    pub nullable: bool,
    /// Default expression or identity/generation clause.
    pub default: Option<String>,
    /// Comment.
    pub comment: Option<String>,
    /// Whether the column belongs to the primary key.
    pub primary_key: bool,
}

/// An index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexInfo {
    /// Name.
    pub name: String,
    /// Full `CREATE INDEX` statement.
    pub definition: String,
    /// Unique index.
    pub unique: bool,
    /// Index of the primary key.
    pub primary: bool,
    /// Created by a constraint (primary key, unique, exclusion).
    pub constraint_backed: bool,
}

/// Kind of constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintKind {
    /// Primary key.
    PrimaryKey,
    /// Foreign key.
    ForeignKey,
    /// Unique.
    Unique,
    /// Check.
    Check,
    /// Exclusion.
    Exclusion,
    /// Other (e.g. NOT NULL constraint objects of recent PostgreSQL).
    Other,
}

/// Table referenced by a foreign key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// Schema.
    pub schema: String,
    /// Table.
    pub table: String,
    /// Referenced columns.
    pub columns: Vec<String>,
}

/// A table constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstraintInfo {
    /// Name.
    pub name: String,
    /// Kind.
    pub kind: ConstraintKind,
    /// Constrained columns.
    pub columns: Vec<String>,
    /// Referenced table (foreign keys).
    pub references: Option<Reference>,
    /// Definition as SQL (`FOREIGN KEY (a) REFERENCES t(b)`).
    pub definition: String,
}

/// A trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerInfo {
    /// Name.
    pub name: String,
    /// Full `CREATE TRIGGER` statement.
    pub definition: String,
}

/// Everything about a table, view or materialized view (NAV-6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableDetails {
    /// The object.
    pub object: ObjectRef,
    /// Columns in order.
    pub columns: Vec<ColumnInfo>,
    /// Indexes.
    pub indexes: Vec<IndexInfo>,
    /// Constraints.
    pub constraints: Vec<ConstraintInfo>,
    /// Triggers.
    pub triggers: Vec<TriggerInfo>,
    /// Comment.
    pub comment: Option<String>,
    /// Estimated number of rows (planner statistics).
    pub estimated_rows: Option<i64>,
    /// Total size on disk in bytes (data, indexes, TOAST).
    pub total_size: Option<i64>,
}
