// SPDX-License-Identifier: GPL-3.0-or-later
//! Dialog to create, edit and test a connection (CON-1 to CON-5, CON-8, CON-12).

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use adw::prelude::*;
use gtk::glib;
use strata_core::{
    ConnectionConfig, ConnectionId, DbKind, Endpoint, Environment, ErrorKind, ErrorReport,
    PasswordPolicy, Secret, SshAuth, SshTunnel, TlsConfig, TlsMode, ValidationError,
};
use strata_engine::{Command, EngineHandle, PasswordChange, RequestId, TestReport};

use crate::i18n::{tr, trf};
use crate::messages;

/// Correlation identifiers of connection tests.
static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

const KINDS: [DbKind; 2] = [DbKind::Postgres, DbKind::MySql];
const ENVIRONMENTS: [Option<Environment>; 4] = [
    None,
    Some(Environment::Development),
    Some(Environment::Staging),
    Some(Environment::Production),
];
const POLICIES: [PasswordPolicy; 4] = [
    PasswordPolicy::Keyring,
    PasswordPolicy::AskEveryTime,
    PasswordPolicy::ClientConfigFile,
    PasswordPolicy::None,
];
/// Where the SSH password or passphrase comes from.
const SSH_POLICIES: [PasswordPolicy; 3] = [
    PasswordPolicy::Keyring,
    PasswordPolicy::AskEveryTime,
    PasswordPolicy::None,
];
/// Indexes of the SSH authentication combo.
const SSH_AGENT: u32 = 0;
const SSH_PASSWORD: u32 = 1;
const SSH_KEY: u32 = 2;

/// Label colours (GNOME palette) and their translatable names.
const COLORS: [(&str, &str); 7] = [
    ("#3584e4", "Blue"),
    ("#2ec27e", "Green"),
    ("#f5c211", "Yellow"),
    ("#ff7800", "Orange"),
    ("#e01b24", "Red"),
    ("#9141ac", "Purple"),
    ("#986a44", "Brown"),
];

/// Translated colour names (listed so that xgettext extracts them).
fn color_name(name: &str) -> String {
    match name {
        "Blue" => tr("Blue"),
        "Green" => tr("Green"),
        "Yellow" => tr("Yellow"),
        "Orange" => tr("Orange"),
        "Red" => tr("Red"),
        "Purple" => tr("Purple"),
        _ => tr("Brown"),
    }
}

/// A row holding an optional file path, with "choose" and "clear" buttons.
struct FileRow {
    row: adw::ActionRow,
    path: Rc<RefCell<Option<PathBuf>>>,
}

