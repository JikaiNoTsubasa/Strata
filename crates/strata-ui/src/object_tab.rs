// SPDX-License-Identifier: GPL-3.0-or-later
//! Tab showing an object's properties (NAV-6) and DDL (NAV-5).

use adw::prelude::*;
use gtk::glib;
use sourceview5::prelude::*;
use strata_core::{ConnectionId, ConstraintKind, ErrorReport, ObjectKind, ObjectRef, TableDetails};

use crate::i18n::{tr, trf};
use crate::messages;
use crate::tree::object_icon;

/// Content of an object tab.
#[derive(Debug, Clone)]
pub(crate) struct ObjectTab {
    pub(crate) connection: ConnectionId,
    pub(crate) object: ObjectRef,
    root: adw::ToolbarView,
    properties: adw::Bin,
    ddl_buffer: sourceview5::Buffer,
    ddl_stack: gtk::Stack,
    ddl_error: adw::StatusPage,
    pub(crate) copy_button: gtk::Button,
    pub(crate) refresh_button: gtk::Button,
}

fn spinner_page() -> gtk::Widget {
    let spinner = gtk::Spinner::builder()
        .spinning(true)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .width_request(32)
        .height_request(32)
        .build();
    spinner.upcast()
}

impl ObjectTab {
    /// Builds an empty tab; content arrives with [`Self::set_details`] and
    /// [`Self::set_ddl`].
    pub(crate) fn new(connection: ConnectionId, object: ObjectRef) -> Self {
        let properties = adw::Bin::new();
        properties.set_child(Some(&spinner_page()));

        let ddl_buffer = sourceview5::Buffer::new(None);
        if let Some(language) = sourceview5::LanguageManager::default().language("sql") {
            ddl_buffer.set_language(Some(&language));
        }
        let style = adw::StyleManager::default();
        let apply_scheme = glib::clone!(
            #[weak]
            ddl_buffer,
            move |style: &adw::StyleManager| {
                let name = if style.is_dark() {
                    "Adwaita-dark"
                } else {
                    "Adwaita"
                };
                if let Some(scheme) = sourceview5::StyleSchemeManager::default().scheme(name) {
                    ddl_buffer.set_style_scheme(Some(&scheme));
                }
            }
        );
        apply_scheme(&style);
        style.connect_dark_notify(apply_scheme);

        let view = sourceview5::View::builder()
            .buffer(&ddl_buffer)
            .editable(false)
            .monospace(true)
            .show_line_numbers(true)
            .left_margin(8)
            .top_margin(8)
            .build();
        view.upcast_ref::<gtk::TextView>()
            .update_property(&[gtk::accessible::Property::Label(&tr("DDL"))]);
        let scrolled = gtk::ScrolledWindow::builder().child(&view).build();
        let ddl_error = adw::StatusPage::builder()
            .icon_name("dialog-warning-symbolic")
            .build();
        let ddl_stack = gtk::Stack::new();
        ddl_stack.add_named(&spinner_page(), Some("loading"));
        ddl_stack.add_named(&scrolled, Some("ddl"));
        ddl_stack.add_named(&ddl_error, Some("error"));

        let stack = adw::ViewStack::new();
        stack.add_titled_with_icon(
            &properties,
            Some("properties"),
            &tr("Properties"),
            "document-properties-symbolic",
        );
        stack.add_titled_with_icon(
            &ddl_stack,
            Some("ddl"),
            &tr("DDL"),
            "format-justify-left-symbolic",
        );

        let switcher = adw::ViewSwitcher::builder()
            .stack(&stack)
            .policy(adw::ViewSwitcherPolicy::Wide)
            .build();
        let copy_button = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .tooltip_text(tr("Copy DDL"))
            .build();
        let refresh_button = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text(tr("Refresh"))
            .build();
        let bar = gtk::CenterBox::builder()
            .margin_start(6)
            .margin_end(6)
            .margin_top(6)
            .margin_bottom(6)
            .center_widget(&switcher)
            .build();
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        buttons.append(&copy_button);
        buttons.append(&refresh_button);
        bar.set_end_widget(Some(&buttons));

        let root = adw::ToolbarView::new();
        root.add_top_bar(&bar);
        root.set_content(Some(&stack));

        Self {
            connection,
            object,
            root,
            properties,
            ddl_buffer,
            ddl_stack,
            ddl_error,
            copy_button,
            refresh_button,
        }
    }

    /// The tab's widget.
    pub(crate) fn widget(&self) -> &adw::ToolbarView {
        &self.root
    }

    /// Tab title.
    pub(crate) fn title(&self) -> String {
        match &self.object.signature {
            Some(sig) => format!("{}({sig})", self.object.name),
            None => self.object.name.clone(),
        }
    }

    /// Tab icon.
    pub(crate) fn icon(&self) -> gtk::gio::ThemedIcon {
        gtk::gio::ThemedIcon::new(object_icon(self.object.kind))
    }

