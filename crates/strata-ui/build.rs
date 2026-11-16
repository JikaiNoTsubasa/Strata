// SPDX-License-Identifier: GPL-3.0-or-later
//! Build script: compiles GResources, and — for running from the source tree —
//! the GSettings schema and the gettext catalogs into `OUT_DIR`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const APP_ID: &str = "fr.coregeek.Strata";
const GETTEXT_PACKAGE: &str = "strata";

fn main() {
    glib_build_tools::compile_resources(
        &["resources"],
        "resources/resources.gresource.xml",
        "strata.gresource",
    );

    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = manifest.join("../..");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap_or_default());

    compile_schema(&root.join("data"), &out.join("schemas"));
    compile_translations(&root.join("po"), &out.join("locale"));
}

/// Compiles `data/<APP_ID>.gschema.xml` so that a non-installed build can
/// load its settings (see `docs/ADR/0006-preferences-storage.md`).
fn compile_schema(data: &Path, dest: &Path) {
    let schema = data.join(format!("{APP_ID}.gschema.xml"));
    println!("cargo:rerun-if-changed={}", schema.display());
    let _ = fs::create_dir_all(dest);
    let _ = fs::copy(&schema, dest.join(format!("{APP_ID}.gschema.xml")));
    let status = Command::new("glib-compile-schemas")
        .arg("--strict")
        .arg(dest)
        .status();
    match status {
        Ok(s) if s.success() => {}
        _ => println!("cargo:warning=glib-compile-schemas failed; settings will not persist"),
    }
    println!("cargo:rustc-env=STRATA_DEV_SCHEMA_DIR={}", dest.display());
}

/// Compiles every `po/<lang>.po` listed in `po/LINGUAS` into
/// `<dest>/<lang>/LC_MESSAGES/strata.mo`.
fn compile_translations(po: &Path, dest: &Path) {
    let linguas = po.join("LINGUAS");
    println!("cargo:rerun-if-changed={}", linguas.display());
    let langs = fs::read_to_string(&linguas).unwrap_or_default();
    for lang in langs
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let src = po.join(format!("{lang}.po"));
        println!("cargo:rerun-if-changed={}", src.display());
        let dir = dest.join(lang).join("LC_MESSAGES");
        let _ = fs::create_dir_all(&dir);
        let status = Command::new("msgfmt")
            .arg("--check")
            .arg("-o")
            .arg(dir.join(format!("{GETTEXT_PACKAGE}.mo")))
            .arg(&src)
            .status();
        match status {
            Ok(s) if s.success() => {}
            _ => println!("cargo:warning=msgfmt failed for {lang}; translation unavailable"),
        }
    }
    println!("cargo:rustc-env=STRATA_DEV_LOCALE_DIR={}", dest.display());
}
