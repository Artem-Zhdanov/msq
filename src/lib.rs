pub mod config;
pub mod metrics;

use rustls_pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use transport_layer::NetCredential;

pub const MAGIC_NUMBER: u64 = 123456789876543210;
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn get_credential() -> NetCredential {
    let key = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    NetCredential {
        my_key: PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key.key_pair.serialize_der())),
        my_certs: vec![key.cert.der().clone()],
        root_certs: vec![],
    }
}