    /// Shows the loading state again before a refresh.
    pub(crate) fn set_loading(&self) {
        self.properties.set_child(Some(&spinner_page()));
        self.ddl_stack.set_visible_child_name("loading");
    }

    /// DDL currently shown.
    pub(crate) fn ddl(&self) -> Option<String> {
        (self.ddl_stack.visible_child_name().as_deref() == Some("ddl")).then(|| {
            let (start, end) = self.ddl_buffer.bounds();
            self.ddl_buffer.text(&start, &end, false).to_string()
        })
    }

    /// Displays the DDL, or the failure.
    pub(crate) fn set_ddl(&self, result: &Result<String, ErrorReport>) {
        match result {
            Ok(ddl) => {
                self.ddl_buffer.set_text(ddl);
                self.ddl_stack.set_visible_child_name("ddl");
            }
            Err(error) => {
                self.ddl_error
                    .set_title(&messages::error_summary(error.kind));
                self.ddl_error
                    .set_description(Some(&glib::markup_escape_text(&error.detail)));
                self.ddl_stack.set_visible_child_name("error");
            }
        }
    }

    /// Properties of objects without columns (sequences, functions…).
    pub(crate) fn set_general(&self) {
        let page = adw::PreferencesPage::new();
        page.add(&self.general_group(None));
        self.properties.set_child(Some(&page));
    }

    /// Displays the description of a table or view, or the failure.
    pub(crate) fn set_details(&self, result: &Result<TableDetails, ErrorReport>) {
        let details = match result {
            Ok(details) => details,
            Err(error) => {
                let status = adw::StatusPage::builder()
                    .icon_name("dialog-warning-symbolic")
                    .title(messages::error_summary(error.kind))
                    .description(glib::markup_escape_text(&error.detail))
                    .build();
                self.properties.set_child(Some(&status));
                return;
            }
        };
        let page = adw::PreferencesPage::new();
        page.add(&self.general_group(Some(details)));

        let columns = group(&tr("Columns"));
        for column in &details.columns {
            let mut parts = vec![column.data_type.clone()];
            if !column.nullable {
                parts.push("NOT NULL".to_owned());
            }
            if let Some(default) = &column.default {
                if default.starts_with("GENERATED") {
                    parts.push(default.clone());
                } else {
                    parts.push(format!("DEFAULT {default}"));
                }
            }
            if let Some(comment) = &column.comment {
                parts.push(comment.clone());
            }
            let row = row(&column.name, &parts.join(" · "));
            if column.primary_key {
                let key = gtk::Label::new(Some(&tr("Primary key")));
                key.add_css_class("dim-label");
                key.add_css_class("caption");
                row.add_suffix(&key);
            }
            columns.add(&row);
        }
        page.add(&columns);

        if !details.constraints.is_empty() {
            let constraints = group(&tr("Keys and Constraints"));
            for c in &details.constraints {
                let row = row(&c.name, &c.definition);
                let kind = match c.kind {
                    ConstraintKind::PrimaryKey => tr("Primary key"),
                    ConstraintKind::ForeignKey => tr("Foreign key"),
                    ConstraintKind::Unique => tr("Unique"),
                    ConstraintKind::Check => tr("Check"),
                    ConstraintKind::Exclusion => tr("Exclusion"),
                    ConstraintKind::Other => tr("Other"),
                };
                let label = gtk::Label::new(Some(&kind));
                label.add_css_class("dim-label");
                label.add_css_class("caption");
                row.add_suffix(&label);
                constraints.add(&row);
            }
            page.add(&constraints);
        }
        if !details.indexes.is_empty() {
            let indexes = group(&tr("Indexes"));
            for index in &details.indexes {
                indexes.add(&row(&index.name, &index.definition));
            }
            page.add(&indexes);
        }
        if !details.triggers.is_empty() {
            let triggers = group(&tr("Triggers"));
            for trigger in &details.triggers {
                triggers.add(&row(&trigger.name, &trigger.definition));
            }
            page.add(&triggers);
        }
        self.properties.set_child(Some(&page));
    }

    fn general_group(&self, details: Option<&TableDetails>) -> adw::PreferencesGroup {
        let o = &self.object;
        let group = adw::PreferencesGroup::builder()
            .title(glib::markup_escape_text(&self.title()))
            .build();
        group.add(&row(&tr("Type"), &kind_label(o.kind)));
        group.add(&row(&tr("Schema"), &format!("{}.{}", o.database, o.schema)));
        if let Some(signature) = &o.signature {
            group.add(&row(&tr("Arguments"), signature));
        }
        if let Some(d) = details {
            if let Some(rows) = d.estimated_rows {
                group.add(&row(
                    &tr("Estimated rows"),
                    &trf("about {count}", &[("count", &group_digits(rows))]),
                ));
            }
            if let Some(size) = d.total_size.and_then(|s| u64::try_from(s).ok()) {
                group.add(&row(&tr("Size on disk"), &glib::format_size(size)));
            }
            if let Some(comment) = &d.comment {
                group.add(&row(&tr("Comment"), comment));
            }
        }
        group
    }
}

