# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Cargo workspace skeleton (core, db, sql, store, secrets, export, engine, ui crates).
- Empty main window (libadwaita) with welcome page, Tokio runtime and UI/engine channels.
- French translation infrastructure (gettext).
- `cargo xtask bench-mem` memory benchmark (idle scenario).
- Continuous integration workflow.
