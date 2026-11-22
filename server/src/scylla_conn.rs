//! Connecting to ScyllaDB: optional password authentication and TLS.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use rustls::RootCertStore;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;
use scylla::client::session::Session;
use scylla::client::session_builder::SessionBuilder;

use crate::config::ScyllaAuth;

/// A rustls client configuration that trusts exactly the CAs in the PEM file at `ca_path`.
///
/// Nodes are verified against the host or IP written in `THEHIDEOUT_SCYLLA_NODES`,
/// so the node certificates need that name or address as a subject alternative name.
pub fn tls_config(ca_path: &Path) -> Result<rustls::ClientConfig> {
    let mut roots = RootCertStore::empty();
    let mut found = 0;
    for cert in CertificateDer::pem_file_iter(ca_path).with_context(|| format!("reading THEHIDEOUT_SCYLLA_TLS_CA {}", ca_path.display()))? {
        let cert = cert.with_context(|| format!("parsing a certificate in {}", ca_path.display()))?;
        roots.add(cert).with_context(|| format!("invalid CA certificate in {}", ca_path.display()))?;
        found += 1;
    }
    anyhow::ensure!(found > 0, "THEHIDEOUT_SCYLLA_TLS_CA {} contains no certificate", ca_path.display());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .context("TLS protocol versions")?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

/// Opens the session to `nodes`, with credentials and TLS when configured.
pub async fn connect(nodes: &[String], auth: Option<&ScyllaAuth>, tls_ca: Option<&Path>) -> Result<Session> {
    let mut builder = SessionBuilder::new().known_nodes(nodes);
    if let Some(auth) = auth {
        builder = builder.user(&auth.username, &auth.password.0);
    }
    if let Some(ca) = tls_ca {
        builder = builder.tls_context(Some(Arc::new(tls_config(ca)?)));
    }
    builder.build().await.context("connecting to ScyllaDB")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_or_empty_ca_file_is_an_error() {
        assert!(tls_config(Path::new("/nonexistent/ca.pem")).is_err());
        let empty = std::env::temp_dir().join(format!("thehideout-empty-ca-{}.pem", std::process::id()));
        std::fs::write(&empty, "not a certificate\n").unwrap();
        let err = tls_config(&empty).unwrap_err();
        let _ = std::fs::remove_file(&empty);
        assert!(format!("{err:#}").contains("no certificate"), "{err:#}");
    }
}
