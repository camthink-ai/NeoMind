//! Self-signed TLS certificate generation for the embedded MQTT broker.
//!
//! Generates a CA certificate and a server certificate signed by that CA,
//! suitable for encrypting MQTT connections in IoT scenarios.

use std::net::IpAddr;
use std::path::PathBuf;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose, SanType,
};

/// Paths to the generated PEM files.
pub struct CertPaths {
    pub ca_cert_path: String,
    pub ca_key_path: String,
    pub server_cert_path: String,
    pub server_key_path: String,
}

/// Get the TLS directory for storing certificates.
/// Uses `NEOMIND_DATA_DIR` env var (same as the rest of the project) or falls back to `data`.
fn get_tls_dir() -> PathBuf {
    let data_dir = std::env::var("NEOMIND_DATA_DIR").unwrap_or_else(|_| "data".to_string());
    PathBuf::from(data_dir).join("tls")
}

/// Get the local machine's LAN IP address (private IPv4).
fn get_local_ip() -> Option<IpAddr> {
    let interfaces = get_if_addrs::get_if_addrs().ok()?;
    for iface in interfaces {
        if !iface.is_loopback() {
            if let get_if_addrs::IfAddr::V4(iface_addr) = iface.addr {
                let ip = iface_addr.ip;
                let octets = ip.octets();
                if (octets[0] == 192 && octets[1] == 168)
                    || (octets[0] == 10)
                    || (octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31)
                {
                    return Some(IpAddr::V4(ip));
                }
            }
        }
    }
    None
}

/// Get the system hostname for inclusion in SANs.
fn get_hostname() -> Option<String> {
    hostname::get().ok().and_then(|h| h.into_string().ok())
}

/// Write a private key file with mode `0600` atomically at creation time on Unix,
/// avoiding the `fs::write` (0644) -> `set_permissions` (0600) TOCTOU window.
///
/// Also enforces `0600` on the open file descriptor via `File::set_permissions`
/// (`fchmod`) before writing, so overwriting an existing file with looser
/// permissions tightens it before any key material is written.
pub(crate) fn write_private_key_file(
    path: &std::path::Path,
    contents: &[u8],
) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        file.write_all(contents)?;
        file.sync_all()?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, contents)
    }
}

