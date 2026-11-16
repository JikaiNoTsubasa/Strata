// SPDX-License-Identifier: GPL-3.0-or-later
//! Access to the application's GSettings (see `docs/ADR/0006-preferences-storage.md`).

use gtk::gio;

use crate::config::{APP_ID, DEV_SCHEMA_DIR};

/// Window width key.
pub(crate) const WINDOW_WIDTH: &str = "window-width";
/// Window height key.
pub(crate) const WINDOW_HEIGHT: &str = "window-height";
/// Window maximized state key.
pub(crate) const WINDOW_MAXIMIZED: &str = "window-maximized";
/// Minutes after which idle sessions are closed, 0 to never close (CON-11).
pub(crate) const IDLE_TIMEOUT_MINUTES: &str = "idle-timeout-minutes";

/// Opens the application settings.
///
/// Looks up the installed schema first, then the schema compiled into the
/// build directory. Returns `None` (settings not persisted) if neither exists.
pub(crate) fn open() -> Option<gio::Settings> {
    let default = gio::SettingsSchemaSource::default();
    let schema = default
        .as_ref()
        .and_then(|source| source.lookup(APP_ID, true))
        .or_else(|| {
            gio::SettingsSchemaSource::from_directory(DEV_SCHEMA_DIR, default.as_ref(), false)
                .ok()
                .and_then(|source| source.lookup(APP_ID, false))
        });
    match schema {
        Some(schema) => Some(gio::Settings::new_full(
            &schema,
            None::<&gio::SettingsBackend>,
            None,
        )),
        None => {
            tracing::warn!("GSettings schema not found; preferences will not be saved");
            None
        }
    }
}