impl FileRow {
    fn new(title: &str, initial: Option<PathBuf>) -> Self {
        let row = adw::ActionRow::builder().title(title).build();
        let path = Rc::new(RefCell::new(initial));
        let choose = gtk::Button::builder()
            .icon_name("document-open-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text(tr("Choose File…"))
            .build();
        choose.add_css_class("flat");
        let clear = gtk::Button::builder()
            .icon_name("edit-clear-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text(tr("Clear"))
            .build();
        clear.add_css_class("flat");
        row.add_suffix(&choose);
        row.add_suffix(&clear);
        row.set_activatable_widget(Some(&choose));

        let this = Self { row, path };
        this.refresh();

        choose.connect_clicked(glib::clone!(
            #[weak(rename_to = row)]
            this.row,
            #[strong(rename_to = path)]
            this.path,
            move |_| {
                let dialog = gtk::FileDialog::builder().modal(true).build();
                let parent = row.root().and_downcast::<gtk::Window>();
                let path = Rc::clone(&path);
                glib::spawn_future_local(async move {
                    if let Ok(file) = dialog.open_future(parent.as_ref()).await {
                        *path.borrow_mut() = file.path();
                        FileRow::show(&row, path.borrow().as_ref());
                    }
                });
            }
        ));
        clear.connect_clicked(glib::clone!(
            #[weak(rename_to = row)]
            this.row,
            #[strong(rename_to = path)]
            this.path,
            move |_| {
                *path.borrow_mut() = None;
                FileRow::show(&row, None);
            }
        ));
        this
    }

    fn refresh(&self) {
        Self::show(&self.row, self.path.borrow().as_ref());
    }

    fn show(row: &adw::ActionRow, path: Option<&PathBuf>) {
        match path {
            Some(p) => row.set_subtitle(&glib::markup_escape_text(&p.display().to_string())),
            None => row.set_subtitle(&tr("None")),
        }
    }

    fn value(&self) -> Option<PathBuf> {
        self.path.borrow().clone()
    }
}

struct Inner {
    window: adw::Window,
    toasts: adw::ToastOverlay,
    engine: EngineHandle,
    id: ConnectionId,
    editing: bool,
    keyring_available: Cell<Option<bool>>,
    pending_test: Cell<Option<RequestId>>,
    saving: Cell<bool>,

    name: adw::EntryRow,
    kind: adw::ComboRow,
    environment: adw::ComboRow,
    color: adw::ComboRow,
    custom_color: Option<String>,
    method: adw::ComboRow,
    host: adw::EntryRow,
    port: adw::SpinRow,
    socket: adw::EntryRow,
    database: adw::EntryRow,
    user: adw::EntryRow,
    policy: adw::ComboRow,
    password: adw::PasswordEntryRow,
    keyring_warning: adw::ActionRow,
    tls_group: adw::PreferencesGroup,
    tls: adw::ComboRow,
    ca: FileRow,
    cert: FileRow,
    key: FileRow,
    read_only: adw::SwitchRow,
    history: adw::SwitchRow,
    test_button: gtk::Button,
    test_spinner: gtk::Spinner,
    test_icon: gtk::Image,
    test_summary: gtk::Label,
    test_details: gtk::Button,
    test_detail_text: RefCell<String>,
    save_button: gtk::Button,
    last_kind: Cell<DbKind>,
    ssh_group: adw::PreferencesGroup,
    ssh: adw::ExpanderRow,
    ssh_host: adw::EntryRow,
    ssh_port: adw::SpinRow,
    ssh_user: adw::EntryRow,
    ssh_auth: adw::ComboRow,
    ssh_key: FileRow,
    ssh_policy: adw::ComboRow,
    ssh_secret: adw::PasswordEntryRow,
    /// SSH server whose key the user agreed to trust; the test reruns once
    /// the engine confirms.
    pending_trust: RefCell<Option<(String, u16)>>,
}

/// The connection dialog. Cheap to clone (shared state).
#[derive(Clone)]
pub(crate) struct ConnectionDialog(Rc<Inner>);

impl std::fmt::Debug for ConnectionDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionDialog")
            .field("id", &self.0.id)
            .finish_non_exhaustive()
    }
}

fn string_list(items: &[String]) -> gtk::StringList {
    let refs: Vec<&str> = items.iter().map(String::as_str).collect();
    gtk::StringList::new(&refs)
}

fn combo(title: &str, items: &[String], selected: usize) -> adw::ComboRow {
    let row = adw::ComboRow::builder()
        .title(title)
        .model(&string_list(items))
        .build();
    row.set_selected(u32::try_from(selected).unwrap_or(0));
    row
}

fn index_of<T: PartialEq>(items: &[T], value: &T) -> usize {
    items.iter().position(|v| v == value).unwrap_or(0)
}

fn selected<T: Copy>(row: &adw::ComboRow, items: &[T]) -> T {
    let index = usize::try_from(row.selected()).unwrap_or(0);
    items.get(index).copied().unwrap_or(items[0])
}

