// SPDX-License-Identifier: GPL-3.0-or-later
//! Main application window (single window, see ADR-0015).

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};

use crate::settings;

mod imp {
    use std::cell::OnceCell;

    use super::*;

    /// Private state of [`super::StrataWindow`].
    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/fr/coregeek/Strata/ui/window.ui")]
    pub struct StrataWindow {
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        pub settings: OnceCell<gio::Settings>,
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
    /// Creates the main window for `app`.
    pub fn new(app: &adw::Application) -> Self {
        glib::Object::builder().property("application", app).build()
    }

    /// Shows a transient notification.
    pub fn toast(&self, message: &str) {
        self.imp().toast_overlay.add_toast(adw::Toast::new(message));
    }
}
