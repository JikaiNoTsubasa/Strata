// SPDX-License-Identifier: GPL-3.0-or-later
//! Gettext initialisation (see `docs/ADR/0007-i18n.md`).

use gettextrs::{bind_textdomain_codeset, bindtextdomain, textdomain};

use crate::config::{DEV_LOCALE_DIR, GETTEXT_PACKAGE, INSTALLED_LOCALE_DIR};

/// Binds the Strata translation domain.
///
/// Must run before any translatable string (including GtkBuilder templates)
/// is loaded. The locale itself is selected by GTK during its initialisation
/// (`setlocale(LC_ALL, "")`), which happens before any string is translated.
pub(crate) fn init() {
    let dir = INSTALLED_LOCALE_DIR.unwrap_or(DEV_LOCALE_DIR);
    let result = bindtextdomain(GETTEXT_PACKAGE, dir)
        .and_then(|_| bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8"))
        .and_then(|_| textdomain(GETTEXT_PACKAGE));
    if let Err(err) = result {
        tracing::warn!(%err, "translations unavailable");
    }
}

/// Translates `msgid` in the Strata domain.
pub(crate) fn tr(msgid: &str) -> String {
    gettextrs::gettext(msgid)
}

/// Translates `msgid`, then replaces each `{key}` with its value.
///
/// Named placeholders let translators reorder them freely.
pub(crate) fn trf(msgid: &str, args: &[(&str, &str)]) -> String {
    let mut text = tr(msgid);
    for (key, value) in args {
        text = text.replace(&format!("{{{key}}}"), value);
    }
    text
}