impl ConnectionDialog {
    /// Builds the dialog; `existing` is `None` for a new connection.
    pub(crate) fn new(
        parent: &impl IsA<gtk::Window>,
        engine: EngineHandle,
        existing: Option<&ConnectionConfig>,
        keyring_available: Option<bool>,
    ) -> Self {
        let editing = existing.is_some();
        let mut cfg = existing.cloned().unwrap_or_else(|| {
            let mut cfg = ConnectionConfig::new("", DbKind::Postgres);
            cfg.user = std::env::var("USER").unwrap_or_default();
            if keyring_available == Some(false) {
                cfg.password_policy = PasswordPolicy::AskEveryTime;
            }
            cfg
        });
        if cfg.name.is_empty() && editing {
            cfg.name = tr("Unnamed");
        }

        // --- General ---------------------------------------------------
        let name = adw::EntryRow::builder()
            .title(tr("Name"))
            .text(&cfg.name)
            .build();
        let kind = combo(
            &tr("Type"),
            &KINDS.map(messages::db_kind),
            index_of(&KINDS, &cfg.kind),
        );
        let environment = combo(
            &tr("Environment"),
            &ENVIRONMENTS.map(messages::environment),
            index_of(&ENVIRONMENTS, &cfg.environment),
        );
        let mut color_items = vec![tr("None")];
        color_items.extend(COLORS.iter().map(|(_, n)| color_name(n)));
        let custom_color = cfg
            .color
            .clone()
            .filter(|c| !COLORS.iter().any(|(hex, _)| hex.eq_ignore_ascii_case(c)));
        if let Some(custom) = &custom_color {
            color_items.push(trf("Custom ({color})", &[("color", custom)]));
        }
        let color_index = match &cfg.color {
            None => 0,
            Some(c) => COLORS
                .iter()
                .position(|(hex, _)| hex.eq_ignore_ascii_case(c))
                .map_or(color_items.len() - 1, |i| i + 1),
        };
        let color = combo(&tr("Label Color"), &color_items, color_index);

        let general = adw::PreferencesGroup::new();
        general.add(&name);
        general.add(&kind);
        general.add(&environment);
        general.add(&color);

        // --- Server ----------------------------------------------------
        let (is_socket, host_text, port_value, socket_text) = match &cfg.endpoint {
            Endpoint::Tcp { host, port } => (false, host.clone(), *port, String::new()),
            Endpoint::Socket { path } => (
                true,
                "localhost".to_owned(),
                cfg.kind.default_port(),
                path.display().to_string(),
            ),
        };
        let method = combo(
            &tr("Connection Method"),
            &[tr("Network (TCP/IP)"), tr("Unix socket")],
            usize::from(is_socket),
        );
        let host = adw::EntryRow::builder()
            .title(tr("Host"))
            .text(&host_text)
            .build();
        let port = adw::SpinRow::with_range(1.0, 65535.0, 1.0);
        port.set_title(&tr("Port"));
        port.set_value(f64::from(port_value));
        let socket = adw::EntryRow::builder()
            .title(tr("Socket Path (file or directory)"))
            .text(&socket_text)
            .build();
        let database = adw::EntryRow::builder()
            .title(tr("Database (optional)"))
            .text(cfg.database.as_deref().unwrap_or_default())
            .build();
        let user = adw::EntryRow::builder()
            .title(tr("User"))
            .text(&cfg.user)
            .build();

        let server = adw::PreferencesGroup::builder().title(tr("Server")).build();
        server.add(&method);
        server.add(&host);
        server.add(&port);
        server.add(&socket);
        server.add(&database);
        server.add(&user);

        // --- SSH tunnel ---------------------------------------------------
        let ssh_cfg = cfg.ssh.clone();
        let ssh = adw::ExpanderRow::builder()
            .title(tr("SSH Tunnel"))
            .subtitle(tr("Reach the database through an SSH server"))
            .show_enable_switch(true)
            .enable_expansion(ssh_cfg.is_some())
            .expanded(ssh_cfg.is_some())
            .build();
        let ssh_host = adw::EntryRow::builder()
            .title(tr("SSH Server"))
            .text(
                ssh_cfg
                    .as_ref()
                    .map(|t| t.host.as_str())
                    .unwrap_or_default(),
            )
            .build();
        let ssh_port = adw::SpinRow::with_range(1.0, 65535.0, 1.0);
        ssh_port.set_title(&tr("SSH Port"));
        ssh_port.set_value(f64::from(
            ssh_cfg.as_ref().map_or(SshTunnel::DEFAULT_PORT, |t| t.port),
        ));
        let ssh_user = adw::EntryRow::builder()
            .title(tr("SSH User"))
            .text(
                ssh_cfg
                    .as_ref()
                    .map(|t| t.user.clone())
                    .or_else(|| std::env::var("USER").ok())
                    .unwrap_or_default(),
            )
            .build();
        let ssh_auth = combo(
            &tr("SSH Authentication"),
            &[tr("SSH agent"), tr("Password"), tr("Private key")],
            match ssh_cfg.as_ref().map(|t| &t.auth) {
                Some(SshAuth::Password) => 1,
                Some(SshAuth::PrivateKey { .. }) => 2,
                _ => 0,
            },
        );
        let default_key = std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(".ssh/id_ed25519"))
            .filter(|p| p.exists());
        let ssh_key = FileRow::new(
            &tr("Private Key"),
            match ssh_cfg.as_ref().map(|t| &t.auth) {
                Some(SshAuth::PrivateKey { path }) => Some(path.clone()),
                _ => default_key,
            },
        );
        let ssh_policy = combo(
            &tr("SSH Password or Passphrase"),
            &[
                tr("Save in the system keyring"),
                tr("Ask every time"),
                tr("None"),
            ],
            index_of(
                &SSH_POLICIES,
                &ssh_cfg.as_ref().map_or(
                    if keyring_available == Some(false) {
                        PasswordPolicy::AskEveryTime
                    } else {
                        PasswordPolicy::Keyring
                    },
                    |t| t.secret_policy,
                ),
            ),
        );
        let ssh_secret = adw::PasswordEntryRow::new();
        ssh.add_row(&ssh_host);
        ssh.add_row(&ssh_port);
        ssh.add_row(&ssh_user);
        ssh.add_row(&ssh_auth);
        ssh.add_row(&ssh_key.row);
        ssh.add_row(&ssh_policy);
        ssh.add_row(&ssh_secret);
        let ssh_group = adw::PreferencesGroup::new();
        ssh_group.add(&ssh);

