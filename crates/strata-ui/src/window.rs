// SPDX-License-Identifier: GPL-3.0-or-later
//! Main application window (single window, see ADR-0015).

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gdk, gio, glib};
use strata_core::{
    ConnectionConfig, ConnectionId, Endpoint, ErrorKind, ErrorReport, ObjectRef, PasswordPolicy,
    Secret, SshAuth,
};
use strata_engine::{
    Command, EngineHandle, MetaItem, MetaRequest, PasswordChange, RequestId, SessionState, UiEvent,
};

use crate::connection_dialog::ConnectionDialog;
use crate::i18n::{tr, trf};
use crate::object_tab::ObjectTab;
use crate::tree::{LoadState, NodeKind, TreeNode, TreeRow, next_request};
use crate::{dialogs, messages, settings};

/// Secrets typed while connecting: database password, SSH secret.
type TypedSecrets = (Option<Secret>, Option<Secret>);

/// What a pending engine request is for.
#[derive(Debug, Clone)]
enum Pending {
    /// Children of a tree node.
    Children(TreeNode, MetaRequest),
    /// Properties of a tab.
    TabDetails(ConnectionId, ObjectRef),
    /// DDL of a tab.
    TabDdl(ConnectionId, ObjectRef),
    /// DDL to copy to the clipboard.
    CopyDdl,
}

mod imp {
    use super::*;

    /// Private state of [`super::StrataWindow`].
    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/fr/coregeek/Strata/ui/window.ui")]
    pub struct StrataWindow {
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub split_view: TemplateChild<adw::OverlaySplitView>,
        #[template_child]
        pub search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub tree_view: TemplateChild<gtk::ListView>,
        #[template_child]
        pub banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub tab_view: TemplateChild<adw::TabView>,
        #[template_child]
        pub content_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub welcome_page: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub connection_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub connection_subtitle: TemplateChild<gtk::Label>,
        #[template_child]
        pub connect_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub disconnect_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub edit_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub status_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub server_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub encryption_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub environment_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub access_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub ssh_row: TemplateChild<adw::ActionRow>,

