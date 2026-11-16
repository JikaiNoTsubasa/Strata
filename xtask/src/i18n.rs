// SPDX-License-Identifier: GPL-3.0-or-later
//! `cargo xtask i18n`: string extraction and catalog update.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

const DOMAIN: &str = "strata";

/// Directories where distributions install gettext ITS rules.
const ITS_DIRS: &[&str] = &["/usr/share/gettext/its", "/usr/local/share/gettext/its"];

pub(crate) fn run(root: &Path) -> anyhow::Result<()> {
    let po = root.join("po");
    let pot = po.join(format!("{DOMAIN}.pot"));
    let files = read_list(&po.join("POTFILES.in"))?;

    let _ = fs::remove_file(&pot);
    // An empty template lets every later call use --join-existing.
    fs::write(&pot, "")?;
    for file in &files {
        let mut cmd = Command::new("xgettext");
        cmd.current_dir(root)
            .args([
                "--from-code=UTF-8",
                "--add-comments=Translators",
                "--join-existing",
            ])
            .args([
                "--package-name",
                DOMAIN,
                "--package-version",
                env!("CARGO_PKG_VERSION"),
            ])
            .args([
                "--keyword=tr",
                "--keyword=trf",
                "--keyword=gettext",
                "--keyword=pgettext:1c,2",
            ])
            .args(["--keyword=ngettext:1,2", "--sort-by-file"])
            .arg("-o")
            .arg(&pot);
        match file.extension().and_then(|e| e.to_str()) {
            // xgettext has no Rust parser; the C lexer handles our call sites.
            Some("rs") => cmd.arg("--language=C"),
            Some("ui") => its_or_language(&mut cmd, "gtk4builder.its", "Glade"),
            Some("in") if file.to_string_lossy().ends_with(".desktop.in") => {
                cmd.arg("--language=Desktop")
            }
            Some("in") => its_or_language(&mut cmd, "metainfo.its", "Glade"),
            _ => bail!("unsupported file type: {}", file.display()),
        };
        run_checked(cmd.arg(file), "xgettext")?;
    }

    for lang in read_list(&po.join("LINGUAS"))? {
        let po_file = po.join(lang.with_extension("po"));
        if po_file.exists() {
            let mut cmd = Command::new("msgmerge");
            cmd.args(["--update", "--backup=none", "--quiet"])
                .arg(&po_file)
                .arg(&pot);
            run_checked(&mut cmd, "msgmerge")?;
        } else {
            let mut cmd = Command::new("msginit");
            cmd.args(["--no-translator", "--input"])
                .arg(&pot)
                .arg("--output-file")
                .arg(&po_file)
                .arg("--locale")
                .arg(&lang);
            run_checked(&mut cmd, "msginit")?;
        }
    }
    println!("updated {}", pot.display());
    Ok(())
}

fn its_or_language<'a>(cmd: &'a mut Command, its: &str, language: &str) -> &'a mut Command {
    match ITS_DIRS
        .iter()
        .map(|d| Path::new(d).join(its))
        .find(|p| p.exists())
    {
        Some(path) => cmd.arg(format!("--its={}", path.display())),
        None => cmd.arg(format!("--language={language}")),
    }
}

/// Reads a gettext list file (one entry per line, `#` comments).
fn read_list(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(PathBuf::from)
        .collect())
}

fn run_checked(cmd: &mut Command, name: &str) -> anyhow::Result<()> {
    let status = cmd
        .status()
        .with_context(|| format!("cannot run {name} (is gettext installed?)"))?;
    if !status.success() {
        bail!("{name} failed with {status}");
    }
    Ok(())
}