        // --- Authentication -------------------------------------------
        let policy = combo(
            &tr("Password"),
            &POLICIES.map(messages::password_policy),
            index_of(&POLICIES, &cfg.password_policy),
        );
        let password = adw::PasswordEntryRow::builder()
            .title(if editing {
                tr("New password (leave empty to keep the current one)")
            } else {
                tr("Password")
            })
            .build();
        let keyring_warning = adw::ActionRow::builder()
            .title(tr("The system keyring is unavailable"))
            .subtitle(tr(
                "Passwords cannot be saved. Choose “Ask every time” instead.",
            ))
            .build();
        let warning_icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
        warning_icon.add_css_class("warning");
        keyring_warning.add_prefix(&warning_icon);

        let auth = adw::PreferencesGroup::builder()
            .title(tr("Authentication"))
            .build();
        auth.add(&policy);
        auth.add(&password);
        auth.add(&keyring_warning);

        // --- Security --------------------------------------------------
        let tls = combo(
            &tr("Encryption (TLS)"),
            &TlsMode::ALL
                .iter()
                .map(|m| messages::tls_mode(*m))
                .collect::<Vec<_>>(),
            index_of(TlsMode::ALL, &cfg.tls.mode),
        );
        let ca = FileRow::new(&tr("CA Certificate"), cfg.tls.ca_cert.clone());
        let cert = FileRow::new(&tr("Client Certificate"), cfg.tls.client_cert.clone());
        let key = FileRow::new(&tr("Client Private Key"), cfg.tls.client_key.clone());
        let tls_group = adw::PreferencesGroup::builder()
            .title(tr("Security"))
            .build();
        tls_group.add(&tls);
        tls_group.add(&ca.row);
        tls_group.add(&cert.row);
        tls_group.add(&key.row);

        let read_only = adw::SwitchRow::builder()
            .title(tr("Read-Only"))
            .subtitle(tr("Reject any modification of the data"))
            .active(cfg.read_only)
            .build();
        let history = adw::SwitchRow::builder()
            .title(tr("Record Query History"))
            .active(cfg.record_history)
            .build();
        let options = adw::PreferencesGroup::builder()
            .title(tr("Options"))
            .build();
        options.add(&read_only);
        options.add(&history);

        // --- Test -------------------------------------------------------
        let test_button = gtk::Button::builder()
            .label(tr("_Test Connection"))
            .use_underline(true)
            .halign(gtk::Align::Start)
            .build();
        test_button.add_css_class("pill");
        let test_spinner = gtk::Spinner::new();
        let test_icon = gtk::Image::builder().visible(false).build();
        let test_summary = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .xalign(0.0)
            .hexpand(true)
            .build();
        let test_details = gtk::Button::builder()
            .label(tr("_Details"))
            .use_underline(true)
            .valign(gtk::Align::Center)
            .visible(false)
            .build();
        test_details.add_css_class("flat");
        let test_box = gtk::Box::builder().spacing(12).hexpand(true).build();
        test_box.append(&test_button);
        test_box.append(&test_spinner);
        test_box.append(&test_icon);
        test_box.append(&test_summary);
        test_box.append(&test_details);

