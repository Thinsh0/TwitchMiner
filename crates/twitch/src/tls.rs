//! A single rustls configuration shared by HTTP and WebSocket connections.
//!
//! Roots = Mozilla roots (always available, also on Android) + the platform store
//! (+ `SSL_CERT_FILE` / `SSL_CERT_DIR`, honoured by `rustls-native-certs`), so the
//! client works behind TLS-intercepting corporate proxies as well.

use std::sync::{Arc, OnceLock};

use rustls::{ClientConfig, RootCertStore};

static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();

pub fn client_config() -> Arc<ClientConfig> {
    CONFIG.get_or_init(|| Arc::new(build())).clone()
}

fn build() -> ClientConfig {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let native = rustls_native_certs::load_native_certs();
    let (added, _ignored) = roots.add_parsable_certificates(native.certs);
    tracing::debug!(native_roots = added, "TLS roots loaded");

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("ring supports the default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth()
}
