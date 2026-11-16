// SPDX-License-Identifier: GPL-3.0-or-later
//! `cargo xtask bench-mem`: start-up time and memory (PSS) benchmark.
//!
//! The application is launched with `STRATA_BENCH_SCENARIO=<name>`; it prints
//! `STRATA_BENCH_READY` once the scenario state is reached. PSS is then read
//! from `/proc/<pid>/smaps_rollup`.
//!
//! Policy (ADR-0013): the run fails on a regression of more than 10 % against
//! `bench/baseline.json`; the CDC objectives are reported but not enforced.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde_json::{Value, json};

const READY_MARKER: &str = "STRATA_BENCH_READY";
const READY_TIMEOUT: Duration = Duration::from_secs(30);
/// Time left to the process to settle (lazy loading, GC of start-up buffers).
const SETTLE: Duration = Duration::from_secs(2);
const SAMPLES: usize = 5;
const SAMPLE_INTERVAL: Duration = Duration::from_millis(400);
/// Allowed growth over the baseline before failing (10 %, CDC §5.7).
const REGRESSION_TOLERANCE: f64 = 0.10;
const BASELINE_FILE: &str = "bench/baseline.json";

/// A benchmark scenario and its CDC objective.
struct Scenario {
    name: &'static str,
    /// Objective from the CDC, in KiB (informative, see ADR-0013).
    objective_kib: u64,
}

const SCENARIOS: &[Scenario] = &[Scenario {
    name: "idle",
    // J0 exit criterion: empty window ≤ 40 MB (CDC §14).
    objective_kib: 40 * 1024,
}];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Display {
    Headless,
    Session,
}

impl Display {
    fn as_str(self) -> &'static str {
        match self {
            Self::Headless => "headless",
            Self::Session => "session",
        }
    }
}

struct Options {
    scenario: &'static Scenario,
    profile: String,
    display: Display,
    renderer: Option<String>,
    mimalloc: bool,
    runs: usize,
    update_baseline: bool,
}

struct Measurement {
    startup_ms: u64,
    pss_kib: u64,
    rss_kib: u64,
}

pub(crate) fn run(root: &Path, args: &[String]) -> anyhow::Result<ExitCode> {
    let opts = parse(args)?;
    let binary = build(root, &opts)?;

    let display = match opts.display {
        Display::Headless => Some(HeadlessCompositor::start(root)?),
        Display::Session => None,
    };

    let mut runs = Vec::with_capacity(opts.runs);
    for i in 0..opts.runs {
        let m = measure(&binary, &opts, display.as_ref())
            .with_context(|| format!("run {} failed", i + 1))?;
        eprintln!(
            "  run {}: start-up {} ms, PSS {:.1} MiB",
            i + 1,
            m.startup_ms,
            mib(m.pss_kib)
        );
        runs.push(m);
    }
    drop(display);

    let result = Measurement {
        startup_ms: median(runs.iter().map(|m| m.startup_ms).collect()),
        pss_kib: median(runs.iter().map(|m| m.pss_kib).collect()),
        rss_kib: median(runs.iter().map(|m| m.rss_kib).collect()),
    };
    report(root, &opts, &result)
}

fn parse(args: &[String]) -> anyhow::Result<Options> {
    let mut scenario = "idle".to_owned();
    let mut profile = "release".to_owned();
    let mut display = None;
    let mut renderer = None;
    let mut mimalloc = false;
    let mut runs = 3;
    let mut update_baseline = false;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .with_context(|| format!("{arg} needs a value"))
        };
        match arg.as_str() {
            "--scenario" => scenario = value()?,
            "--profile" => profile = value()?,
            "--display" => {
                display = Some(match value()?.as_str() {
                    "headless" => Display::Headless,
                    "session" => Display::Session,
                    other => bail!("unknown display mode `{other}`"),
                })
            }
            "--renderer" => renderer = Some(value()?),
            "--mimalloc" => mimalloc = true,
            "--runs" => runs = value()?.parse().context("--runs expects a number")?,
            "--update-baseline" => update_baseline = true,
            other => bail!("unknown option `{other}`"),
        }
    }
    let scenario = SCENARIOS
        .iter()
        .find(|s| s.name == scenario)
        .with_context(|| format!("unknown scenario `{scenario}`"))?;
    let display = display.unwrap_or(if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        Display::Session
    } else {
        Display::Headless
    });
    if runs == 0 {
        bail!("--runs must be at least 1");
    }
    Ok(Options {
        scenario,
        profile,
        display,
        renderer,
        mimalloc,
        runs,
        update_baseline,
    })
}

