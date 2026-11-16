// SPDX-License-Identifier: GPL-3.0-or-later
//! Schema tree of the sidebar (NAV-1 to NAV-3).
//!
//! Rows are [`TreeNode`]s in a `GtkTreeListModel`. Children are loaded only
//! when a row is expanded (NAV-2); while loading, a "Loading…" child is
//! shown.

use std::cell::{Cell, OnceCell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gdk, gio, glib};
use strata_core::{
    ConnectionConfig, ConnectionId, ConstraintKind, Environment, ObjectKind, ObjectRef, TablePart,
};
use strata_engine::{MetaHint, MetaItem, MetaRequest, RequestId, SessionState};

use crate::i18n::tr;
use crate::messages;

/// What a node represents.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NodeKind {
    /// A saved connection.
    Connection(ConnectionId),
    /// A database.
    Database {
        conn: ConnectionId,
        database: String,
        current: bool,
    },
    /// A schema.
    Schema {
        conn: ConnectionId,
        database: String,
        schema: String,
    },
    /// "Tables", "Views"… of a schema.
    Folder {
        conn: ConnectionId,
        database: String,
        schema: String,
        kind: ObjectKind,
    },
    /// A schema object.
    Object {
        conn: ConnectionId,
        object: ObjectRef,
    },
    /// "Columns", "Indexes"… of a table.
    TableFolder {
        conn: ConnectionId,
        object: ObjectRef,
        part: TablePart,
    },
    /// Column, index, constraint or trigger.
    Leaf {
        hint: MetaHint,
        detail: Option<String>,
    },
    /// Placeholder while loading.
    Loading,
    /// Placeholder for an empty level.
    Empty,
    /// Placeholder for a failed load (the label holds the message).
    Error,
}

/// Loading state of a node's children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LoadState {
    #[default]
    NotLoaded,
    Loading,
    Loaded,
}

mod imp {
    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::TreeNode)]
    pub struct TreeNode {
        pub(crate) kind: RefCell<Option<NodeKind>>,
        pub label: RefCell<String>,
        pub tooltip: RefCell<Option<String>>,
        pub children: OnceCell<gio::ListStore>,
        pub(crate) load: Cell<LoadState>,
        /// Connection settings and state (connection nodes only).
        pub config: RefCell<Option<ConnectionConfig>>,
        pub state: RefCell<Option<SessionState>>,
        /// Bumped whenever the display changes; rows listen to it.
        #[property(get, set)]
        pub revision: Cell<u32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TreeNode {
        const NAME: &'static str = "StrataTreeNode";
        type Type = super::TreeNode;
    }

    #[glib::derived_properties]
    impl ObjectImpl for TreeNode {}
}

glib::wrapper! {
    /// A node of the schema tree.
    pub struct TreeNode(ObjectSubclass<imp::TreeNode>);
}

impl TreeNode {
    fn with(kind: NodeKind, label: impl Into<String>) -> Self {
        let node: Self = glib::Object::new();
        node.imp().kind.replace(Some(kind));
        node.imp().label.replace(label.into());
        node
    }

    /// Node of a saved connection.
    pub(crate) fn connection(cfg: &ConnectionConfig) -> Self {
        let node = Self::with(NodeKind::Connection(cfg.id), cfg.name.clone());
        node.set_config(cfg.clone());
        node
    }

    /// Placeholder shown while a level loads.
    fn loading() -> Self {
        Self::with(NodeKind::Loading, tr("Loading…"))
    }

    pub(crate) fn kind(&self) -> NodeKind {
        self.imp().kind.borrow().clone().unwrap_or(NodeKind::Empty)
    }

    pub(crate) fn label(&self) -> String {
        self.imp().label.borrow().clone()
    }

    pub(crate) fn tooltip(&self) -> Option<String> {
        self.imp().tooltip.borrow().clone()
    }

    pub(crate) fn config(&self) -> Option<ConnectionConfig> {
        self.imp().config.borrow().clone()
    }

    pub(crate) fn state(&self) -> Option<SessionState> {
        self.imp().state.borrow().clone()
    }

    pub(crate) fn load_state(&self) -> LoadState {
        self.imp().load.get()
    }

    fn touch(&self) {
        self.set_revision(self.revision().wrapping_add(1));
    }

