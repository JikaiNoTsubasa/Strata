// SPDX-License-Identifier: GPL-3.0-or-later
//! Application object: actions, shortcuts and the engine event loop.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use strata_engine::{EngineHandle, UiEvent};

use crate::bench;
use crate::config::{APP_ID, RESOURCE_BASE_PATH, VERSION};
use crate::i18n::tr;
use crate::window::StrataWindow;

/// Routes engine events to the main window, queuing them until it exists.
#[derive(Default)]
struct EventRouter {
    window: glib::WeakRef<StrataWindow>,
    pending: VecDeque<UiEvent>,
}

impl EventRouter {
    fn dispatch(&mut self, event: UiEvent) {
        match self.window.upgrade() {
            Some(window) => window.handle_event(event),
            None => self.pending.push_back(event),
        }
    }

    fn attach(&mut self, window: &StrataWindow) {
        self.window.set(Some(window));
        while let Some(event) = self.pending.pop_front() {
            window.handle_event(event);
        }
    }
}

/// Creates the application and wires it to `engine`.
pub(crate) fn build(engine: EngineHandle) -> adw::Application {
    let scenario = bench::scenario();
    let flags = if scenario.is_some() {
        // A benchmark must measure its own process, never forward to a
        // running instance.
        gio::ApplicationFlags::NON_UNIQUE
    } else {
        gio::ApplicationFlags::empty()
    };
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .resource_base_path(RESOURCE_BASE_PATH)
        .flags(flags)
        .build();

    let router = Rc::new(RefCell::new(EventRouter::default()));

    app.connect_startup(glib::clone!(
        #[strong]
        engine,
        #[strong]
        router,
        move |app| startup(app, &engine, &router)
    ));
    app.connect_activate(glib::clone!(
        #[strong]
        engine,
        move |app| activate(app, &engine, &router, scenario)
    ));
    app.connect_shutdown(move |_| {
        engine.send(strata_engine::Command::Shutdown);
    });
    app
}

fn startup(app: &adw::Application, engine: &EngineHandle, router: &Rc<RefCell<EventRouter>>) {
    sourceview5::init();
    gtk::Window::set_default_icon_name(APP_ID);
    if !crate::config::is_installed() {
        add_source_tree_icons();
    }

    install_actions(app);
    spawn_event_loop(engine, Rc::clone(router));
}

fn activate(
    app: &adw::Application,
    engine: &EngineHandle,
    router: &Rc<RefCell<EventRouter>>,
    scenario: Option<bench::Scenario>,
) {
    // Single window (ADR-0015): re-activating presents the existing one.
    if let Some(window) = main_window(app) {
        window.present();
        return;
    }
    let window = StrataWindow::new(app, engine.clone());
    router.borrow_mut().attach(&window);
    if let Some(scenario) = scenario {
        bench::run(scenario, window.upcast_ref());
    }
    window.present();
}

fn main_window(app: &adw::Application) -> Option<StrataWindow> {
    app.windows()
        .into_iter()
        .find_map(|w| w.downcast::<StrataWindow>().ok())
}

fn install_actions(app: &adw::Application) {
    let quit = gio::ActionEntry::builder("quit")
        .activate(|app: &adw::Application, _, _| app.quit())
        .build();
    let about = gio::ActionEntry::builder("about")
        .activate(|app: &adw::Application, _, _| show_about(app))
        .build();
    let new_connection = gio::ActionEntry::builder("new-connection")
        .activate(|app: &adw::Application, _, _| {
            if let Some(window) = main_window(app) {
                window.open_connection_dialog(None);
            }
        })
        .build();
    let import = gio::ActionEntry::builder("import-connections")
        .activate(|app: &adw::Application, _, _| {
            if let Some(window) = main_window(app) {
                window.import_connections();
            }
        })
        .build();
    app.add_action_entries([quit, about, new_connection, import]);

    app.set_accels_for_action("app.quit", &["<Control>q"]);
    app.set_accels_for_action("app.new-connection", &["<Control>n"]);
}

fn show_about(app: &adw::Application) {
    let about = adw::AboutWindow::builder()
        .application_name("Strata")
        .application_icon(APP_ID)
        .version(VERSION)
        .comments(tr("A lightweight native database client"))
        .website("https://github.com/JikaiNoTsubasa/Strata")
        .issue_url("https://github.com/JikaiNoTsubasa/Strata/issues")
        .license_type(gtk::License::Gpl30)
        .translator_credits(tr("translator-credits"))
        .modal(true)
        .build();
    if let Some(window) = main_window(app) {
        about.set_transient_for(Some(&window));
    }
    about.present();
}

/// Consumes engine events on the GLib main context.
fn spawn_event_loop(engine: &EngineHandle, router: Rc<RefCell<EventRouter>>) {
    let events = engine.events();
    glib::spawn_future_local(async move {
        while let Ok(event) = events.recv().await {
            router.borrow_mut().dispatch(event);
        }
    });
}

/// Makes the icons of `data/icons` available when running from the source
/// tree (installed builds get them from the system icon theme).
fn add_source_tree_icons() {
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::IconTheme::for_display(&display)
            .add_search_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/icons"));
    }
}
