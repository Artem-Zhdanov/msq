pub mod config;
pub mod metrics;
pub mod quic_settings;

use msquic_async::msquic::{CertificateFile, Credential};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use transport_layer::NetCredential;

pub const MAGIC_NUMBER: u64 = 123456789876543210;
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// input can be: "5000-5010", "5000", "5000,6000, 7000-7010"
pub fn ports_string_to_vec(input: &str) -> anyhow::Result<Vec<u16>> {
    let mut ports = std::collections::BTreeSet::new(); // to keep them sorted and unique

    for token in input.split(',') {
        if let Some((start, end)) = token.split_once('-') {
            let start: u16 = start.trim().parse()?;
            let end: u16 = end.trim().parse()?;
            if start > end {
                return Err(anyhow::anyhow!("Start port {} > end port {}", start, end));
            }
            ports.extend(start..=end);
        } else {
            let port: u16 = token.trim().parse()?;
            ports.insert(port);
        }
    }

    Ok(ports.into_iter().collect())
}

pub fn get_test_cred() -> Credential {
    let cert_dir = std::env::temp_dir().join("msquic_test_rs");
    let key = "key.pem";
    let cert = "cert.pem";
    let key_path = cert_dir.join(key);
    let cert_path = cert_dir.join(cert);
    if !key_path.exists() || !cert_path.exists() {
        // remove the dir
        let _ = std::fs::remove_dir_all(&cert_dir);
        std::fs::create_dir_all(&cert_dir).expect("cannot create cert dir");
        // generate test cert using openssl cli
        let output = std::process::Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:4096",
                "-keyout",
                "key.pem",
                "-out",
                "cert.pem",
                "-sha256",
                "-days",
                "3650",
                "-nodes",
                "-subj",
                "/CN=localhost",
            ])
            .current_dir(cert_dir)
            .stderr(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .output()
            .expect("cannot generate cert");
        if !output.status.success() {
            panic!("generate cert failed");
        }
    }
    Credential::CertificateFile(CertificateFile::new(
        key_path.display().to_string(),
        cert_path.display().to_string(),
    ))
}

pub fn get_credential() -> NetCredential {
    NetCredential {
        my_key: PrivateKeyDer::from(PrivatePkcs8KeyDer::from(vec![])),
        my_certs: vec![CertificateDer::from(vec![])],
        root_certs: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::ports_string_to_vec;

    #[test]
    fn test_parse_single_ports() {
        let result = ports_string_to_vec("5000,5002,5003").unwrap();
        assert_eq!(result, vec![5000, 5002, 5003]);
    }

    #[test]
    fn test_parse_port_ranges() {
        let result = ports_string_to_vec("5000-5002").unwrap();
        assert_eq!(result, vec![5000, 5001, 5002]);
    }

    #[test]
    fn test_parse_mixed_ports() {
        let result = ports_string_to_vec("5000,5002,5005-5007").unwrap();
        assert_eq!(result, vec![5000, 5002, 5005, 5006, 5007]);
    }

    #[test]
    fn test_parse_duplicate_and_sorted() {
        let result = ports_string_to_vec("5002,5000,5002,5001").unwrap();
        assert_eq!(result, vec![5000, 5001, 5002]);
    }

    #[test]
    fn test_parse_with_spaces() {
        let result = ports_string_to_vec(" 5000 , 5001 - 5002 ").unwrap();
        assert_eq!(result, vec![5000, 5001, 5002]);
    }

    #[test]
    fn test_invalid_port_number() {
        let err = ports_string_to_vec("not_a_port").unwrap_err();
        assert!(err.to_string().contains("invalid digit"));
    }

    #[test]
    fn test_invalid_range_order() {
        let err = ports_string_to_vec("5005-5002").unwrap_err();
        assert!(err.to_string().contains("Start port"));
    }

    #[test]
    fn test_empty_string_should_fail() {
        let result = ports_string_to_vec("");
        assert!(result.is_err());
    }
}