fn group(title: &str) -> adw::PreferencesGroup {
    adw::PreferencesGroup::builder().title(title).build()
}

fn row(title: &str, subtitle: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(title))
        .subtitle(glib::markup_escape_text(subtitle))
        .subtitle_selectable(true)
        .build();
    row.add_css_class("property");
    row
}

/// `1234567` → `1 234 567` (thin spaces, locale-neutral).
fn group_digits(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{202f}');
        }
        out.push(c);
    }
    if n < 0 {
        out.insert(0, '-');
    }
    out
}

/// Translated name of an object kind.
pub(crate) fn kind_label(kind: ObjectKind) -> String {
    match kind {
        ObjectKind::Table => tr("Table"),
        ObjectKind::View => tr("View"),
        ObjectKind::MaterializedView => tr("Materialized view"),
        ObjectKind::Sequence => tr("Sequence"),
        ObjectKind::Function => tr("Function"),
        ObjectKind::Procedure => tr("Procedure"),
    }
}

#[cfg(test)]
mod tests {
    use super::group_digits;

    #[test]
    fn digits_are_grouped() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1234567), "1\u{202f}234\u{202f}567");
        assert_eq!(group_digits(-1000), "-1\u{202f}000");
    }
}

/// Smoke test of the tab widgets; needs a display:
/// `STRATA_SNAPSHOT_DIR=/tmp cargo test -p strata-ui -- --ignored`.
#[cfg(test)]
mod display_tests {
    use strata_core::{ColumnInfo, ConstraintInfo, IndexInfo, Reference, TriggerInfo};

    use super::*;

    fn details(object: &ObjectRef) -> TableDetails {
        TableDetails {
            object: object.clone(),
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
                    name: "customer_id".into(),
                    data_type: "bigint".into(),
                    nullable: false,
                    default: None,
                    comment: Some("Buyer".into()),
                    primary_key: false,
                },
            ],
            indexes: vec![IndexInfo {
                name: "order_customer_idx".into(),
                definition:
                    "CREATE INDEX order_customer_idx ON shop.\"order\" USING btree (customer_id)"
                        .into(),
                unique: false,
                primary: false,
                constraint_backed: false,
            }],
            constraints: vec![ConstraintInfo {
                name: "order_customer_id_fkey".into(),
                kind: ConstraintKind::ForeignKey,
                columns: vec!["customer_id".into()],
                references: Some(Reference {
                    schema: "shop".into(),
                    table: "customer".into(),
                    columns: vec!["id".into()],
                }),
                definition: "FOREIGN KEY (customer_id) REFERENCES shop.customer(id)".into(),
            }],
            triggers: vec![TriggerInfo {
                name: "order_touch".into(),
                definition: "CREATE TRIGGER order_touch BEFORE UPDATE ON shop.\"order\"".into(),
            }],
            comment: Some("Orders".into()),
            estimated_rows: Some(1_234_567),
            total_size: Some(52_428_800),
        }
    }

    #[test]
    #[ignore = "needs a display"]
    fn renders_properties_and_ddl() {
        gtk::init().expect("display");
        sourceview5::init();
        let object = ObjectRef {
            database: "postgres".into(),
            schema: "shop".into(),
            name: "order".into(),
            kind: ObjectKind::Table,
            signature: None,
        };
        let tab = ObjectTab::new(ConnectionId::new(), object.clone());
        tab.set_details(&Ok(details(&object)));
        tab.set_ddl(&Ok(
            "CREATE TABLE shop.\"order\" (\n    id bigint NOT NULL\n);\n".into(),
        ));
        assert!(tab.ddl().is_some_and(|d| d.starts_with("CREATE TABLE")));

        let window = gtk::Window::builder()
            .default_width(800)
            .default_height(900)
            .child(tab.widget())
            .build();
        window.present();
        let context = glib::MainContext::default();
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_millis(800) {
            context.iteration(false);
        }
        if let Some(dir) = std::env::var_os("STRATA_SNAPSHOT_DIR") {
            let paintable = gtk::WidgetPaintable::new(Some(tab.widget()));
            let snapshot = gtk::Snapshot::new();
            let (w, h) = (
                f64::from(tab.widget().width()),
                f64::from(tab.widget().height()),
            );
            paintable.snapshot(&snapshot, w, h);
            let renderer = window.renderer().expect("renderer");
            if let Some(node) = snapshot.to_node() {
                let texture = renderer.render_texture(node, None);
                texture
                    .save_to_png(std::path::Path::new(&dir).join("object-tab.png"))
                    .expect("png");
            }
        }
        // The error path renders too.
        tab.set_ddl(&Err(ErrorReport::new(
            strata_core::ErrorKind::NotFound,
            "gone",
        )));
        assert!(tab.ddl().is_none());
    }
}