    /// Updates the settings of a connection node.
    pub(crate) fn set_config(&self, cfg: ConnectionConfig) {
        self.imp().label.replace(cfg.name.clone());
        self.imp().tooltip.replace(Some(cfg.summary()));
        self.imp().config.replace(Some(cfg));
        self.touch();
    }

    /// Updates the session state of a connection node.
    pub(crate) fn set_state(&self, state: SessionState) {
        self.imp().state.replace(Some(state));
        self.touch();
    }

    /// Whether the node can have children.
    pub(crate) fn is_expandable(&self) -> bool {
        match self.kind() {
            NodeKind::Connection(_)
            | NodeKind::Database { .. }
            | NodeKind::Schema { .. }
            | NodeKind::Folder { .. }
            | NodeKind::TableFolder { .. } => true,
            NodeKind::Object { object, .. } => object.kind.has_columns(),
            _ => false,
        }
    }

    /// Children store (created on first use).
    pub(crate) fn children(&self) -> gio::ListStore {
        self.imp()
            .children
            .get_or_init(gio::ListStore::new::<TreeNode>)
            .clone()
    }

    /// Children already loaded (empty if not loaded).
    pub(crate) fn loaded_children(&self) -> Vec<TreeNode> {
        self.imp()
            .children
            .get()
            .map(|store| store.iter::<TreeNode>().filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    /// Request loading this node's children from the engine, if any; fixed
    /// levels (folders) are filled immediately and return `None`.
    pub(crate) fn start_loading(&self) -> Option<(ConnectionId, MetaRequest)> {
        if self.load_state() != LoadState::NotLoaded {
            return None;
        }
        let request = match self.kind() {
            NodeKind::Connection(conn) => Some((conn, MetaRequest::Databases)),
            NodeKind::Database { conn, database, .. } => {
                Some((conn, MetaRequest::Schemas { database }))
            }
            NodeKind::Folder {
                conn,
                database,
                schema,
                kind,
            } => Some((
                conn,
                MetaRequest::Objects {
                    database,
                    schema,
                    kind,
                },
            )),
            NodeKind::TableFolder { conn, object, part } => Some((
                conn,
                MetaRequest::TablePart {
                    database: object.database,
                    schema: object.schema,
                    table: object.name,
                    part,
                },
            )),
            NodeKind::Schema {
                conn,
                database,
                schema,
            } => {
                let folders: Vec<TreeNode> = ObjectKind::ALL
                    .iter()
                    .map(|kind| {
                        TreeNode::with(
                            NodeKind::Folder {
                                conn,
                                database: database.clone(),
                                schema: schema.clone(),
                                kind: *kind,
                            },
                            folder_label(*kind),
                        )
                    })
                    .collect();
                self.fill(&folders);
                None
            }
            NodeKind::Object { conn, object } => {
                let parts: &[TablePart] = match object.kind {
                    ObjectKind::Table => &[
                        TablePart::Columns,
                        TablePart::Indexes,
                        TablePart::Constraints,
                        TablePart::Triggers,
                    ],
                    ObjectKind::MaterializedView => &[TablePart::Columns, TablePart::Indexes],
                    _ => &[TablePart::Columns],
                };
                let folders: Vec<TreeNode> = parts
                    .iter()
                    .map(|part| {
                        TreeNode::with(
                            NodeKind::TableFolder {
                                conn,
                                object: object.clone(),
                                part: *part,
                            },
                            part_label(*part),
                        )
                    })
                    .collect();
                self.fill(&folders);
                None
            }
            _ => None,
        };
        if request.is_some() {
            self.imp().load.set(LoadState::Loading);
            let store = self.children();
            store.remove_all();
            store.append(&TreeNode::loading());
        }
        request
    }

    fn fill(&self, nodes: &[TreeNode]) {
        let store = self.children();
        let empty = [TreeNode::with(NodeKind::Empty, tr("No items"))];
        let nodes = if nodes.is_empty() { &empty[..] } else { nodes };
        store.splice(0, store.n_items(), nodes);
        self.imp().load.set(LoadState::Loaded);
    }

    /// Fills the children with the engine's answer.
    pub(crate) fn set_children(&self, items: Vec<MetaItem>) {
        let nodes: Vec<TreeNode> = items
            .into_iter()
            .filter_map(|item| self.child_from(item))
            .collect();
        self.fill(&nodes);
    }

    /// Shows a load failure as a child row.
    pub(crate) fn set_error(&self, summary: &str, detail: &str) {
        let node = TreeNode::with(NodeKind::Error, summary);
        node.imp().tooltip.replace(Some(detail.to_owned()));
        let store = self.children();
        store.splice(0, store.n_items(), &[node]);
        // Reloadable with "Refresh" or by collapsing/expanding.
        self.imp().load.set(LoadState::NotLoaded);
    }

    /// Forgets the children so that they are reloaded on next expansion.
    pub(crate) fn reset(&self) {
        self.imp().load.set(LoadState::NotLoaded);
        if let Some(store) = self.imp().children.get() {
            store.remove_all();
        }
    }

    fn child_from(&self, item: MetaItem) -> Option<TreeNode> {
        let node = match (self.kind(), item.hint) {
            (NodeKind::Connection(conn), MetaHint::Database { current }) => TreeNode::with(
                NodeKind::Database {
                    conn,
                    database: item.name.clone(),
                    current,
                },
                item.name,
            ),
            (NodeKind::Database { conn, database, .. }, MetaHint::Schema) => TreeNode::with(
                NodeKind::Schema {
                    conn,
                    database,
                    schema: item.name.clone(),
                },
                item.name,
            ),
            (
                NodeKind::Folder {
                    conn,
                    database,
                    schema,
                    ..
                },
                MetaHint::Object(kind),
            ) => {
                let label = match &item.signature {
                    Some(sig) => format!("{}({sig})", item.name),
                    None => item.name.clone(),
                };
                TreeNode::with(
                    NodeKind::Object {
                        conn,
                        object: ObjectRef {
                            database,
                            schema,
                            name: item.name,
                            kind,
                            signature: item.signature,
                        },
                    },
                    label,
                )
            }
            (NodeKind::TableFolder { .. }, hint) => TreeNode::with(
                NodeKind::Leaf {
                    hint,
                    detail: item.detail,
                },
                item.name,
            ),
            _ => return None,
        };
        node.imp().tooltip.replace(item.comment);
        Some(node)
    }

    /// Whether this node or a loaded descendant matches `query` (lower case).
    pub(crate) fn matches_deep(&self, query: &str) -> bool {
        self.label().to_lowercase().contains(query)
            || self
                .loaded_children()
                .iter()
                .any(|child| child.matches_deep(query))
    }

    /// Connection this node belongs to.
    pub(crate) fn connection_id(&self) -> Option<ConnectionId> {
        match self.kind() {
            NodeKind::Connection(conn)
            | NodeKind::Database { conn, .. }
            | NodeKind::Schema { conn, .. }
            | NodeKind::Folder { conn, .. }
            | NodeKind::Object { conn, .. }
            | NodeKind::TableFolder { conn, .. } => Some(conn),
            _ => None,
        }
    }
}

fn folder_label(kind: ObjectKind) -> String {
    match kind {
        ObjectKind::Table => tr("Tables"),
        ObjectKind::View => tr("Views"),
        ObjectKind::MaterializedView => tr("Materialized Views"),
        ObjectKind::Sequence => tr("Sequences"),
        ObjectKind::Function => tr("Functions"),
        ObjectKind::Procedure => tr("Procedures"),
    }
}

fn part_label(part: TablePart) -> String {
    match part {
        TablePart::Columns => tr("Columns"),
        TablePart::Indexes => tr("Indexes"),
        TablePart::Constraints => tr("Constraints"),
        TablePart::Triggers => tr("Triggers"),
    }
}

/// Icon of an object kind.
pub(crate) fn object_icon(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Table => "view-grid-symbolic",
        ObjectKind::View => "format-justify-left-symbolic",
        ObjectKind::MaterializedView => "drive-harddisk-symbolic",
        ObjectKind::Sequence => "media-playlist-repeat-symbolic",
        ObjectKind::Function => "system-run-symbolic",
        ObjectKind::Procedure => "emblem-system-symbolic",
    }
}

fn icon(node: &TreeNode) -> Option<&'static str> {
    Some(match node.kind() {
        NodeKind::Connection(_) => "network-server-symbolic",
        NodeKind::Database { .. } => "drive-multidisk-symbolic",
        NodeKind::Schema { .. } | NodeKind::Folder { .. } | NodeKind::TableFolder { .. } => {
            "folder-symbolic"
        }
        NodeKind::Object { object, .. } => object_icon(object.kind),
        NodeKind::Leaf { hint, .. } => match hint {
            MetaHint::Column { .. } => "object-select-symbolic",
            MetaHint::Index { .. } => "view-sort-ascending-symbolic",
            MetaHint::Constraint(ConstraintKind::ForeignKey) => "insert-link-symbolic",
            MetaHint::Constraint(_) => "preferences-other-symbolic",
            MetaHint::Trigger => "applications-science-symbolic",
            _ => "folder-symbolic",
        },
        NodeKind::Error => "dialog-warning-symbolic",
        NodeKind::Loading | NodeKind::Empty => return None,
    })
}

