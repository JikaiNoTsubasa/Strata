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
- Connection management: create, edit, duplicate, delete, search; label colour and
  environment badge (dev/staging/prod); read-only option.
- PostgreSQL connection over TCP or Unix socket, TLS modes disable/prefer/require/
  verify-ca/verify-full with custom CA and client certificates.
- Connection test with duration, server version and detailed errors.
- Passwords in the system keyring (Secret Service / portal), "ask every time", or read
  from `~/.pgpass` / `~/.my.cnf`; import from `pg_service.conf`, `.pgpass`, `.my.cnf`.
- Connection page with status, server, encryption; production and clear-text banners.
- Internal SQLite storage (`$XDG_DATA_HOME/strata/strata.db`, mode 0600).
- Schema browser in the sidebar: connections > databases > schemas > tables, views,
  materialized views, sequences, functions, procedures > columns, indexes, constraints,
  triggers; loaded lazily with per-node loading and error indicators; filter.
- SSH tunnels: SSH agent, password or private key (optional passphrase), secret in
  the keyring or asked; server keys checked against `~/.ssh/known_hosts` and Strata's
  own file, unknown keys trusted on first use after confirmation, changed keys refused.
- Object tabs with properties (columns, keys, indexes, triggers, estimated rows, size)
  and highlighted DDL; copy name (quoted) and DDL from the context menu.
- One session per browsed database, automatic reconnection after a connection loss,
  idle sessions closed after 15 minutes (configurable).