        // --- Layout ----------------------------------------------------
        let page = adw::PreferencesPage::new();
        page.add(&general);
        page.add(&server);
        page.add(&ssh_group);
        page.add(&auth);
        page.add(&tls_group);
        page.add(&options);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&page);
        page.set_vexpand(true);

        let cancel = gtk::Button::with_mnemonic(&tr("_Cancel"));
        let save_button = gtk::Button::with_mnemonic(&tr("_Save"));
        save_button.add_css_class("suggested-action");
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .build();
        header.pack_start(&cancel);
        header.pack_end(&save_button);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        let bottom = gtk::ActionBar::new();
        bottom.pack_start(&test_box);
        toolbar.add_bottom_bar(&bottom);
        toolbar.set_content(Some(&content));

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&toolbar));

        let window = adw::Window::builder()
            .title(if editing {
                tr("Edit Connection")
            } else {
                tr("New Connection")
            })
            .modal(true)
            .transient_for(parent)
            .default_width(560)
            .default_height(720)
            .width_request(360)
            .height_request(400)
            .content(&toasts)
            .build();
        window.set_default_widget(Some(&save_button));

        let dialog = Self(Rc::new(Inner {
            window,
            toasts,
            engine,
            id: cfg.id,
            editing,
            keyring_available: Cell::new(keyring_available),
            pending_test: Cell::new(None),
            saving: Cell::new(false),
            name,
            kind,
            environment,
            color,
            custom_color,
            method,
            host,
            port,
            socket,
            database,
            user,
            policy,
            password,
            keyring_warning,
            tls_group,
            tls,
            ca,
            cert,
            key,
            read_only,
            history,
            test_button,
            test_spinner,
            test_icon,
            test_summary,
            test_details,
            test_detail_text: RefCell::new(String::new()),
            save_button,
            last_kind: Cell::new(cfg.kind),
            ssh_group,
            ssh,
            ssh_host,
            ssh_port,
            ssh_user,
            ssh_auth,
            ssh_key,
            ssh_policy,
            ssh_secret,
            pending_trust: RefCell::new(None),
        }));
        dialog.connect_signals(&cancel);
        dialog.update_visibility();
        dialog
    }

    fn connect_signals(&self, cancel: &gtk::Button) {
        let inner = &self.0;
        let weak = Rc::downgrade(&self.0);
        let with = move |f: fn(&ConnectionDialog)| {
            let weak = weak.clone();
            move || {
                if let Some(inner) = weak.upgrade() {
                    f(&ConnectionDialog(inner));
                }
            }
        };

        let refresh = with(Self::update_visibility);
        inner.method.connect_selected_notify(move |_| refresh());
        let refresh = with(Self::update_visibility);
        inner.policy.connect_selected_notify(move |_| refresh());
        let refresh = with(Self::update_visibility);
        inner.tls.connect_selected_notify(move |_| refresh());
        let refresh = with(Self::update_visibility);
        inner.ssh_auth.connect_selected_notify(move |_| refresh());
        let refresh = with(Self::update_visibility);
        inner.ssh_policy.connect_selected_notify(move |_| refresh());
        let kind_changed = with(Self::on_kind_changed);
        inner.kind.connect_selected_notify(move |_| kind_changed());
        let save = with(Self::save);
        inner.save_button.connect_clicked(move |_| save());
        let test = with(Self::test);
        inner.test_button.connect_clicked(move |_| test());
        let details = with(Self::show_test_details);
        inner.test_details.connect_clicked(move |_| details());

        cancel.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            inner.window,
            move |_| window.close()
        ));
        let escape = gtk::ShortcutController::new();
        escape.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("Escape"),
            Some(gtk::NamedAction::new("window.close")),
        ));
        inner.window.add_controller(escape);

        // Clear error highlighting as soon as the user edits a field.
        for row in [
            &inner.name,
            &inner.host,
            &inner.socket,
            &inner.user,
            &inner.ssh_host,
            &inner.ssh_user,
        ] {
            row.connect_changed(|row| row.remove_css_class("error"));
        }
    }

    fn on_kind_changed(&self) {
        let inner = &self.0;
        let kind = selected(&inner.kind, &KINDS);
        let previous = inner.last_kind.replace(kind);
        // Follow the default port unless the user changed it.
        if inner.port.value() == f64::from(previous.default_port()) {
            inner.port.set_value(f64::from(kind.default_port()));
        }
    }

    fn update_visibility(&self) {
        let inner = &self.0;
        let socket = inner.method.selected() == 1;
        inner.host.set_visible(!socket);
        inner.port.set_visible(!socket);
        inner.socket.set_visible(socket);
        // TLS never applies to Unix sockets.
        inner.tls_group.set_visible(!socket);

        let mode = selected(&inner.tls, TlsMode::ALL);
        inner.tls.set_subtitle(&messages::tls_mode_help(mode));
        let uses_tls = mode != TlsMode::Disable;
        inner.ca.row.set_visible(uses_tls);
        inner.cert.row.set_visible(uses_tls);
        inner.key.row.set_visible(uses_tls);

        let policy = selected(&inner.policy, &POLICIES);
        inner.password.set_visible(matches!(
            policy,
            PasswordPolicy::Keyring | PasswordPolicy::AskEveryTime
        ));
        if policy == PasswordPolicy::AskEveryTime {
            inner
                .password
                .set_title(&tr("Password (used for the test only, never saved)"));
        } else if inner.editing {
            inner
                .password
                .set_title(&tr("New password (leave empty to keep the current one)"));
        } else {
            inner.password.set_title(&tr("Password"));
        }
        inner.keyring_warning.set_visible(
            policy == PasswordPolicy::Keyring && inner.keyring_available.get() == Some(false),
        );

        // SSH tunnels only apply to TCP endpoints.
        inner.ssh_group.set_visible(!socket);
        let auth = inner.ssh_auth.selected();
        let ssh_policy = selected(&inner.ssh_policy, &SSH_POLICIES);
        inner.ssh_key.row.set_visible(auth == SSH_KEY);
        inner.ssh_policy.set_visible(auth != SSH_AGENT);
        inner.ssh_secret.set_visible(
            auth != SSH_AGENT
                && matches!(
                    ssh_policy,
                    PasswordPolicy::Keyring | PasswordPolicy::AskEveryTime
                ),
        );
        // Whole sentences, so that translators can adapt grammar.
        let title = match (auth == SSH_KEY, ssh_policy, inner.editing) {
            (true, PasswordPolicy::AskEveryTime, _) => {
                tr("Key passphrase (used for the test only, never saved)")
            }
            (false, PasswordPolicy::AskEveryTime, _) => {
                tr("SSH password (used for the test only, never saved)")
            }
            (true, _, true) => tr("New key passphrase (leave empty to keep the current one)"),
            (false, _, true) => tr("New SSH password (leave empty to keep the current one)"),
            (true, _, false) => tr("Key passphrase"),
            (false, _, false) => tr("SSH password"),
        };
        inner.ssh_secret.set_title(&title);
    }

    /// Settings as currently entered.
    fn collect(&self) -> ConnectionConfig {
        let inner = &self.0;
        let kind = selected(&inner.kind, &KINDS);
        let endpoint = if inner.method.selected() == 1 {
            Endpoint::Socket {
                path: PathBuf::from(inner.socket.text().trim()),
            }
        } else {
            Endpoint::Tcp {
                host: inner.host.text().trim().to_owned(),
                // SpinRow keeps the value within 1..=65535.
                port: inner.port.value().round().clamp(0.0, 65535.0) as u16,
            }
        };
        let color = match usize::try_from(inner.color.selected()).unwrap_or(0) {
            0 => None,
            i if i <= COLORS.len() => Some(COLORS[i - 1].0.to_owned()),
            _ => inner.custom_color.clone(),
        };
        let database = inner.database.text().trim().to_owned();
        ConnectionConfig {
            id: inner.id,
            name: inner.name.text().trim().to_owned(),
            kind,
            endpoint,
            database: (!database.is_empty()).then_some(database),
            user: inner.user.text().trim().to_owned(),
            tls: TlsConfig {
                mode: selected(&inner.tls, TlsMode::ALL),
                ca_cert: inner.ca.value(),
                client_cert: inner.cert.value(),
                client_key: inner.key.value(),
            },
            environment: selected(&inner.environment, &ENVIRONMENTS),
            color,
            group_id: None,
            read_only: inner.read_only.is_active(),
            password_policy: selected(&inner.policy, &POLICIES),
            record_history: inner.history.is_active(),
            ssh: (inner.ssh.enables_expansion() && inner.method.selected() != 1).then(|| {
                let auth = match inner.ssh_auth.selected() {
                    SSH_PASSWORD => SshAuth::Password,
                    SSH_KEY => SshAuth::PrivateKey {
                        path: inner.ssh_key.value().unwrap_or_default(),
                    },
                    _ => SshAuth::Agent,
                };
                SshTunnel {
                    host: inner.ssh_host.text().trim().to_owned(),
                    port: inner.ssh_port.value().round().clamp(0.0, 65535.0) as u16,
                    user: inner.ssh_user.text().trim().to_owned(),
                    secret_policy: if auth == SshAuth::Agent {
                        PasswordPolicy::None
                    } else {
                        selected(&inner.ssh_policy, &SSH_POLICIES)
                    },
                    auth,
                }
            }),
        }
    }

    /// Validates the settings, highlighting the faulty rows.
    fn validate(&self, cfg: &ConnectionConfig) -> bool {
        let inner = &self.0;
        let Err(errors) = cfg.validate() else {
            return true;
        };
        for error in &errors {
            let row: Option<&adw::EntryRow> = match error {
                ValidationError::EmptySshHost => Some(&inner.ssh_host),
                ValidationError::EmptySshUser => Some(&inner.ssh_user),
                ValidationError::EmptyName => Some(&inner.name),
                ValidationError::EmptyHost => Some(&inner.host),
                ValidationError::RelativeSocketPath => Some(&inner.socket),
                ValidationError::EmptyUser => Some(&inner.user),
                _ => None,
            };
            if let Some(row) = row {
                row.add_css_class("error");
            }
        }
        let message = errors
            .iter()
            .map(|e| messages::validation(*e))
            .collect::<Vec<_>>()
            .join(" ");
        self.toast(&message);
        false
    }

    fn typed_ssh_secret(&self) -> Option<Secret> {
        let inner = &self.0;
        let text = inner.ssh_secret.text();
        (inner.ssh_secret.is_visible() && !text.is_empty()).then(|| Secret::new(text.as_str()))
    }

    /// Checks the SSH choices the model cannot validate alone.
    fn validate_ssh(&self, cfg: &ConnectionConfig) -> bool {
        match &cfg.ssh {
            Some(ssh)
                if ssh.auth == SshAuth::Password && ssh.secret_policy == PasswordPolicy::None =>
            {
                self.toast(&tr("Choose how to provide the SSH password."));
                false
            }
            _ => true,
        }
    }

    fn typed_password(&self) -> Option<Secret> {
        let inner = &self.0;
        let text = inner.password.text();
        (inner.password.is_visible() && !text.is_empty()).then(|| Secret::new(text.as_str()))
    }

    fn save(&self) {
        let inner = &self.0;
        if inner.saving.get() {
            return;
        }
        let cfg = self.collect();
        if !self.validate(&cfg) || !self.validate_ssh(&cfg) {
            return;
        }
        let ssh_password = match &cfg.ssh {
            Some(ssh) if ssh.secret_policy == PasswordPolicy::Keyring => {
                match self.typed_ssh_secret() {
                    Some(_) if inner.keyring_available.get() == Some(false) => {
                        self.toast(&tr(
                            "The system keyring is unavailable: choose another password option.",
                        ));
                        return;
                    }
                    Some(secret) => PasswordChange::Set(secret),
                    None => PasswordChange::Keep,
                }
            }
            _ => PasswordChange::Keep,
        };
        let password = match cfg.password_policy {
            PasswordPolicy::Keyring => {
                if inner.keyring_available.get() == Some(false) {
                    self.toast(&tr(
                        "The system keyring is unavailable: choose another password option.",
                    ));
                    return;
                }
                self.typed_password()
                    .map_or(PasswordChange::Keep, PasswordChange::Set)
            }
            _ => PasswordChange::Keep,
        };
        if inner.engine.send(Command::SaveConnection {
            config: cfg,
            password,
            ssh_password,
        }) {
            inner.saving.set(true);
            inner.save_button.set_sensitive(false);
        } else {
            self.toast(&tr("Strata is busy, try again."));
        }
    }

    fn test(&self) {
        let inner = &self.0;
        let cfg = self.collect();
        if !self.validate(&cfg) || !self.validate_ssh(&cfg) {
            return;
        }
        let request = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        if inner.engine.send(Command::TestConnection {
            request,
            config: cfg,
            password: self.typed_password(),
            ssh_secret: self.typed_ssh_secret(),
        }) {
            inner.pending_test.set(Some(request));
            inner.test_button.set_sensitive(false);
            inner.test_spinner.set_spinning(true);
            inner.test_icon.set_visible(false);
            inner.test_details.set_visible(false);
            inner.test_summary.set_text(&tr("Testing…"));
        }
    }

    /// Shows the technical detail of the last failed test.
    fn show_test_details(&self) {
        let inner = &self.0;
        let detail = inner.test_detail_text.borrow().clone();
        let dialog =
            adw::MessageDialog::new(Some(&inner.window), Some(&inner.test_summary.text()), None);
        let label = gtk::Label::builder()
            .label(&detail)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .selectable(true)
            .xalign(0.0)
            .build();
        label.add_css_class("monospace");
        dialog.set_extra_child(Some(&label));
        dialog.add_response("close", &tr("_Close"));
        dialog.present();
    }

    fn toast(&self, message: &str) {
        self.0.toasts.add_toast(adw::Toast::new(message));
    }

    /// Identifier of the edited (or created) connection.
    pub(crate) fn id(&self) -> ConnectionId {
        self.0.id
    }

    /// Whether a save request is in flight.
    pub(crate) fn is_saving(&self) -> bool {
        self.0.saving.get()
    }

    /// Shows the dialog.
    pub(crate) fn present(&self) {
        self.0.window.present();
        self.0.name.grab_focus();
    }

    /// Closes the dialog.
    pub(crate) fn close(&self) {
        self.0.window.close();
    }

    /// Calls `f` when the dialog is closed.
    pub(crate) fn connect_closed(&self, f: impl Fn() + 'static) {
        self.0.window.connect_close_request(move |_| {
            f();
            glib::Propagation::Proceed
        });
    }

    /// Updates the keyring availability (CDC §10.1).
    pub(crate) fn set_keyring_available(&self, available: bool) {
        let inner = &self.0;
        let first_answer = inner.keyring_available.replace(Some(available)).is_none();
        if first_answer && !available && !inner.editing {
            let index = index_of(&POLICIES, &PasswordPolicy::AskEveryTime);
            inner.policy.set_selected(u32::try_from(index).unwrap_or(0));
        }
        self.update_visibility();
    }

    /// The save request failed.
    pub(crate) fn on_save_failed(&self, error: &ErrorReport) {
        let inner = &self.0;
        inner.saving.set(false);
        inner.save_button.set_sensitive(true);
        let toast = adw::Toast::builder()
            .title(glib::markup_escape_text(&messages::error_summary(
                error.kind,
            )))
            .timeout(8)
            .build();
        inner.toasts.add_toast(toast);
        tracing::debug!(detail = %error.detail, "save failed");
    }

    /// Displays the result of a connection test, if it is ours.
    pub(crate) fn on_test_finished(
        &self,
        request: RequestId,
        result: &Result<TestReport, ErrorReport>,
    ) {
        let inner = &self.0;
        if inner.pending_test.get() != Some(request) {
            return;
        }
        inner.pending_test.set(None);
        inner.test_button.set_sensitive(true);
        inner.test_spinner.set_spinning(false);
        inner.test_icon.set_visible(true);
        for class in ["success", "error", "warning"] {
            inner.test_icon.remove_css_class(class);
        }
        let (icon, class, summary, detail) = match result {
            Ok(report) => {
                let cfg = self.collect();
                // Through SSH, the network hop is encrypted by the tunnel.
                let unencrypted = report.server.encrypted == Some(false)
                    && !cfg.endpoint.is_local()
                    && cfg.ssh.is_none();
                let mut summary = trf(
                    "Connection succeeded: {product} {version} ({duration} ms)",
                    &[
                        ("product", &report.server.product),
                        // "18.3 (Debian 18.3-1)" → "18.3"; the full string is
                        // shown on the connection page.
                        (
                            "version",
                            report
                                .server
                                .version
                                .split_whitespace()
                                .next()
                                .unwrap_or_default(),
                        ),
                        ("duration", &report.duration.as_millis().to_string()),
                    ],
                );
                if unencrypted {
                    summary.push_str(" — ");
                    summary.push_str(&tr("not encrypted"));
                    ("dialog-warning-symbolic", "warning", summary, None)
                } else {
                    ("emblem-ok-symbolic", "success", summary, None)
                }
            }
            Err(error) => (
                "dialog-error-symbolic",
                "error",
                messages::error_summary(error.kind),
                Some(error.detail.clone()),
            ),
        };
        inner.test_icon.set_icon_name(Some(icon));
        inner.test_icon.add_css_class(class);
        inner.test_summary.set_text(&summary);
        inner.test_summary.set_tooltip_text(Some(&summary));
        inner.test_details.set_visible(detail.is_some());
        inner.test_detail_text.replace(detail.unwrap_or_default());

        // Unknown SSH server: offer to trust its key, then test again.
        if let Err(error) = result
            && error.kind == ErrorKind::UnknownHostKey
            && let Some(ssh) = self.collect().ssh
        {
            let weak = Rc::downgrade(&self.0);
            let (host, port) = (ssh.host.clone(), ssh.port);
            crate::dialogs::confirm_host_key(
                &inner.window,
                &ssh.host,
                port,
                &error.detail,
                move || {
                    if let Some(inner) = weak.upgrade() {
                        inner.pending_trust.replace(Some((host.clone(), port)));
                        inner.engine.send(Command::TrustHostKey {
                            host: host.clone(),
                            port,
                        });
                    }
                },
            );
        }
    }

    /// The engine trusted an SSH server key; returns whether this dialog
    /// asked for it (it then tests again).
    pub(crate) fn on_host_key_trusted(&self, host: &str, port: u16) -> bool {
        let pending = self.0.pending_trust.borrow().clone();
        if pending.is_some_and(|(h, p)| h == host && p == port) {
            self.0.pending_trust.replace(None);
            self.test();
            return true;
        }
        false
    }
}
