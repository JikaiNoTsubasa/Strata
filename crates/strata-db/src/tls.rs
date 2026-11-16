// SPDX-License-Identifier: GPL-3.0-or-later
//! rustls configuration for the libpq-style TLS modes (CON-4).

use std::path::Path;
use std::sync::Arc;

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
};
use strata_core::{TlsConfig, TlsMode};

use crate::{DbError, Result};

/// Builds the client configuration for `tls` (whose mode is not `Disable`).
pub(crate) fn client_config(tls: &TlsConfig) -> Result<ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let algorithms = provider.signature_verification_algorithms;

    let verifier: Arc<dyn ServerCertVerifier> = match tls.mode {
        // libpq semantics: encrypted, but the certificate is not verified.
        TlsMode::Disable | TlsMode::Prefer | TlsMode::Require => {
            Arc::new(AcceptAnyCertificate { algorithms })
        }
        TlsMode::VerifyCa => Arc::new(IgnoreHostName {
            inner: webpki_verifier(tls, &provider)?,
        }),
        TlsMode::VerifyFull => webpki_verifier(tls, &provider)?,
    };

    let builder = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| DbError::Tls(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(verifier);

    match (&tls.client_cert, &tls.client_key) {
        (Some(cert), Some(key)) => {
            let certs = load_certs(cert)?;
            let key =
                PrivateKeyDer::from_pem_file(key).map_err(|e| config_error(key, &e.to_string()))?;
            builder
                .with_client_auth_cert(certs, key)
                .map_err(|e| DbError::Tls(e.to_string()))
        }
        _ => Ok(builder.with_no_client_auth()),
    }
}

fn config_error(path: &Path, err: &str) -> DbError {
    DbError::Config(format!("{}: {err}", path.display()))
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let certs: Vec<_> = CertificateDer::pem_file_iter(path)
        .map_err(|e| config_error(path, &e.to_string()))?
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| config_error(path, &e.to_string()))?;
    if certs.is_empty() {
        return Err(config_error(path, "no certificate found"));
    }
    Ok(certs)
}

fn webpki_verifier(
    tls: &TlsConfig,
    provider: &Arc<CryptoProvider>,
) -> Result<Arc<WebPkiServerVerifier>> {
    let mut roots = RootCertStore::empty();
    match &tls.ca_cert {
        Some(ca) => {
            for cert in load_certs(ca)? {
                roots
                    .add(cert)
                    .map_err(|e| config_error(ca, &e.to_string()))?;
            }
        }
        None => {
            let native = rustls_native_certs::load_native_certs();
            for err in &native.errors {
                tracing::debug!(%err, "skipping unreadable system certificate");
            }
            roots.add_parsable_certificates(native.certs);
        }
    }
    WebPkiServerVerifier::builder_with_provider(Arc::new(roots), Arc::clone(provider))
        .build()
        .map_err(|e| DbError::Tls(e.to_string()))
}

/// `prefer` / `require`: any certificate is accepted (libpq behaviour), but
/// handshake signatures are still checked.
#[derive(Debug)]
struct AcceptAnyCertificate {
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for AcceptAnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// `verify-ca`: full chain verification, host name not checked.
#[derive(Debug)]
struct IgnoreHostName {
    inner: Arc<WebPkiServerVerifier>,
}

impl ServerCertVerifier for IgnoreHostName {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        match self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        ) {
            Err(rustls::Error::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
            )) => Ok(ServerCertVerified::assertion()),
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_builds_with_system_roots() {
        for mode in [
            TlsMode::Prefer,
            TlsMode::Require,
            TlsMode::VerifyCa,
            TlsMode::VerifyFull,
        ] {
            let tls = TlsConfig {
                mode,
                ..TlsConfig::default()
            };
            assert!(client_config(&tls).is_ok(), "{mode}");
        }
    }

    #[test]
    fn missing_ca_file_is_a_configuration_error() {
        let tls = TlsConfig {
            mode: TlsMode::VerifyFull,
            ca_cert: Some("/nonexistent/ca.pem".into()),
            ..TlsConfig::default()
        };
        assert!(matches!(client_config(&tls), Err(DbError::Config(_))));
    }
}
