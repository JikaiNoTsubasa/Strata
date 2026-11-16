// SPDX-License-Identifier: GPL-3.0-or-later
//! Developer tasks, run with `cargo xtask <task>`.
//!
//! * `bench-mem` — memory/start-up benchmark (CDC §5, §12; ADR-0013);
//! * `i18n` — regenerates `po/strata.pot` and merges it into every `.po`.

mod bench;
mod i18n;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};

const USAGE: &str = "\
Usage: cargo xtask <task> [options]

Tasks:
  bench-mem   Measure start-up time and memory (PSS) of benchmark scenarios
      --scenario <name>     Scenario to run (default: idle; available: idle)
      --profile <name>      Cargo profile to build (default: release)
      --display <mode>      headless | session (default: session if a Wayland
                            session is available, headless otherwise)
      --renderer <name>     Force GSK_RENDERER (e.g. ngl, vulkan, cairo)
      --mimalloc            Build with the mimalloc allocator (ADR-0005)
      --runs <n>            Number of runs; the median is kept (default: 3)
      --update-baseline     Store the result as the new reference
  i18n        Regenerate po/strata.pot and update the translations
";

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<ExitCode> {
    let mut args = std::env::args().skip(1);
    let task = args.next();
    let rest: Vec<String> = args.collect();
    match task.as_deref() {
        Some("bench-mem") => bench::run(&workspace_root()?, &rest),
        Some("i18n") => i18n::run(&workspace_root()?).map(|()| ExitCode::SUCCESS),
        Some("-h" | "--help") | None => {
            print!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Some(other) => bail!("unknown task `{other}`\n\n{USAGE}"),
    }
}

/// Root of the Cargo workspace (parent of the `xtask` directory).
fn workspace_root() -> anyhow::Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("cannot locate the workspace root")
}
