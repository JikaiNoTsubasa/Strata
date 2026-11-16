-- Schema v1: connections and groups (CDC §11).
-- Passwords are NEVER stored here: `secret_ref` only names a keyring item.

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

CREATE TABLE groups (
    id        INTEGER PRIMARY KEY,
    name      TEXT NOT NULL,
    parent_id INTEGER REFERENCES groups (id) ON DELETE CASCADE
) STRICT;

CREATE TABLE connections (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    kind            TEXT NOT NULL,
    host            TEXT,
    port            INTEGER,
    socket_path     TEXT,
    database        TEXT,
    username        TEXT NOT NULL,
    tls_mode        TEXT NOT NULL,
    tls_ca          TEXT,
    tls_cert        TEXT,
    tls_key         TEXT,
    environment     TEXT,
    color           TEXT,
    group_id        INTEGER REFERENCES groups (id) ON DELETE SET NULL,
    read_only       INTEGER NOT NULL DEFAULT 0,
    password_policy TEXT NOT NULL,
    record_history  INTEGER NOT NULL DEFAULT 1,
    secret_ref      TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    CHECK ((host IS NOT NULL AND port IS NOT NULL) OR socket_path IS NOT NULL)
) STRICT;

CREATE INDEX connections_name ON connections (name COLLATE NOCASE);
