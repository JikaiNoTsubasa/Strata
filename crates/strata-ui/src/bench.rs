// SPDX-License-Identifier: GPL-3.0-or-later
//! Hooks for the automated memory benchmark (`cargo xtask bench-mem`).
//!
//! When `STRATA_BENCH_SCENARIO` is set, Strata runs as a non-unique instance
//! and prints [`READY_MARKER`] on stdout once the scenario has reached the
//! state to be measured.

use std::io::Write;

use gtk::glib;
use gtk::prelude::*;

/// Environment variable selecting the benchmark scenario.
const SCENARIO_VAR: &str = "STRATA_BENCH_SCENARIO";

/// Line printed on stdout when the scenario is ready to be measured.
pub(crate) const READY_MARKER: &str = "STRATA_BENCH_READY";

/// Benchmark scenarios understood by the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scenario {
    /// Application started, main window shown, no connection.
    Idle,
}

/// Returns the scenario requested through the environment, if any.
pub(crate) fn scenario() -> Option<Scenario> {
    let value = std::env::var(SCENARIO_VAR).ok()?;
    match value.as_str() {
        "idle" => Some(Scenario::Idle),
        other => {
            tracing::warn!(scenario = other, "unknown benchmark scenario");
            None
        }
    }
}

/// Drives `scenario` on `window` and signals readiness.
pub(crate) fn run(scenario: Scenario, window: &gtk::Widget) {
    match scenario {
        Scenario::Idle => {
            // Ready once the window is mapped and the main loop is idle.
            window.connect_map(|_| {
                glib::idle_add_local_once(signal_ready);
            });
        }
    }
}

fn signal_ready() {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{READY_MARKER}");
    let _ = stdout.flush();
}
