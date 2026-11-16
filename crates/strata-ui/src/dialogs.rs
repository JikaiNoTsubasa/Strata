// SPDX-License-Identifier: GPL-3.0-or-later
//! Small dialogs: password prompt, delete confirmation, import.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use strata_core::client_config::{ImportCandidate, ImportSource};
use strata_core::{ConnectionConfig, PasswordPolicy, Secret, SshAuth};

use crate::i18n::{tr, trf};

/// Answer of the password prompt.
pub(crate) struct PasswordAnswer {
    /// Password typed by the user.
    pub password: Secret,
    /// Whether to save it in the keyring.
    pub remember: bool,
}

/// Asks for a secret; `on_answer` runs if the user confirms.
fn ask_secret(
    parent: &impl IsA<gtk::Window>,
    heading: &str,
    body: &str,
    show_remember: bool,
    on_answer: impl Fn(PasswordAnswer) + 'static,
) {
    let dialog = adw::MessageDialog::new(Some(parent), Some(heading), Some(body));

    let entry = gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .activates_default(true)
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(heading)]);
    let remember = gtk::CheckButton::with_mnemonic(&tr("_Remember in the system keyring"));
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&entry);
    if show_remember {
        content.append(&remember);
    }
    dialog.set_extra_child(Some(&content));

    dialog.add_responses(&[("cancel", &tr("_Cancel")), ("connect", &tr("C_onnect"))]);
    dialog.set_response_appearance("connect", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("connect"));
    dialog.set_close_response("cancel");
    dialog.connect_response(None, move |_, response| {
        if response == "connect" {
            on_answer(PasswordAnswer {
                password: Secret::new(entry.text().as_str()),
                remember: remember.is_active(),
            });
        }
    });
    dialog.present();
}

fn with_error(mut body: String, previous_error: Option<&str>) -> String {
    if let Some(error) = previous_error {
        body.push_str("\n\n");
        body.push_str(error);
    }
    body
}

/// Asks for the database password of `cfg`.
pub(crate) fn ask_password(
    parent: &impl IsA<gtk::Window>,
    cfg: &ConnectionConfig,
    previous_error: Option<&str>,
    can_remember: bool,
    on_answer: impl Fn(PasswordAnswer) + 'static,
) {
    let body = with_error(
        trf(
            "Enter the password of “{user}” for {connection}.",
            &[("user", &cfg.user), ("connection", &cfg.name)],
        ),
        previous_error,
    );
    ask_secret(
        parent,
        &tr("Password Required"),
        &body,
        can_remember && cfg.password_policy == PasswordPolicy::Keyring,
        on_answer,
    );
}

/// Asks for the SSH password or key passphrase of `cfg`.
pub(crate) fn ask_ssh_secret(
    parent: &impl IsA<gtk::Window>,
    cfg: &ConnectionConfig,
    previous_error: Option<&str>,
    can_remember: bool,
    on_answer: impl Fn(PasswordAnswer) + 'static,
) {
    let Some(ssh) = &cfg.ssh else {
        return;
    };
    let (heading, body) = match &ssh.auth {
        SshAuth::PrivateKey { path } => (
            tr("SSH Key Passphrase"),
            trf(
                "Enter the passphrase of the key {key} for {connection}.",
                &[
                    ("key", &path.display().to_string()),
                    ("connection", &cfg.name),
                ],
            ),
        ),
        _ => (
            tr("SSH Password"),
            trf(
                "Enter the SSH password of “{target}” for {connection}.",
                &[("target", &ssh.summary()), ("connection", &cfg.name)],
            ),
        ),
    };
    ask_secret(
        parent,
        &heading,
        &with_error(body, previous_error),
        can_remember && ssh.secret_policy == PasswordPolicy::Keyring,
        on_answer,
    );
}

/// Asks whether to trust an unknown SSH server key (trust on first use).
pub(crate) fn confirm_host_key(
    parent: &impl IsA<gtk::Window>,
    host: &str,
    port: u16,
    fingerprint: &str,
    on_trust: impl Fn() + 'static,
) {
    let target = if port == 22 {
        host.to_owned()
    } else {
        format!("{host}:{port}")
    };
    let dialog = adw::MessageDialog::new(
        Some(parent),
        Some(&tr("Unknown SSH Server")),
        Some(&trf(
            "Strata has never connected to {server}. Check with its administrator that its key fingerprint is the following before trusting it:",
            &[("server", &target)],
        )),
    );
    let label = gtk::Label::builder()
        .label(fingerprint)
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::Char)
        .build();
    label.add_css_class("monospace");
    dialog.set_extra_child(Some(&label));
    dialog.add_responses(&[
        ("cancel", &tr("_Cancel")),
        ("trust", &tr("_Trust and Continue")),
    ]);
    dialog.set_response_appearance("trust", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.connect_response(None, move |_, response| {
        if response == "trust" {
            on_trust();
        }
    });
    dialog.present();
}

