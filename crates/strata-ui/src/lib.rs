// SPDX-License-Identifier: GPL-3.0-or-later
//! Strata user interface (GTK4 + libadwaita).
//!
//! This is the only crate allowed to depend on GTK (see
//! `docs/ARCHITECTURE.md`). It renders state and forwards user intents to the
//! engine as [`strata_engine::Command`]s.

mod application;
mod bench;
pub mod config;
mod i18n;
mod settings;
mod window;

use std::process::ExitCode;

use strata_engine::EngineHandle;

/// Runs the GTK application until its last window is closed.
///
/// Must be called from the main thread; `engine` must already be running.
pub fn run(engine: EngineHandle) -> ExitCode {
    i18n::init();
    gtk::gio::resources_register_include!("strata.gresource")
        .unwrap_or_else(|err| tracing::error!(%err, "failed to register resources"));

    let app = application::build(engine);
    let code = gtk::prelude::ApplicationExtManual::run(&app);
    if code == gtk::glib::ExitCode::SUCCESS {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