        pub settings: OnceCell<gio::Settings>,
        pub engine: OnceCell<EngineHandle>,
        /// Root nodes of the tree, one per saved connection, sorted by name.
        pub roots: OnceCell<gio::ListStore>,
        pub tree: OnceCell<gtk::TreeListModel>,
        pub filter: OnceCell<gtk::CustomFilter>,
        pub selection: OnceCell<gtk::SingleSelection>,
        pub query: Rc<RefCell<String>>,
        pub selected: Cell<Option<ConnectionId>>,
        pub keyring_available: Cell<Option<bool>>,
        pub(super) dialog: RefCell<Option<ConnectionDialog>>,
        pub(super) pending: RefCell<HashMap<RequestId, Pending>>,
        pub(super) tabs: RefCell<Vec<(ObjectTab, adw::TabPage)>>,
        /// Node targeted by the context menu.
        pub(super) context_node: RefCell<Option<TreeNode>>,
        /// Connections whose row expands once connected.
        pub(super) expand_on_connect: RefCell<HashSet<ConnectionId>>,
        /// Secrets typed while connecting (database, SSH), kept until the
        /// connection succeeds or is abandoned.
        pub(super) typed: RefCell<HashMap<ConnectionId, TypedSecrets>>,
        /// Connection waiting for an SSH host key to be trusted.
        pub(super) pending_trust: RefCell<Option<(ConnectionId, String, u16)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for StrataWindow {
        const NAME: &'static str = "StrataWindow";
        type Type = super::StrataWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for StrataWindow {
        fn constructed(&self) {
            self.parent_constructed();
            if let Some(settings) = settings::open() {
                self.restore_state(&settings);
                let _ = self.settings.set(settings);
            }
        }
    }

    impl WidgetImpl for StrataWindow {}

    impl WindowImpl for StrataWindow {
        fn close_request(&self) -> glib::Propagation {
            if let Some(settings) = self.settings.get() {
                self.save_state(settings);
            }
            self.parent_close_request()
        }
    }

    impl ApplicationWindowImpl for StrataWindow {}
    impl AdwApplicationWindowImpl for StrataWindow {}

    impl StrataWindow {
        fn restore_state(&self, settings: &gio::Settings) {
            let obj = self.obj();
            obj.set_default_size(
                settings.int(settings::WINDOW_WIDTH),
                settings.int(settings::WINDOW_HEIGHT),
            );
            if settings.boolean(settings::WINDOW_MAXIMIZED) {
                obj.maximize();
            }
        }

        fn save_state(&self, settings: &gio::Settings) {
            let obj = self.obj();
            let (width, height) = obj.default_size();
            let result = settings
                .set_int(settings::WINDOW_WIDTH, width)
                .and_then(|()| settings.set_int(settings::WINDOW_HEIGHT, height))
                .and_then(|()| {
                    settings.set_boolean(settings::WINDOW_MAXIMIZED, obj.is_maximized())
                });
            if let Err(err) = result {
                tracing::warn!(%err, "failed to save window state");
            }
        }
    }
}

glib::wrapper! {
    /// The main Strata window.
    pub struct StrataWindow(ObjectSubclass<imp::StrataWindow>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
            gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl StrataWindow {
    /// Creates the main window for `app`, driven by `engine`.
    pub fn new(app: &adw::Application, engine: EngineHandle) -> Self {
        let window: Self = glib::Object::builder().property("application", app).build();
        let _ = window.imp().engine.set(engine);
        window.setup();
        if let Some(settings) = window.imp().settings.get() {
            let minutes = settings.int(settings::IDLE_TIMEOUT_MINUTES);
            let timeout = u64::try_from(minutes)
                .ok()
                .filter(|m| *m > 0)
                .map(|m| std::time::Duration::from_secs(m * 60));
            window.send(Command::SetIdleTimeout(timeout));
        }
        window.send(Command::LoadConnections);
        window
    }

    /// Shows a transient notification.
    pub fn toast(&self, message: &str) {
        self.imp().toast_overlay.add_toast(adw::Toast::new(message));
    }

    fn send(&self, command: Command) -> bool {
        let sent = self.imp().engine.get().is_some_and(|e| e.send(command));
        if !sent {
            self.toast(&tr("Strata is busy, try again."));
        }
        sent
    }

    fn roots(&self) -> &gio::ListStore {
        self.imp()
            .roots
            .get_or_init(gio::ListStore::new::<TreeNode>)
    }

    fn root_nodes(&self) -> Vec<TreeNode> {
        self.roots()
            .iter::<TreeNode>()
            .filter_map(Result::ok)
            .collect()
    }

    fn root_node(&self, id: ConnectionId) -> Option<(u32, TreeNode)> {
        self.root_nodes()
            .into_iter()
            .enumerate()
            .find(|(_, n)| n.kind() == NodeKind::Connection(id))
            .and_then(|(i, n)| u32::try_from(i).ok().map(|i| (i, n)))
    }

    fn root_row(&self, id: ConnectionId) -> Option<gtk::TreeListRow> {
        let (index, _) = self.root_node(id)?;
        self.imp().tree.get()?.child_row(index)
    }

    fn connection(&self, id: ConnectionId) -> Option<ConnectionConfig> {
        self.root_node(id).and_then(|(_, n)| n.config())
    }

    fn state(&self, id: ConnectionId) -> Option<SessionState> {
        self.root_node(id).and_then(|(_, n)| n.state())
    }

    fn is_connected(&self, id: ConnectionId) -> bool {
        matches!(self.state(id), Some(SessionState::Connected(_)))
    }

    // ----------------------------------------------------------------
    // Setup
    // ----------------------------------------------------------------

    fn setup(&self) {
        self.setup_tree();
        self.setup_content();
        self.install_actions();
        self.refresh_content();
    }

    fn setup_tree(&self) {
        let imp = self.imp();
        let tree = gtk::TreeListModel::new(self.roots().clone(), false, false, |item| {
            let node = item.downcast_ref::<TreeNode>()?;
            node.is_expandable()
                .then(|| node.children().upcast::<gio::ListModel>())
        });

        // Filter on loaded nodes (NAV-3): a row stays visible if it, one of
        // its loaded descendants, or one of its ancestors matches.
        let query = Rc::clone(&imp.query);
        let filter = gtk::CustomFilter::new(move |item| {
            let query = query.borrow();
            if query.is_empty() {
                return true;
            }
            let Some(row) = item.downcast_ref::<gtk::TreeListRow>() else {
                return true;
            };
            if row
                .item()
                .and_downcast::<TreeNode>()
                .is_some_and(|n| n.matches_deep(&query))
            {
                return true;
            }
            let mut parent = row.parent();
            while let Some(p) = parent {
                if p.item()
                    .and_downcast::<TreeNode>()
                    .is_some_and(|n| n.label().to_lowercase().contains(query.as_str()))
                {
                    return true;
                }
                parent = p.parent();
            }
            false
        });
        let filtered = gtk::FilterListModel::new(Some(tree.clone()), Some(filter.clone()));
        let selection = gtk::SingleSelection::builder()
            .model(&filtered)
            .autoselect(false)
            .can_unselect(true)
            .build();

        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, item| {
                let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let row = TreeRow::new();
                let click = gtk::GestureClick::builder()
                    .button(gdk::BUTTON_SECONDARY)
                    .build();
                let list_item = item.downgrade();
                click.connect_pressed(glib::clone!(
                    #[weak]
                    window,
                    #[weak]
                    row,
                    move |_, _, x, y| {
                        let node = list_item
                            .upgrade()
                            .and_then(|li| li.item())
                            .and_downcast::<gtk::TreeListRow>()
                            .and_then(|r| r.item())
                            .and_downcast::<TreeNode>();
                        if let Some(node) = node {
                            window.show_context_menu(&node, row.upcast_ref(), Some((x, y)));
                        }
                    }
                ));
                row.add_controller(click);
                item.set_child(Some(&row));
            }
        ));
        factory.connect_bind(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, item| {
                let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let (Some(list_row), Some(widget)) = (
                    item.item().and_downcast::<gtk::TreeListRow>(),
                    item.child().and_downcast::<TreeRow>(),
                ) else {
                    return;
                };
                // Screen readers announce the row by its label (CDC §8).
                if let Some(node) = list_row.item().and_downcast::<TreeNode>() {
                    item.set_accessible_label(&node.label());
                }
                widget.bind(
                    &list_row,
                    glib::clone!(
                        #[weak]
                        window,
                        move |node| window.load_children(node)
                    ),
                );
            }
        ));
        factory.connect_unbind(|_, item| {
            if let Some(widget) = item
                .downcast_ref::<gtk::ListItem>()
                .and_then(|i| i.child())
                .and_downcast::<TreeRow>()
            {
                widget.unbind();
            }
        });

        imp.tree_view.set_model(Some(&selection));
        imp.tree_view.set_factory(Some(&factory));
        imp.tree_view.connect_activate(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, position| window.on_activate(position)
        ));
        selection.connect_selection_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _, _| window.on_selection_changed()
        ));

        // Keyboard access to the context menu.
        let shortcuts = gtk::ShortcutController::new();
        let open_menu = gtk::CallbackAction::new(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, _| {
                if let Some(node) = window.selected_node() {
                    window.show_context_menu(&node, window.imp().tree_view.upcast_ref(), None);
                }
                glib::Propagation::Stop
            }
        ));
        for trigger in ["Menu", "<Shift>F10"] {
            shortcuts.add_shortcut(gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string(trigger),
                Some(open_menu.clone()),
            ));
        }
        imp.tree_view.add_controller(shortcuts);

        imp.search_entry.connect_search_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |entry| {
                let imp = window.imp();
                imp.query.replace(entry.text().to_lowercase());
                if let Some(filter) = imp.filter.get() {
                    filter.changed(gtk::FilterChange::Different);
                }
            }
        ));

        let _ = imp.tree.set(tree);
        let _ = imp.filter.set(filter);
        let _ = imp.selection.set(selection);
    }

    fn setup_content(&self) {
        let imp = self.imp();
        let with_selected = |f: fn(&Self, ConnectionId)| {
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_: &gtk::Button| {
                    if let Some(id) = window.imp().selected.get() {
                        f(&window, id);
                    }
                }
            )
        };
        imp.connect_button
            .connect_clicked(with_selected(Self::connect));
        imp.disconnect_button
            .connect_clicked(with_selected(|w, id| {
                w.send(Command::Disconnect(id));
            }));
        imp.edit_button
            .connect_clicked(with_selected(|w, id| w.open_connection_dialog(Some(id))));

        imp.tab_view.connect_close_page(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, page| {
                window.imp().tabs.borrow_mut().retain(|(_, p)| p != page);
                glib::Propagation::Proceed
            }
        ));
        imp.tab_view.connect_n_pages_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.refresh_content()
        ));
        imp.tab_view.connect_selected_page_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.refresh_content()
        ));
    }

    fn install_actions(&self) {
        let with_id = |name: &str, f: fn(&Self, ConnectionId)| {
            gio::ActionEntry::builder(name)
                .parameter_type(Some(glib::VariantTy::STRING))
                .activate(move |window: &Self, _, param| {
                    let id = param
                        .and_then(|p| p.get::<String>())
                        .and_then(|s| s.parse::<ConnectionId>().ok());
                    if let Some(id) = id {
                        f(window, id);
                    }
                })
                .build()
        };
        let on_node = |name: &str, f: fn(&Self, &TreeNode)| {
            gio::ActionEntry::builder(name)
                .activate(move |window: &Self, _, _| {
                    let node = window.imp().context_node.borrow().clone();
                    if let Some(node) = node {
                        f(window, &node);
                    }
                })
                .build()
        };
        self.add_action_entries([
            with_id("connect", Self::connect),
            with_id("disconnect", |w, id| {
                w.send(Command::Disconnect(id));
            }),
            with_id("edit-connection", |w, id| {
                w.open_connection_dialog(Some(id))
            }),
            with_id("duplicate-connection", Self::duplicate),
            with_id("delete-connection", Self::delete),
            on_node("node-open", |w, node| {
                if let NodeKind::Object { conn, object } = node.kind() {
                    w.open_object_tab(conn, object);
                }
            }),
            on_node("node-refresh", Self::refresh_node),
            on_node("node-copy-name", Self::copy_name),
            on_node("node-copy-ddl", |w, node| {
                if let NodeKind::Object { conn, object } = node.kind() {
                    w.request(
                        |request| Command::LoadDdl {
                            request,
                            connection: conn,
                            object,
                        },
                        Pending::CopyDdl,
                    );
                }
            }),
        ]);
    }

    /// Sends the command built with a fresh request id and remembers its
    /// purpose.
    fn request(&self, build: impl FnOnce(RequestId) -> Command, purpose: Pending) {
        let id = next_request();
        if self.send(build(id)) {
            self.imp().pending.borrow_mut().insert(id, purpose);
        }
    }

    // ----------------------------------------------------------------
    // Tree
    // ----------------------------------------------------------------

    /// Loads the children of an expanded node (NAV-2).
    fn load_children(&self, node: &TreeNode) {
        if node.load_state() != LoadState::NotLoaded {
            return;
        }
        if let NodeKind::Connection(id) = node.kind()
            && !self.is_connected(id)
        {
            // Expanding a disconnected connection connects it; the
            // databases are loaded once connected.
            let _ = node.start_loading();
            self.connect(id);
            return;
        }
        if let Some((connection, what)) = node.start_loading() {
            let purpose = Pending::Children(node.clone(), what.clone());
            self.request(
                |request| Command::LoadMetadata {
                    request,
                    connection,
                    what,
                },
                purpose,
            );
        }
    }

    fn refresh_node(&self, node: &TreeNode) {
        if let NodeKind::Connection(id) = node.kind()
            && !self.is_connected(id)
        {
            return;
        }
        node.reset();
        self.load_children(node);
    }

    fn selected_node(&self) -> Option<TreeNode> {
        self.imp()
            .selection
            .get()?
            .selected_item()
            .and_downcast::<gtk::TreeListRow>()?
            .item()
            .and_downcast::<TreeNode>()
    }

    fn on_selection_changed(&self) {
        let id = self.selected_node().and_then(|n| n.connection_id());
        self.imp().selected.set(id);
        self.refresh_content();
    }

    fn on_activate(&self, position: u32) {
        let Some(row) = self
            .imp()
            .selection
            .get()
            .and_then(|s| s.item(position))
            .and_downcast::<gtk::TreeListRow>()
        else {
            return;
        };
        let Some(node) = row.item().and_downcast::<TreeNode>() else {
            return;
        };
        match node.kind() {
            NodeKind::Object { conn, object } => self.open_object_tab(conn, object),
            NodeKind::Connection(id) if !self.is_connected(id) => self.connect(id),
            _ if row.is_expandable() => row.set_expanded(!row.is_expanded()),
            _ => {}
        }
    }

    fn show_context_menu(&self, node: &TreeNode, anchor: &gtk::Widget, at: Option<(f64, f64)>) {
        let section = |items: &[(String, &str, Option<glib::Variant>)]| {
            let section = gio::Menu::new();
            for (label, action, target) in items {
                let item = gio::MenuItem::new(Some(label), None);
                item.set_action_and_target_value(Some(action), target.as_ref());
                section.append_item(&item);
            }
            section
        };
        let menu = gio::Menu::new();
        match node.kind() {
            NodeKind::Connection(id) => {
                let target = Some(id.to_string().to_variant());
                let session = if self.is_connected(id) {
                    vec![
                        (tr("_Disconnect"), "win.disconnect", target.clone()),
                        (tr("_Refresh"), "win.node-refresh", None),
                    ]
                } else {
                    vec![(tr("_Connect"), "win.connect", target.clone())]
                };
                menu.append_section(None, &section(&session));
                menu.append_section(
                    None,
                    &section(&[
                        (tr("_Edit…"), "win.edit-connection", target.clone()),
                        (tr("D_uplicate"), "win.duplicate-connection", target.clone()),
                    ]),
                );
                menu.append_section(
                    None,
                    &section(&[(tr("_Delete…"), "win.delete-connection", target)]),
                );
            }
            NodeKind::Database { .. } | NodeKind::Schema { .. } => {
                menu.append_section(
                    None,
                    &section(&[
                        (tr("Copy _Name"), "win.node-copy-name", None),
                        (tr("_Refresh"), "win.node-refresh", None),
                    ]),
                );
            }
            NodeKind::Folder { .. } | NodeKind::TableFolder { .. } => {
                menu.append_section(
                    None,
                    &section(&[(tr("_Refresh"), "win.node-refresh", None)]),
                );
            }
            NodeKind::Object { .. } => {
                menu.append_section(
                    None,
                    &section(&[(tr("_Open Properties"), "win.node-open", None)]),
                );
                menu.append_section(
                    None,
                    &section(&[
                        (tr("Copy _Name"), "win.node-copy-name", None),
                        (tr("Copy _DDL"), "win.node-copy-ddl", None),
                    ]),
                );
            }
            NodeKind::Leaf { .. } => {
                menu.append_section(
                    None,
                    &section(&[(tr("Copy _Name"), "win.node-copy-name", None)]),
                );
            }
            _ => return,
        }
        self.imp().context_node.replace(Some(node.clone()));

        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_parent(anchor);
        popover.set_has_arrow(false);
        if let Some((x, y)) = at {
            // Truncation to whole pixels is intended.
            #[allow(clippy::cast_possible_truncation)]
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        }
        popover.connect_closed(|popover| {
            let popover = popover.clone();
            glib::idle_add_local_once(move || popover.unparent());
        });
        popover.popup();
    }

    fn copy_name(&self, node: &TreeNode) {
        let Some(cfg) = node.connection_id().and_then(|id| self.connection(id)) else {
            return;
        };
        let dialect = strata_sql::dialect_for(cfg.kind);
        let text = match node.kind() {
            NodeKind::Database { database, .. } => dialect.quote_identifier(&database),
            NodeKind::Schema { schema, .. } => dialect.quote_identifier(&schema),
            NodeKind::Object { object, .. } => dialect.qualified_name(&object.schema, &object.name),
            NodeKind::Leaf { .. } => dialect.quote_identifier(&node.label()),
            _ => return,
        };
        self.clipboard().set_text(&text);
        self.toast(&trf("Copied “{text}”", &[("text", &text)]));
    }

    // ----------------------------------------------------------------
    // Tabs
    // ----------------------------------------------------------------

    fn open_object_tab(&self, conn: ConnectionId, object: ObjectRef) {
        let imp = self.imp();
        let existing = imp
            .tabs
            .borrow()
            .iter()
            .find(|(tab, _)| tab.connection == conn && tab.object == object)
            .map(|(_, page)| page.clone());
        if let Some(page) = existing {
            imp.tab_view.set_selected_page(&page);
            return;
        }
        let tab = ObjectTab::new(conn, object.clone());
        let page = imp.tab_view.append(tab.widget());
        page.set_title(&tab.title());
        page.set_tooltip(&glib::markup_escape_text(&format!(
            "{}.{}.{}",
            object.database,
            object.schema,
            tab.title()
        )));
        page.set_icon(Some(&tab.icon()));
        tab.copy_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[strong]
            tab,
            move |_| {
                if let Some(ddl) = tab.ddl() {
                    window.clipboard().set_text(&ddl);
                    window.toast(&tr("DDL copied"));
                }
            }
        ));
        tab.refresh_button.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[strong]
            tab,
            move |_| window.load_tab(&tab)
        ));
        imp.tabs.borrow_mut().push((tab.clone(), page.clone()));
        imp.tab_view.set_selected_page(&page);
        self.load_tab(&tab);
        if imp.split_view.is_collapsed() {
            imp.split_view.set_show_sidebar(false);
        }
    }

    fn load_tab(&self, tab: &ObjectTab) {
        tab.set_loading();
        let conn = tab.connection;
        let object = tab.object.clone();
        if object.kind.has_columns() {
            let target = object.clone();
            self.request(
                |request| Command::DescribeTable {
                    request,
                    connection: conn,
                    object: target,
                },
                Pending::TabDetails(conn, object.clone()),
            );
        } else {
            tab.set_general();
        }
        let target = object.clone();
        self.request(
            |request| Command::LoadDdl {
                request,
                connection: conn,
                object: target,
            },
            Pending::TabDdl(conn, object),
        );
    }

    fn tab(&self, conn: ConnectionId, object: &ObjectRef) -> Option<ObjectTab> {
        self.imp()
            .tabs
            .borrow()
            .iter()
            .find(|(t, _)| t.connection == conn && &t.object == object)
            .map(|(t, _)| t.clone())
    }

    /// Closes the tabs of a connection (on deletion).
    fn close_tabs_of(&self, conn: ConnectionId) {
        let pages: Vec<adw::TabPage> = self
            .imp()
            .tabs
            .borrow()
            .iter()
            .filter(|(t, _)| t.connection == conn)
            .map(|(_, p)| p.clone())
            .collect();
        for page in pages {
            self.imp().tab_view.close_page(&page);
        }
    }

    // ----------------------------------------------------------------
    // User actions
    // ----------------------------------------------------------------

    /// Opens the connection dialog, for a new connection if `id` is `None`.
    pub fn open_connection_dialog(&self, id: Option<ConnectionId>) {
        let imp = self.imp();
        if let Some(dialog) = imp.dialog.borrow().as_ref() {
            dialog.present();
            return;
        }
        let Some(engine) = imp.engine.get().cloned() else {
            return;
        };
        let existing = id.and_then(|id| self.connection(id));
        let dialog =
            ConnectionDialog::new(self, engine, existing.as_ref(), imp.keyring_available.get());
        dialog.connect_closed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move || {
                window.imp().dialog.replace(None);
            }
        ));
        imp.dialog.replace(Some(dialog.clone()));
        if imp.keyring_available.get().is_none() {
            // Checked lazily: contacting the keyring at start-up costs memory
            // and time for nothing.
            self.send(Command::CheckKeyring);
        }
        dialog.present();
    }

    /// Looks for connections in the client configuration files.
    pub fn import_connections(&self) {
        self.send(Command::ScanClientConfigs);
    }

    fn connect(&self, id: ConnectionId) {
        let Some(cfg) = self.connection(id) else {
            return;
        };
        self.imp().expand_on_connect.borrow_mut().insert(id);
        self.imp().typed.borrow_mut().remove(&id);
        if cfg.password_policy == PasswordPolicy::AskEveryTime {
            self.ask_password(&cfg, None);
        } else {
            self.send_connect(id);
        }
    }

    /// Sends `Connect` with the secrets typed so far.
    fn send_connect(&self, id: ConnectionId) {
        let (password, ssh_secret) = self
            .imp()
            .typed
            .borrow()
            .get(&id)
            .cloned()
            .unwrap_or_default();
        self.send(Command::Connect {
            id,
            password,
            ssh_secret,
        });
    }

    fn ask_ssh_secret(&self, cfg: &ConnectionConfig, error: Option<&str>) {
        let can_remember = self.imp().keyring_available.get() != Some(false);
        let id = cfg.id;
        let config = cfg.clone();
        dialogs::ask_ssh_secret(
            self,
            cfg,
            error,
            can_remember,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |answer| {
                    if answer.remember {
                        window.send(Command::SaveConnection {
                            config: config.clone(),
                            password: PasswordChange::Keep,
                            ssh_password: PasswordChange::Set(answer.password.clone()),
                        });
                    }
                    window.imp().typed.borrow_mut().entry(id).or_default().1 =
                        Some(answer.password);
                    window.send_connect(id);
                }
            ),
        );
    }

    fn confirm_host_key(&self, cfg: &ConnectionConfig, fingerprint: &str) {
        let Some(ssh) = &cfg.ssh else {
            return;
        };
        let (id, host, port) = (cfg.id, ssh.host.clone(), ssh.port);
        dialogs::confirm_host_key(
            self,
            &ssh.host,
            port,
            fingerprint,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || {
                    window
                        .imp()
                        .pending_trust
                        .replace(Some((id, host.clone(), port)));
                    window.send(Command::TrustHostKey {
                        host: host.clone(),
                        port,
                    });
                }
            ),
        );
    }

    fn ask_password(&self, cfg: &ConnectionConfig, error: Option<&str>) {
        let can_remember = self.imp().keyring_available.get() != Some(false);
        let id = cfg.id;
        let config = cfg.clone();
        dialogs::ask_password(
            self,
            cfg,
            error,
            can_remember,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |answer| {
                    if answer.remember {
                        window.send(Command::SaveConnection {
                            config: config.clone(),
                            password: PasswordChange::Set(answer.password.clone()),
                            ssh_password: PasswordChange::Keep,
                        });
                    }
                    window.imp().typed.borrow_mut().entry(id).or_default().0 =
                        Some(answer.password);
                    window.send_connect(id);
                }
            ),
        );
    }

    fn duplicate(&self, id: ConnectionId) {
        if let Some(cfg) = self.connection(id) {
            let name = trf("{name} (copy)", &[("name", &cfg.name)]);
            self.send(Command::DuplicateConnection { id, name });
        }
    }

    fn delete(&self, id: ConnectionId) {
        if let Some(cfg) = self.connection(id) {
            dialogs::confirm_delete(
                self,
                &cfg,
                glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    move || {
                        window.send(Command::DeleteConnection(id));
                    }
                ),
            );
        }
    }

    // ----------------------------------------------------------------
    // Engine events
    // ----------------------------------------------------------------

    /// Applies an engine event.
    pub fn handle_event(&self, event: UiEvent) {
        let imp = self.imp();
        match event {
            UiEvent::Ready => {}
            UiEvent::StoreUnavailable(error) => {
                let toast = adw::Toast::builder()
                    .title(glib::markup_escape_text(&format!(
                        "{} {}",
                        messages::error_summary(error.kind),
                        tr("Connections cannot be saved.")
                    )))
                    .timeout(0)
                    .build();
                imp.toast_overlay.add_toast(toast);
            }
            UiEvent::KeyringStatus { available } => {
                imp.keyring_available.set(Some(available));
                if let Some(dialog) = imp.dialog.borrow().as_ref() {
                    dialog.set_keyring_available(available);
                }
            }
            UiEvent::ConnectionsLoaded(list) => self.sync_connections(&list),
            UiEvent::ConnectionSaved(cfg) => self.on_saved(cfg),
            UiEvent::ConnectionDeleted(id) => {
                self.close_tabs_of(id);
                if let Some((index, _)) = self.root_node(id) {
                    self.roots().remove(index);
                }
                if imp.selected.get() == Some(id) {
                    imp.selected.set(None);
                }
                self.refresh_content();
                self.toast(&tr("Connection deleted"));
            }
            UiEvent::TestFinished { request, result } => {
                if let Some(dialog) = imp.dialog.borrow().as_ref() {
                    dialog.on_test_finished(request, &result);
                }
            }
            UiEvent::SessionState { id, state } => self.on_session_state(id, state),
            UiEvent::ImportCandidates {
                candidates,
                warnings,
            } => {
                let existing: Vec<ConnectionConfig> = self
                    .root_nodes()
                    .iter()
                    .filter_map(TreeNode::config)
                    .collect();
                dialogs::import(
                    self,
                    candidates,
                    &warnings,
                    &existing,
                    glib::clone!(
                        #[weak(rename_to = window)]
                        self,
                        move |chosen| {
                            for config in chosen {
                                window.send(Command::SaveConnection {
                                    config,
                                    password: PasswordChange::Keep,
                                    ssh_password: PasswordChange::Keep,
                                });
                            }
                        }
                    ),
                );
            }
            UiEvent::OperationFailed(error) => {
                let dialog = imp.dialog.borrow().clone();
                match dialog {
                    Some(dialog) if dialog.is_saving() => dialog.on_save_failed(&error),
                    _ => {
                        tracing::debug!(detail = %error.detail, "operation failed");
                        self.toast(&messages::error_summary(error.kind));
                    }
                }
            }
            UiEvent::HostKeyTrusted { host, port } => {
                let dialog = imp.dialog.borrow().clone();
                if dialog.is_some_and(|d| d.on_host_key_trusted(&host, port)) {
                    return;
                }
                let pending = imp.pending_trust.borrow().clone();
                if let Some((id, h, p)) = pending
                    && h == host
                    && p == port
                {
                    imp.pending_trust.replace(None);
                    self.send_connect(id);
                }
            }
            UiEvent::MetadataLoaded { request, result } => {
                let pending = imp.pending.borrow_mut().remove(&request);
                if let Some(Pending::Children(node, what)) = pending {
                    self.on_children(&node, &what, result);
                }
            }
            UiEvent::TableDescribed { request, result } => {
                let pending = imp.pending.borrow_mut().remove(&request);
                if let Some(Pending::TabDetails(conn, object)) = pending
                    && let Some(tab) = self.tab(conn, &object)
                {
                    tab.set_details(&result);
                }
            }
            UiEvent::DdlLoaded { request, result } => {
                let pending = imp.pending.borrow_mut().remove(&request);
                match pending {
                    Some(Pending::TabDdl(conn, object)) => {
                        if let Some(tab) = self.tab(conn, &object) {
                            tab.set_ddl(&result);
                        }
                    }
                    Some(Pending::CopyDdl) => match result {
                        Ok(ddl) => {
                            self.clipboard().set_text(&ddl);
                            self.toast(&tr("DDL copied"));
                        }
                        Err(error) => self.toast(&messages::error_summary(error.kind)),
                    },
                    _ => {}
                }
            }
            other => tracing::debug!(?other, "unhandled engine event"),
        }
    }

    fn on_children(
        &self,
        node: &TreeNode,
        what: &MetaRequest,
        result: Result<Vec<MetaItem>, ErrorReport>,
    ) {
        match result {
            Ok(items) => {
                node.set_children(items);
                // Open the current database right away: it is almost always
                // the one the user wants to browse.
                if let (NodeKind::Connection(id), MetaRequest::Databases) = (node.kind(), what)
                    && let Some(root) = self.root_row(id)
                {
                    let current = node
                        .loaded_children()
                        .iter()
                        .position(|c| matches!(c.kind(), NodeKind::Database { current: true, .. }));
                    if let Some(row) = current
                        .and_then(|i| u32::try_from(i).ok())
                        .and_then(|i| root.child_row(i))
                    {
                        row.set_expanded(true);
                    }
                }
            }
            Err(error) => node.set_error(&messages::error_summary(error.kind), &error.detail),
        }
    }

    /// Updates the root nodes from the saved connections, keeping the
    /// expansion of unchanged ones.
    fn sync_connections(&self, list: &[ConnectionConfig]) {
        let roots = self.roots();
        for node in self.root_nodes() {
            if let NodeKind::Connection(id) = node.kind()
                && !list.iter().any(|c| c.id == id)
                && let Some((index, _)) = self.root_node(id)
            {
                roots.remove(index);
            }
        }
        for (position, cfg) in list.iter().enumerate() {
            let position = u32::try_from(position).unwrap_or(u32::MAX);
            match self.root_node(cfg.id) {
                Some((index, node)) => {
                    node.set_config(cfg.clone());
                    if index != position {
                        roots.remove(index);
                        roots.insert(position.min(roots.n_items()), &node);
                    }
                }
                None => roots.insert(position.min(roots.n_items()), &TreeNode::connection(cfg)),
            }
        }
        self.refresh_content();
    }

    fn on_saved(&self, cfg: ConnectionConfig) {
        let imp = self.imp();
        let id = cfg.id;
        let mut list: Vec<ConnectionConfig> = self
            .root_nodes()
            .iter()
            .filter_map(TreeNode::config)
            .collect();
        match list.iter_mut().find(|c| c.id == id) {
            Some(existing) => *existing = cfg,
            None => list.push(cfg),
        }
        list.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.id.cmp(&b.id))
        });
        self.sync_connections(&list);

        let dialog = imp.dialog.borrow().clone();
        if let Some(dialog) = dialog.filter(|d| d.id() == id && d.is_saving()) {
            dialog.close();
            self.select_connection(id);
            self.toast(&tr("Connection saved"));
        }
    }

    fn select_connection(&self, id: ConnectionId) {
        let imp = self.imp();
        let (Some(selection), Some(row)) = (imp.selection.get(), self.root_row(id)) else {
            return;
        };
        for i in 0..selection.n_items() {
            if selection.item(i).as_ref() == Some(row.upcast_ref()) {
                selection.set_selected(i);
                break;
            }
        }
    }

    fn on_session_state(&self, id: ConnectionId, state: SessionState) {
        let imp = self.imp();
        let Some((_, node)) = self.root_node(id) else {
            return;
        };
        node.set_state(state.clone());
        match &state {
            SessionState::Connected(_) => {
                imp.typed.borrow_mut().remove(&id);
                let expand = imp.expand_on_connect.borrow_mut().remove(&id);
                if let Some(row) = self.root_row(id) {
                    if node.load_state() == LoadState::Loading && row.is_expanded() {
                        // Expanded while disconnected: load now.
                        node.reset();
                        self.load_children(&node);
                    } else if expand && !row.is_expanded() {
                        row.set_expanded(true);
                    }
                }
            }
            SessionState::Disconnected | SessionState::Failed { .. } => {
                imp.expand_on_connect.borrow_mut().remove(&id);
                if let Some(row) = self.root_row(id) {
                    row.set_expanded(false);
                }
                node.reset();
            }
            SessionState::Connecting | SessionState::Reconnecting => {}
        }
        self.refresh_content();

        if let (
            SessionState::Failed {
                error,
                needs_password,
            },
            Some(cfg),
        ) = (state, node.config())
        {
            let ssh_secret_needed = matches!(
                error.kind,
                ErrorKind::MissingSshSecret | ErrorKind::SshAuthentication
            ) && cfg.ssh.as_ref().is_some_and(|t| t.auth != SshAuth::Agent);
            if error.kind == ErrorKind::UnknownHostKey {
                self.confirm_host_key(&cfg, &error.detail);
            } else if ssh_secret_needed {
                let previous = (error.kind == ErrorKind::SshAuthentication)
                    .then(|| messages::error_summary(error.kind));
                self.ask_ssh_secret(&cfg, previous.as_deref());
            } else if needs_password {
                let previous = (error.kind == ErrorKind::Authentication)
                    .then(|| messages::error_summary(error.kind));
                self.ask_password(&cfg, previous.as_deref());
            } else {
                imp.typed.borrow_mut().remove(&id);
                self.toast(&trf(
                    "{name}: {error}",
                    &[
                        ("name", &cfg.name),
                        ("error", &messages::error_summary(error.kind)),
                    ],
                ));
            }
        }
    }

    // ----------------------------------------------------------------
    // Content
    // ----------------------------------------------------------------

    fn refresh_content(&self) {
        let imp = self.imp();
        if imp.tab_view.n_pages() > 0 {
            let tab_connection = imp.tab_view.selected_page().and_then(|page| {
                imp.tabs
                    .borrow()
                    .iter()
                    .find(|(_, p)| *p == page)
                    .map(|(t, _)| t.connection)
            });
            imp.content_stack.set_visible_child_name("tabs");
            self.update_banner(tab_connection.and_then(|id| self.connection(id)).as_ref());
            return;
        }

        let cfg = imp.selected.get().and_then(|id| self.connection(id));
        let Some(cfg) = cfg else {
            imp.banner.set_revealed(false);
            if self.roots().n_items() == 0 {
                imp.welcome_page.set_title(&tr("Welcome to Strata"));
                imp.welcome_page.set_description(Some(&tr(
                    "Connect to a PostgreSQL or MySQL/MariaDB database to get started.",
                )));
            } else {
                imp.welcome_page.set_title("Strata");
                imp.welcome_page.set_description(Some(&tr(
                    "Select a connection in the sidebar, or create a new one.",
                )));
            }
            imp.content_stack.set_visible_child_name("welcome");
            return;
        };

        let state = self.state(cfg.id);
        imp.connection_title.set_text(&cfg.name);
        imp.connection_subtitle.set_text(&format!(
            "{} — {}",
            messages::db_kind(cfg.kind),
            cfg.summary()
        ));

        let connected = matches!(state, Some(SessionState::Connected(_)));
        let busy = matches!(
            state,
            Some(SessionState::Connecting | SessionState::Reconnecting)
        );
        imp.connect_button.set_visible(!connected);
        imp.connect_button.set_sensitive(!busy);
        imp.disconnect_button.set_visible(connected);

        let status = match &state {
            None | Some(SessionState::Disconnected) => tr("Not connected"),
            Some(SessionState::Connecting) => tr("Connecting…"),
            Some(SessionState::Reconnecting) => tr("Reconnecting…"),
            Some(SessionState::Connected(_)) => tr("Connected"),
            Some(SessionState::Failed { error, .. }) => messages::error_summary(error.kind),
        };
        imp.status_row
            .set_subtitle(&glib::markup_escape_text(&status));

        let local = cfg.endpoint.is_local();
        let (server, encryption) = match &state {
            Some(SessionState::Connected(info)) => (
                format!("{} {}", info.product, info.version),
                match info.encrypted {
                    Some(true) => tr("Encrypted (TLS)"),
                    Some(false) if cfg.ssh.is_some() => tr("Encrypted by the SSH tunnel"),
                    Some(false) if local => tr("Not encrypted (local connection)"),
                    Some(false) => tr("Not encrypted"),
                    None => tr("Unknown"),
                },
            ),
            _ => (
                "—".to_owned(),
                match &cfg.endpoint {
                    Endpoint::Socket { .. } => tr("Unix socket (local)"),
                    Endpoint::Tcp { .. } => messages::tls_mode(cfg.tls.mode),
                },
            ),
        };
        imp.server_row
            .set_subtitle(&glib::markup_escape_text(&server));
        imp.encryption_row
            .set_subtitle(&glib::markup_escape_text(&encryption));
        imp.environment_row
            .set_subtitle(&messages::environment(cfg.environment));
        imp.access_row.set_subtitle(&if cfg.read_only {
            tr("Read-only")
        } else {
            tr("Read and write")
        });
        imp.ssh_row.set_visible(cfg.ssh.is_some());
        if let Some(ssh) = &cfg.ssh {
            imp.ssh_row
                .set_subtitle(&glib::markup_escape_text(&ssh.summary()));
        }
        self.update_banner(Some(&cfg));
        imp.content_stack.set_visible_child_name("connection");
    }

    /// Strong visual signal for production and clear-text connections
    /// (CDC §8, §10.2).
    fn update_banner(&self, cfg: Option<&ConnectionConfig>) {
        let imp = self.imp();
        let text = cfg.and_then(|cfg| {
            let unencrypted = matches!(
                self.state(cfg.id),
                Some(SessionState::Connected(ref info)) if info.encrypted == Some(false)
            ) && !cfg.endpoint.is_local()
                && cfg.ssh.is_none();
            if cfg.is_production() {
                Some(trf(
                    "{name}: production environment, changes affect real data.",
                    &[("name", &cfg.name)],
                ))
            } else if unencrypted {
                Some(tr(
                    "This connection is not encrypted: data travels in clear text on the network.",
                ))
            } else {
                None
            }
        });
        match text {
            Some(text) => {
                imp.banner.set_title(&glib::markup_escape_text(&text));
                imp.banner.set_revealed(true);
            }
            None => imp.banner.set_revealed(false),
        }
    }
}