fn build(root: &Path, opts: &Options) -> anyhow::Result<PathBuf> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(root).args([
        "build",
        "--quiet",
        "--package",
        "strata",
        "--profile",
        &opts.profile,
    ]);
    if opts.mimalloc {
        cmd.args(["--features", "mimalloc"]);
    }
    let status = cmd.status().context("cannot run cargo")?;
    if !status.success() {
        bail!("build failed");
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let dir = if opts.profile == "dev" {
        "debug"
    } else {
        &opts.profile
    };
    Ok(target.join(dir).join("strata"))
}

fn measure(
    binary: &Path,
    opts: &Options,
    compositor: Option<&HeadlessCompositor>,
) -> anyhow::Result<Measurement> {
    let mut cmd = Command::new(binary);
    cmd.env("STRATA_BENCH_SCENARIO", opts.scenario.name)
        .env("STRATA_LOG", "warn")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if let Some(renderer) = &opts.renderer {
        cmd.env("GSK_RENDERER", renderer);
    }
    if let Some(compositor) = compositor {
        cmd.env("WAYLAND_DISPLAY", &compositor.socket)
            .env("XDG_RUNTIME_DIR", &compositor.runtime_dir)
            .env("GDK_BACKEND", "wayland")
            .env_remove("DISPLAY");
    }

    let start = Instant::now();
    let child = cmd.spawn().context("cannot start strata")?;
    let mut child = KillOnDrop(child);
    let stdout = child.0.stdout.take().context("no stdout")?;

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line.trim() == READY_MARKER {
                let _ = tx.send(());
            }
        }
    });
    rx.recv_timeout(READY_TIMEOUT)
        .context("strata did not report readiness in time")?;
    let startup_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    std::thread::sleep(SETTLE);
    let pid = child.0.id();
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        samples.push(read_smaps_rollup(pid)?);
        std::thread::sleep(SAMPLE_INTERVAL);
    }
    let pss_kib = median(samples.iter().map(|(pss, _)| *pss).collect());
    let rss_kib = median(samples.iter().map(|(_, rss)| *rss).collect());
    Ok(Measurement {
        startup_ms,
        pss_kib,
        rss_kib,
    })
}

/// Returns `(Pss, Rss)` in KiB.
fn read_smaps_rollup(pid: u32) -> anyhow::Result<(u64, u64)> {
    let path = format!("/proc/{pid}/smaps_rollup");
    let text = fs::read_to_string(&path).with_context(|| format!("cannot read {path}"))?;
    let field = |name: &str| -> anyhow::Result<u64> {
        text.lines()
            .find_map(|l| l.strip_prefix(name))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|v| v.parse().ok())
            .with_context(|| format!("{name} missing in {path}"))
    };
    Ok((field("Pss:")?, field("Rss:")?))
}