// ---------------------------------------------------------------------------
// Row widget
// ---------------------------------------------------------------------------

mod row_imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct TreeRow {
        pub expander: gtk::TreeExpander,
        pub icon: gtk::Image,
        pub color: gtk::DrawingArea,
        pub label: gtk::Label,
        pub detail: gtk::Label,
        pub badge: gtk::Label,
        pub spinner: gtk::Spinner,
        pub status: gtk::Image,
        pub handlers: RefCell<Vec<(glib::Object, glib::SignalHandlerId)>>,
        pub color_value: RefCell<Option<gdk::RGBA>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TreeRow {
        const NAME: &'static str = "StrataTreeRow";
        type Type = super::TreeRow;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for TreeRow {}
    impl WidgetImpl for TreeRow {}
    impl BoxImpl for TreeRow {}
}

glib::wrapper! {
    /// Widget displaying a [`TreeNode`].
    pub struct TreeRow(ObjectSubclass<row_imp::TreeRow>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl Default for TreeRow {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeRow {
    /// Creates an empty row.
    pub fn new() -> Self {
        let row: Self = glib::Object::new();
        let imp = row.imp();
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);

        imp.color.set_content_width(10);
        imp.color.set_content_height(10);
        imp.color.set_valign(gtk::Align::Center);
        imp.color.set_draw_func(glib::clone!(
            #[weak]
            row,
            move |_, cr, width, height| {
                let Some(rgba) = *row.imp().color_value.borrow() else {
                    return;
                };
                let radius = f64::from(width.min(height)) / 2.0;
                cr.set_source_rgba(
                    f64::from(rgba.red()),
                    f64::from(rgba.green()),
                    f64::from(rgba.blue()),
                    1.0,
                );
                cr.arc(
                    f64::from(width) / 2.0,
                    f64::from(height) / 2.0,
                    radius,
                    0.0,
                    std::f64::consts::TAU,
                );
                let _ = cr.fill();
            }
        ));

        imp.label.set_xalign(0.0);
        imp.label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        imp.detail.set_xalign(0.0);
        imp.detail.set_hexpand(true);
        imp.detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
        imp.detail.add_css_class("dim-label");
        imp.detail.add_css_class("caption");
        imp.badge.set_valign(gtk::Align::Center);
        imp.badge.add_css_class("env-badge");

        content.append(&imp.color);
        content.append(&imp.icon);
        content.append(&imp.label);
        content.append(&imp.detail);
        content.append(&imp.badge);
        content.append(&imp.spinner);
        content.append(&imp.status);
        imp.expander.set_child(Some(&content));
        imp.expander.set_hexpand(true);
        row.append(&imp.expander);
        row
    }

    /// Binds the row to `list_row`; `on_expand` runs when it gets expanded.
    pub(crate) fn bind(
        &self,
        list_row: &gtk::TreeListRow,
        on_expand: impl Fn(&TreeNode) + 'static,
    ) {
        let imp = self.imp();
        imp.expander.set_list_row(Some(list_row));
        let Some(node) = list_row.item().and_downcast::<TreeNode>() else {
            return;
        };
        self.update(&node);

        let id = node.connect_revision_notify(glib::clone!(
            #[weak(rename_to = row)]
            self,
            move |node| row.update(node)
        ));
        let expand_id = list_row.connect_expanded_notify(move |list_row| {
            if list_row.is_expanded()
                && let Some(node) = list_row.item().and_downcast::<TreeNode>()
            {
                on_expand(&node);
            }
        });
        imp.handlers
            .borrow_mut()
            .extend([(node.upcast(), id), (list_row.clone().upcast(), expand_id)]);
    }

    /// Releases the row before reuse.
    pub(crate) fn unbind(&self) {
        let imp = self.imp();
        for (object, id) in imp.handlers.borrow_mut().drain(..) {
            object.disconnect(id);
        }
        imp.expander.set_list_row(None);
    }

    fn update(&self, node: &TreeNode) {
        let imp = self.imp();
        imp.label.set_text(&node.label());
        // The sidebar is narrow: the tooltip repeats what may be ellipsized.
        let tooltip = match (node.kind(), node.tooltip()) {
            (
                NodeKind::Leaf {
                    detail: Some(detail),
                    ..
                },
                Some(comment),
            ) if detail != comment => Some(format!("{} — {detail}\n{comment}", node.label())),
            (
                NodeKind::Leaf {
                    detail: Some(detail),
                    ..
                },
                _,
            ) => Some(format!("{} — {detail}", node.label())),
            (_, tooltip) => tooltip,
        };
        self.set_tooltip_text(tooltip.as_deref());
        match icon(node) {
            Some(name) => {
                imp.icon.set_icon_name(Some(name));
                imp.icon.set_visible(true);
            }
            None => imp.icon.set_visible(false),
        }
        imp.icon.remove_css_class("warning");
        imp.label.remove_css_class("dim-label");
        imp.label.remove_css_class("heading");
        imp.spinner.set_spinning(false);
        imp.spinner.set_visible(false);
        imp.status.set_visible(false);
        imp.badge.set_visible(false);
        imp.detail.set_text("");
        imp.color_value.replace(None);
        imp.color.set_visible(false);

        match node.kind() {
            NodeKind::Loading => {
                imp.spinner.set_visible(true);
                imp.spinner.set_spinning(true);
                imp.label.add_css_class("dim-label");
            }
            NodeKind::Empty => imp.label.add_css_class("dim-label"),
            NodeKind::Error => imp.icon.add_css_class("warning"),
            NodeKind::Database { current: true, .. } => {
                imp.detail.set_text(&tr("current"));
            }
            NodeKind::Leaf { hint, detail } => {
                let mut text = detail.unwrap_or_default();
                if hint == (MetaHint::Column { primary_key: true }) {
                    text = format!("{text} · {}", tr("primary key"));
                }
                if hint == (MetaHint::Index { unique: true }) {
                    text = format!("{} · {text}", tr("unique"));
                }
                imp.detail.set_text(&text);
            }
            NodeKind::Connection(_) => self.update_connection(node),
            _ => {}
        }
    }

    fn update_connection(&self, node: &TreeNode) {
        let imp = self.imp();
        let Some(cfg) = node.config() else {
            return;
        };
        imp.label.add_css_class("heading");
        if let Some(rgba) = cfg.color.as_deref().and_then(|c| gdk::RGBA::parse(c).ok()) {
            imp.color_value.replace(Some(rgba));
            imp.color.set_visible(true);
            imp.color.queue_draw();
        }
        for class in ["env-dev", "env-staging", "env-prod"] {
            imp.badge.remove_css_class(class);
        }
        if let Some(env) = cfg.environment {
            imp.badge.set_text(&messages::environment_badge(env));
            imp.badge
                .set_tooltip_text(Some(&messages::environment(Some(env))));
            imp.badge.add_css_class(match env {
                Environment::Development => "env-dev",
                Environment::Staging => "env-staging",
                Environment::Production => "env-prod",
            });
            imp.badge.set_visible(true);
        }
        for class in ["success", "error", "warning"] {
            imp.status.remove_css_class(class);
        }
        let status = match node.state() {
            Some(SessionState::Connecting | SessionState::Reconnecting) => {
                imp.spinner.set_visible(true);
                imp.spinner.set_spinning(true);
                imp.spinner.set_tooltip_text(Some(&tr("Connecting…")));
                None
            }
            Some(SessionState::Connected(_)) => {
                Some(("emblem-ok-symbolic", "success", tr("Connected")))
            }
            Some(SessionState::Failed { error, .. }) => Some((
                "dialog-warning-symbolic",
                "error",
                messages::error_summary(error.kind),
            )),
            _ => None,
        };
        if let Some((icon, class, text)) = status {
            imp.status.set_icon_name(Some(icon));
            imp.status.add_css_class(class);
            imp.status.set_tooltip_text(Some(&text));
            imp.status
                .update_property(&[gtk::accessible::Property::Label(&text)]);
            imp.status.set_visible(true);
        }
    }
}

/// Correlation identifiers of metadata requests.
pub(crate) fn next_request() -> RequestId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1 << 32);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
