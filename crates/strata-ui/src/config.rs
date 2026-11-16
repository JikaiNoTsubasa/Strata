// SPDX-License-Identifier: GPL-3.0-or-later
//! Build-time application identity (see `docs/ADR/0016-application-id.md`).

/// Application identifier (D-Bus name, GSettings schema, icons, keyring).
pub const APP_ID: &str = "fr.coregeek.Strata";

/// Base path of the application's GResources.
pub const RESOURCE_BASE_PATH: &str = "/fr/coregeek/Strata";

/// Gettext translation domain.
pub const GETTEXT_PACKAGE: &str = "strata";

/// Application version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Installed locale directory, provided by packaging builds (Meson/Flatpak).
pub(crate) const INSTALLED_LOCALE_DIR: Option<&str> = option_env!("STRATA_LOCALEDIR");

/// Locale catalogs compiled by the build script, used when not installed.
pub(crate) const DEV_LOCALE_DIR: &str = env!("STRATA_DEV_LOCALE_DIR");

/// GSettings schema compiled by the build script, used when not installed.
pub(crate) const DEV_SCHEMA_DIR: &str = env!("STRATA_DEV_SCHEMA_DIR");

/// Whether this build is installed (packaging provides the data paths), as
/// opposed to running from the source tree.
pub(crate) fn is_installed() -> bool {
    INSTALLED_LOCALE_DIR.is_some()
}
