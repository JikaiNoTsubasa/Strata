// SPDX-License-Identifier: GPL-3.0-or-later
//! Strata binary: logging, Tokio runtime, engine and UI start-up.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use strata_engine::{EngineConfig, StoreLocation};
use strata_secrets::{KeyringStore, MemoryStore, SecretStore};
use tracing_subscriber::EnvFilter;

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Environment variable holding the log filter (`tracing` directives).
const LOG_ENV: &str = "STRATA_LOG";
/// Default log filter. SQL text is never logged by default (CDC §10.5).
const DEFAULT_LOG_FILTER: &str = "warn,strata=info,strata_ui=info,strata_engine=info";
/// Number of Tokio worker threads (CDC §4).
const WORKER_THREADS: usize = 2;
/// Maximum number of threads for blocking work (SQLite, files).
const MAX_BLOCKING_THREADS: usize = 4;
/// Grace period granted to engine tasks on exit.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(500);

fn main() -> anyhow::Result<ExitCode> {
    init_logging();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(WORKER_THREADS)
        .max_blocking_threads(MAX_BLOCKING_THREADS)
        .thread_name("strata-rt")
        .enable_all()
        .build()
        .context("failed to start the Tokio runtime")?;

    let engine = strata_engine::spawn(runtime.handle(), engine_config());
    let code = strata_ui::run(engine);

    runtime.shutdown_timeout(SHUTDOWN_TIMEOUT);
    Ok(code)
}

/// Real storage and keyring, except for benchmarks which must not depend on
/// (nor modify) the user's data.
fn engine_config() -> EngineConfig {
    if std::env::var_os("STRATA_BENCH_SCENARIO").is_some() {
        EngineConfig {
            store: StoreLocation::InMemory,
            secrets: Arc::new(MemoryStore::new()),
            known_hosts: None,
        }
    } else {
        let secrets: Arc<dyn SecretStore> = Arc::new(KeyringStore::new(strata_ui::config::APP_ID));
        EngineConfig {
            store: StoreLocation::Default,
            secrets,
            known_hosts: None,
        }
    }
}

fn init_logging() {
    let filter =
        EnvFilter::try_from_env(LOG_ENV).unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}