/// Asks for confirmation before deleting `cfg`.
pub(crate) fn confirm_delete(
    parent: &impl IsA<gtk::Window>,
    cfg: &ConnectionConfig,
    on_confirm: impl Fn() + 'static,
) {
    let dialog = adw::MessageDialog::new(
        Some(parent),
        Some(&trf("Delete “{name}”?", &[("name", &cfg.name)])),
        Some(&tr(
            "The connection settings and its saved password will be deleted. Your databases are not affected.",
        )),
    );
    dialog.add_responses(&[("cancel", &tr("_Cancel")), ("delete", &tr("_Delete"))]);
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.connect_response(None, move |_, response| {
        if response == "delete" {
            on_confirm();
        }
    });
    dialog.present();
}

fn source_label(source: &ImportSource) -> String {
    match source {
        ImportSource::PgService => tr("PostgreSQL service file"),
        ImportSource::PgPass => "~/.pgpass".to_owned(),
        ImportSource::MyCnf => "~/.my.cnf".to_owned(),
    }
}

/// Whether `candidate` matches an already saved connection.
fn already_saved(candidate: &ConnectionConfig, existing: &[ConnectionConfig]) -> bool {
    existing.iter().any(|c| {
        c.kind == candidate.kind
            && c.endpoint == candidate.endpoint
            && c.user == candidate.user
            && c.database == candidate.database
    })
}

/// Lets the user pick connections to import (ADR-0014); `on_import`
/// receives the selected ones.
pub(crate) fn import(
    parent: &impl IsA<gtk::Window>,
    candidates: Vec<ImportCandidate>,
    warnings: &[String],
    existing: &[ConnectionConfig],
    on_import: impl Fn(Vec<ConnectionConfig>) + 'static,
) {
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title(tr("Connections Found"))
        .description(tr(
            "Read from ~/.pgpass, pg_service.conf and ~/.my.cnf. Passwords stay in these files and are read at each connection.",
        ))
        .build();
    page.add(&group);

    let selection: Rc<RefCell<Vec<(gtk::CheckButton, ConnectionConfig)>>> = Rc::default();
    if candidates.is_empty() {
        group.add(
            &adw::ActionRow::builder()
                .title(tr("No connection found"))
                .build(),
        );
    }
    for candidate in candidates {
        let saved = already_saved(&candidate.config, existing);
        let check = gtk::CheckButton::builder()
            .active(!saved)
            .valign(gtk::Align::Center)
            .build();
        let mut subtitle = format!(
            "{} — {}",
            candidate.config.summary(),
            source_label(&candidate.source)
        );
        if saved {
            subtitle.push_str(" — ");
            subtitle.push_str(&tr("already saved"));
        }
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&candidate.config.name))
            .subtitle(glib::markup_escape_text(&subtitle))
            .activatable_widget(&check)
            .build();
        row.add_prefix(&check);
        group.add(&row);
        selection.borrow_mut().push((check, candidate.config));
    }

    if !warnings.is_empty() {
        let warn_group = adw::PreferencesGroup::builder()
            .title(tr("Ignored Files"))
            .build();
        for warning in warnings {
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(warning))
                .title_lines(0)
                .build();
            let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
            icon.add_css_class("warning");
            row.add_prefix(&icon);
            warn_group.add(&row);
        }
        page.add(&warn_group);
    }

    let cancel = gtk::Button::with_mnemonic(&tr("_Cancel"));
    let import = gtk::Button::with_mnemonic(&tr("_Import"));
    import.add_css_class("suggested-action");
    import.set_sensitive(!selection.borrow().is_empty());
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    header.pack_start(&cancel);
    header.pack_end(&import);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&page));

    let window = adw::Window::builder()
        .title(tr("Import Connections"))
        .modal(true)
        .transient_for(parent)
        .default_width(520)
        .default_height(560)
        .content(&toolbar)
        .build();

    cancel.connect_clicked(glib::clone!(
        #[weak]
        window,
        move |_| window.close()
    ));
    import.connect_clicked(glib::clone!(
        #[weak]
        window,
        move |_| {
            let chosen: Vec<ConnectionConfig> = selection
                .borrow()
                .iter()
                .filter(|(check, _)| check.is_active())
                .map(|(_, cfg)| cfg.clone())
                .collect();
            on_import(chosen);
            window.close();
        }
    ));
    window.present();
}
