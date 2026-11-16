// SPDX-License-Identifier: GPL-3.0-or-later
//! Application object: actions, shortcuts and the engine event loop.

use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use strata_engine::{Command, EngineHandle, UiEvent};

use crate::bench;
use crate::config::{APP_ID, RESOURCE_BASE_PATH, VERSION};
use crate::i18n::tr;
use crate::window::StrataWindow;

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

    let engine = Rc::new(engine);

    app.connect_startup({
        let engine = Rc::clone(&engine);
        move |app| startup(app, &engine)
    });
    app.connect_activate(move |app| activate(app, scenario));
    app.connect_shutdown(move |_| {
        engine.send(Command::Shutdown);
    });
    app
}

fn startup(app: &adw::Application, engine: &EngineHandle) {
    gtk::Window::set_default_icon_name(APP_ID);
    #[cfg(debug_assertions)]
    add_source_tree_icons();

    install_actions(app);
    spawn_event_loop(engine);
}

fn activate(app: &adw::Application, scenario: Option<bench::Scenario>) {
    // Single window (ADR-0015): re-activating presents the existing one.
    if let Some(window) = app.active_window() {
        window.present();
        return;
    }
    let window = StrataWindow::new(app);
    if let Some(scenario) = scenario {
        bench::run(scenario, window.upcast_ref());
    }
    window.present();
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
            // Connection management arrives in milestone J1.
            if let Some(window) = active_strata_window(app) {
                window.toast(&tr("Connection management is not available yet."));
            }
        })
        .build();
    app.add_action_entries([quit, about, new_connection]);

    app.set_accels_for_action("app.quit", &["<Control>q"]);
    app.set_accels_for_action("app.new-connection", &["<Control>n"]);
}

fn active_strata_window(app: &adw::Application) -> Option<StrataWindow> {
    app.active_window()
        .and_then(|window| window.downcast::<StrataWindow>().ok())
}

fn show_about(app: &adw::Application) {
    let about = adw::AboutWindow::builder()
        .application_name("Strata")
        .application_icon(APP_ID)
        .version(VERSION)
        .comments(tr("A lightweight native database client"))
        .license_type(gtk::License::Gpl30)
        .translator_credits(tr("translator-credits"))
        .modal(true)
        .build();
    if let Some(window) = app.active_window() {
        about.set_transient_for(Some(&window));
    }
    about.present();
}

/// Consumes engine events on the GLib main context.
fn spawn_event_loop(engine: &EngineHandle) {
    let events = engine.events();
    glib::spawn_future_local(async move {
        while let Ok(event) = events.recv().await {
            match event {
                UiEvent::Ready => tracing::debug!("engine ready"),
                other => tracing::debug!(?other, "unhandled engine event"),
            }
        }
    });
}

/// Makes the icons of `data/icons` available when running from the source tree.
#[cfg(debug_assertions)]
fn add_source_tree_icons() {
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::IconTheme::for_display(&display)
            .add_search_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/icons"));
    }
}