fn report(root: &Path, opts: &Options, m: &Measurement) -> anyhow::Result<ExitCode> {
    let key = format!(
        "{}/{}/{}",
        opts.scenario.name,
        opts.display.as_str(),
        opts.renderer.as_deref().unwrap_or("default")
    );
    let baseline_path = root.join(BASELINE_FILE);
    let mut baseline: BTreeMap<String, Value> = match fs::read_to_string(&baseline_path) {
        Ok(text) => serde_json::from_str(&text).context("invalid bench/baseline.json")?,
        Err(_) => BTreeMap::new(),
    };
    let reference = baseline
        .get(&key)
        .and_then(|v| v.get("pss_kib"))
        .and_then(Value::as_u64);

    let objective = opts.scenario.objective_kib;
    let regression = reference.map(|r| (m.pss_kib as f64 - r as f64) / r as f64);
    let report = json!({
        "scenario": opts.scenario.name,
        "display": opts.display.as_str(),
        "profile": opts.profile,
        "renderer": opts.renderer.as_deref().unwrap_or("default"),
        "allocator": if opts.mimalloc { "mimalloc" } else { "glibc" },
        "runs": opts.runs,
        "startup_ms": m.startup_ms,
        "pss_kib": m.pss_kib,
        "rss_kib": m.rss_kib,
        "objective_kib": objective,
        "baseline_pss_kib": reference,
        "regression_ratio": regression,
    });
    let out_dir = root.join("target").join("bench-mem");
    fs::create_dir_all(&out_dir)?;
    let out = out_dir.join(format!(
        "{}-{}.json",
        opts.scenario.name,
        opts.display.as_str()
    ));
    fs::write(&out, serde_json::to_string_pretty(&report)?)?;

    println!(
        "{key}: start-up {} ms, PSS {:.1} MiB (RSS {:.1} MiB), objective {:.1} MiB{}",
        m.startup_ms,
        mib(m.pss_kib),
        mib(m.rss_kib),
        mib(objective),
        if m.pss_kib > objective {
            " — objective exceeded (informative, ADR-0013)"
        } else {
            ""
        }
    );
    println!("report: {}", out.display());

    if opts.update_baseline {
        baseline.insert(
            key.clone(),
            json!({ "pss_kib": m.pss_kib, "startup_ms": m.startup_ms }),
        );
        fs::create_dir_all(baseline_path.parent().unwrap_or(root))?;
        fs::write(
            &baseline_path,
            serde_json::to_string_pretty(&baseline)? + "\n",
        )?;
        println!("baseline updated: {key}");
        return Ok(ExitCode::SUCCESS);
    }

    match (reference, regression) {
        (Some(r), Some(ratio)) if ratio > REGRESSION_TOLERANCE => {
            eprintln!(
                "REGRESSION: PSS {:.1} MiB vs baseline {:.1} MiB (+{:.1} %)",
                mib(m.pss_kib),
                mib(r),
                ratio * 100.0
            );
            Ok(ExitCode::FAILURE)
        }
        (Some(r), Some(ratio)) => {
            println!("baseline {:.1} MiB ({:+.1} %)", mib(r), ratio * 100.0);
            Ok(ExitCode::SUCCESS)
        }
        _ => {
            println!("no baseline for {key} (use --update-baseline to record one)");
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn median(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    values.get(values.len() / 2).copied().unwrap_or(0)
}

fn mib(kib: u64) -> f64 {
    kib as f64 / 1024.0
}

/// Child process killed when dropped (also on error paths).
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Headless Weston compositor used when no graphical session is available.
///
/// Note: without GPU access GTK falls back to software rendering, so the
/// figures are not comparable to a `session` run.
struct HeadlessCompositor {
    _process: KillOnDrop,
    socket: String,
    runtime_dir: PathBuf,
}

impl HeadlessCompositor {
    fn start(root: &Path) -> anyhow::Result<Self> {
        let runtime_dir = match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => {
                let dir = root.join("target").join("xdg-runtime");
                fs::create_dir_all(&dir)?;
                fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
                dir
            }
        };
        let socket = format!("strata-bench-{}", std::process::id());
        let process = Command::new("weston")
            .args([
                "--backend=headless",
                "--width=1280",
                "--height=800",
                "--idle-time=0",
            ])
            .arg(format!("--socket={socket}"))
            .env("XDG_RUNTIME_DIR", &runtime_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("cannot start weston (install it for headless benchmarks)")?;
        let process = KillOnDrop(process);

        let socket_path = runtime_dir.join(&socket);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !socket_path.exists() {
            if Instant::now() > deadline {
                bail!("weston did not create its socket");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(Self {
            _process: process,
            socket,
            runtime_dir,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_of_odd_and_even_sets() {
        assert_eq!(median(vec![3, 1, 2]), 2);
        assert_eq!(median(vec![4, 1, 3, 2]), 3);
        assert_eq!(median(vec![]), 0);
    }
}