/// Generate self-signed CA + server certificates for the MQTT broker.
///
/// The CA certificate is valid for 5 years; the server certificate is valid
/// for 1 year and includes SANs for localhost, 127.0.0.1, the local LAN IP,
/// and the system hostname.
///
/// Both certificates include proper Key Usage and Extended Key Usage extensions
/// required by strict TLS implementations (e.g. rustls).
pub fn generate_self_signed_certs() -> Result<CertPaths, String> {
    let tls_dir = get_tls_dir();
    std::fs::create_dir_all(&tls_dir)
        .map_err(|e| format!("Failed to create TLS directory: {}", e))?;

    // Use time::OffsetDateTime directly (same type rcgen uses internally)
    // to avoid the panic risk in rcgen::date_time_ymd on invalid dates (e.g. Feb 29).
    let now = time::OffsetDateTime::now_utc();
    // Start 1 hour in the past to tolerate clock skew on devices
    let not_before = now - time::Duration::hours(1);
    let ca_not_after = now + time::Duration::days(5 * 365);
    let server_not_after = now + time::Duration::days(365);

    // --- CA key + self-signed cert ---
    let ca_key = KeyPair::generate().map_err(|e| format!("Failed to generate CA key: {}", e))?;

    let mut ca_params = CertificateParams::new(Vec::<String>::new())
        .map_err(|e| format!("Failed to create CA params: {}", e))?;
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "NeoMind MQTT CA");
    ca_params
        .distinguished_name
        .push(DnType::OrganizationName, "NeoMind");

    // CA Key Usage: certificate signing and CRL signing
    ca_params.key_usages.push(KeyUsagePurpose::KeyCertSign);
    ca_params.key_usages.push(KeyUsagePurpose::CrlSign);
    ca_params.key_usages.push(KeyUsagePurpose::DigitalSignature);

    ca_params.not_before = not_before;
    ca_params.not_after = ca_not_after;

    let ca_cert = ca_params
        .self_signed(&ca_key)
        .map_err(|e| format!("Failed to sign CA cert: {}", e))?;

    // --- Server key + cert signed by CA ---
    let server_key =
        KeyPair::generate().map_err(|e| format!("Failed to generate server key: {}", e))?;

    let mut server_params = CertificateParams::new(Vec::<String>::new())
        .map_err(|e| format!("Failed to create server params: {}", e))?;
    server_params
        .distinguished_name
        .push(DnType::CommonName, "NeoMind MQTT Server");

    // SANs: localhost, 127.0.0.1, local LAN IP, and hostname
    let mut sans = vec![
        SanType::DnsName(
            "localhost"
                .try_into()
                .map_err(|_| "Invalid DNS name".to_string())?,
        ),
        SanType::IpAddress(IpAddr::from([127, 0, 0, 1])),
    ];
    if let Some(local_ip) = get_local_ip() {
        sans.push(SanType::IpAddress(local_ip));
    }
    if let Some(hostname) = get_hostname() {
        if let Ok(dns_name) = hostname.as_str().try_into() {
            sans.push(SanType::DnsName(dns_name));
        }
    }
    server_params.subject_alt_names = sans;

    // Server Key Usage: digital signature and key encipherment
    server_params
        .key_usages
        .push(KeyUsagePurpose::DigitalSignature);
    server_params
        .key_usages
        .push(KeyUsagePurpose::KeyEncipherment);
    // Extended Key Usage: TLS server authentication
    server_params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ServerAuth);

    server_params.not_before = not_before;
    server_params.not_after = server_not_after;

    let sans_count = server_params.subject_alt_names.len();

    let server_cert = server_params
        .signed_by(&server_key, &ca_cert, &ca_key)
        .map_err(|e| format!("Failed to sign server cert: {}", e))?;

    // --- Write PEM files ---
    let ca_cert_path = tls_dir.join("mqtt-ca.crt");
    let ca_key_path = tls_dir.join("mqtt-ca.key");
    let server_cert_path = tls_dir.join("mqtt-server.crt");
    let server_key_path = tls_dir.join("mqtt-server.key");

    std::fs::write(&ca_cert_path, ca_cert.pem())
        .map_err(|e| format!("Failed to write CA cert: {}", e))?;
    write_private_key_file(&ca_key_path, ca_key.serialize_pem().as_bytes())
        .map_err(|e| format!("Failed to write CA key: {}", e))?;
    std::fs::write(&server_cert_path, server_cert.pem())
        .map_err(|e| format!("Failed to write server cert: {}", e))?;
    write_private_key_file(&server_key_path, server_key.serialize_pem().as_bytes())
        .map_err(|e| format!("Failed to write server key: {}", e))?;

    tracing::info!(
        ca_cert = %ca_cert_path.display(),
        server_cert = %server_cert_path.display(),
        sans_count = sans_count,
        "Generated self-signed TLS certificates"
    );

    Ok(CertPaths {
        ca_cert_path: ca_cert_path.to_string_lossy().to_string(),
        ca_key_path: ca_key_path.to_string_lossy().to_string(),
        server_cert_path: server_cert_path.to_string_lossy().to_string(),
        server_key_path: server_key_path.to_string_lossy().to_string(),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn write_private_key_file_creates_with_0600_and_tightens_existing() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = dir.path().join("mqtt-server.key");
        write_private_key_file(&fresh, b"secret-key-1").unwrap();
        let mode = std::fs::metadata(&fresh).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "fresh private key must be created 0600");
        assert_eq!(std::fs::read(&fresh).unwrap(), b"secret-key-1");

        // Pre-existing file with world-readable 0644 must be tightened to 0600
        let existing = dir.path().join("mqtt-ca.key");
        std::fs::write(&existing, b"old-key").unwrap();
        std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private_key_file(&existing, b"new-secret-key").unwrap();
        let mode = std::fs::metadata(&existing).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "overwritten private key must be tightened to 0600"
        );
        assert_eq!(std::fs::read(&existing).unwrap(), b"new-secret-key");
    }
}
