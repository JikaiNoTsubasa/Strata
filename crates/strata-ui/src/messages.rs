// SPDX-License-Identifier: GPL-3.0-or-later
//! User-facing (translated) labels for domain values and errors.

use strata_core::{DbKind, Environment, ErrorKind, PasswordPolicy, TlsMode, ValidationError};

use crate::i18n::tr;

/// Short sentence describing an error category.
pub(crate) fn error_summary(kind: ErrorKind) -> String {
    match kind {
        ErrorKind::HostNotFound => tr("Host not found."),
        ErrorKind::ConnectionRefused => tr("Cannot reach the server."),
        ErrorKind::Timeout => tr("The server did not respond in time."),
        ErrorKind::Authentication => tr("Authentication failed: check the user name and password."),
        ErrorKind::MissingPassword => tr("A password is required."),
        ErrorKind::Tls => tr("The secure connection (TLS) failed."),
        ErrorKind::Server => tr("The server returned an error."),
        ErrorKind::Configuration => tr("The connection settings are invalid."),
        ErrorKind::Keyring => tr("The system keyring could not be used."),
        ErrorKind::Storage => tr("Strata could not access its internal database."),
        ErrorKind::Unsupported => tr("This database type is not supported yet."),
        ErrorKind::NotConnected => tr("Not connected."),
        ErrorKind::NotFound => tr("The object no longer exists."),
        ErrorKind::SshConnection => tr("Cannot reach the SSH server."),
        ErrorKind::SshAuthentication => tr("SSH authentication failed."),
        ErrorKind::MissingSshSecret => tr("The SSH password or key passphrase is required."),
        ErrorKind::UnknownHostKey => tr("The SSH server is not known yet."),
        ErrorKind::HostKeyMismatch => tr(
            "The SSH server key has changed: the connection was refused to protect you from a possible attack.",
        ),
        _ => tr("Unexpected error."),
    }
}

/// Message for a settings validation problem.
pub(crate) fn validation(error: ValidationError) -> String {
    match error {
        ValidationError::EmptyName => tr("Enter a name."),
        ValidationError::EmptyHost => tr("Enter a host."),
        ValidationError::InvalidPort => tr("Enter a valid port."),
        ValidationError::RelativeSocketPath => tr("The socket path must be absolute."),
        ValidationError::EmptyUser => tr("Enter a user name."),
        ValidationError::InvalidColor => tr("Invalid label color."),
        ValidationError::IncompleteClientCertificate => {
            tr("Select both the client certificate and its private key.")
        }
        ValidationError::EmptySshHost => tr("Enter the SSH server."),
        ValidationError::EmptySshUser => tr("Enter the SSH user name."),
        ValidationError::InvalidSshPort => tr("Enter a valid SSH port."),
        ValidationError::RelativeSshKeyPath => tr("Select the SSH private key file."),
        ValidationError::SshWithSocket => tr("SSH tunnels require a network (TCP/IP) connection."),
    }
}

/// Database type name.
pub(crate) fn db_kind(kind: DbKind) -> String {
    match kind {
        DbKind::Postgres => "PostgreSQL".to_owned(),
        DbKind::MySql => "MySQL / MariaDB".to_owned(),
    }
}

/// Environment name.
pub(crate) fn environment(env: Option<Environment>) -> String {
    match env {
        None => tr("Not specified"),
        Some(Environment::Development) => tr("Development"),
        Some(Environment::Staging) => tr("Staging"),
        Some(Environment::Production) => tr("Production"),
    }
}

/// Short badge text of an environment.
pub(crate) fn environment_badge(env: Environment) -> String {
    match env {
        // Translators: short badge for the development environment.
        Environment::Development => tr("DEV"),
        // Translators: short badge for the staging environment.
        Environment::Staging => tr("STAGING"),
        // Translators: short badge for the production environment.
        Environment::Production => tr("PROD"),
    }
}

/// Password policy label.
pub(crate) fn password_policy(policy: PasswordPolicy) -> String {
    match policy {
        PasswordPolicy::Keyring => tr("Save in the system keyring"),
        PasswordPolicy::AskEveryTime => tr("Ask every time"),
        PasswordPolicy::ClientConfigFile => tr("Read from ~/.pgpass or ~/.my.cnf"),
        PasswordPolicy::None => tr("No password"),
    }
}

/// TLS mode label.
pub(crate) fn tls_mode(mode: TlsMode) -> String {
    match mode {
        TlsMode::Disable => tr("Disabled"),
        TlsMode::Prefer => tr("Preferred"),
        TlsMode::Require => tr("Required"),
        TlsMode::VerifyCa => tr("Verify certificate authority"),
        TlsMode::VerifyFull => tr("Verify certificate and host name"),
    }
}

/// Explanation of a TLS mode.
pub(crate) fn tls_mode_help(mode: TlsMode) -> String {
    match mode {
        TlsMode::Disable => tr("The connection is never encrypted."),
        TlsMode::Prefer => {
            tr("Encrypted if the server supports it; the certificate is not verified.")
        }
        TlsMode::Require => tr("Always encrypted; the certificate is not verified."),
        TlsMode::VerifyCa => {
            tr("Always encrypted; the certificate must be signed by a trusted authority.")
        }
        TlsMode::VerifyFull => {
            tr("Always encrypted; the certificate must be trusted and match the host name.")
        }
    }
}
