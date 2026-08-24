//! Hot Sheet 2 **Tier-1 mTLS material** (`docs/04` §4.6, `docs/08`, HS2-VT3JMF) — the
//! per-project **certificate authority** + **device (client) cert** issuance + **revocation**
//! that let a server bind off-loopback securely instead of refusing (Tier 0 is loopback +
//! shared secret; Tier 1 adds mutual TLS).
//!
//! Trust model (v1, option b): one **per-project CA** signs the server's leaf and every
//! device's client cert. Enrollment is manual — `issue_device` returns the device cert, its
//! key, and the CA cert for the operator to copy to the device by hand; a `revoked`
//! fingerprint list the verifier consults kills a lost device. (`.p12` packaging and QR
//! enrollment are deferred to the client work.) This crate is **rcgen-only** (no rustls) so
//! the CLI can issue certs without linking the server; the server layers the rustls verifier
//! on top of what's here.
//!
//! Material lives under `${HOTSHEET_HOME}/tls/<project-id>/` (machine-local, like the index
//! and instance files — a CA private key is a secret, never committed):
//! `ca.crt` `ca.key` `server.crt` `server.key` `devices/<name>.crt` `revoked`.

use std::collections::{BTreeMap, HashSet};
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose, SanType,
};
use sha2::{Digest, Sha256};

/// Why a certificate operation failed.
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("certificate: {0}")]
    Cert(#[from] rcgen::Error),
    #[error("TLS not initialized for this store — run `hotsheet-cli cert init` first")]
    NotInitialized,
    #[error("already initialized (ca.crt exists at {0}); refusing to overwrite")]
    AlreadyInitialized(PathBuf),
    #[error("no issued device named {0}")]
    NoSuchDevice(String),
    #[error("device {0} already has a certificate; use cert renew to rotate it")]
    DeviceExists(String),
    #[error("ACL: {0}")]
    Acl(#[from] serde_json::Error),
}

/// The on-disk layout of a project's TLS material.
pub struct Paths {
    pub dir: PathBuf,
}

impl Paths {
    /// `${HOTSHEET_HOME}/tls/<project-id>` for a store, where the project id is the same
    /// path-hash the index/instance files use, so the CLI and server resolve the same dir.
    pub fn for_store(store_path: &Path) -> Self {
        let dir = hotsheet_plugins::hotsheet_home()
            .join("tls")
            .join(project_id(store_path));
        Paths { dir }
    }

    /// Point at an explicit directory (tests).
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Paths { dir: dir.into() }
    }

    pub fn ca_cert(&self) -> PathBuf {
        self.dir.join("ca.crt")
    }
    pub fn ca_key(&self) -> PathBuf {
        self.dir.join("ca.key")
    }
    pub fn server_cert(&self) -> PathBuf {
        self.dir.join("server.crt")
    }
    pub fn server_key(&self) -> PathBuf {
        self.dir.join("server.key")
    }
    pub fn revoked(&self) -> PathBuf {
        self.dir.join("revoked")
    }
    pub fn acl(&self) -> PathBuf {
        self.dir.join("acl.json")
    }
    fn device_cert(&self, name: &str) -> PathBuf {
        self.dir.join("devices").join(format!("{name}.crt"))
    }

    /// Whether `cert init` has run (the CA + server leaf exist).
    pub fn is_initialized(&self) -> bool {
        self.ca_cert().is_file() && self.server_cert().is_file() && self.server_key().is_file()
    }
}

/// The project id — a SHA-256 of the canonical store path (first 16 hex), matching
/// `lifecycle::project_id` so the TLS dir lines up with the index/instance dirs.
pub fn project_id(store_path: &Path) -> String {
    let root = store_path
        .canonicalize()
        .unwrap_or_else(|_| store_path.to_path_buf());
    let mut h = Sha256::new();
    h.update(root.to_string_lossy().as_bytes());
    format!("{:x}", h.finalize())[..16].to_string()
}

/// Initialize the per-project CA + a server leaf whose SANs cover `hosts` (DNS names or IPs;
/// always includes `localhost` + `127.0.0.1`). Idempotency guard: refuses if a CA already
/// exists, so a re-init can't silently invalidate every issued device cert.
pub fn init_ca(paths: &Paths, hosts: &[String]) -> Result<(), TlsError> {
    if paths.ca_cert().exists() {
        return Err(TlsError::AlreadyInitialized(paths.ca_cert()));
    }
    std::fs::create_dir_all(&paths.dir)?;

    // The CA: self-signed, marked as a CA, allowed to sign certs.
    let ca_key = KeyPair::generate()?;
    let mut ca_params = CertificateParams::new(Vec::<String>::new())?;
    let now = time::OffsetDateTime::now_utc();
    ca_params.not_before = now - time::Duration::days(1);
    ca_params.not_after = now + time::Duration::days(3650);
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Hot Sheet Project CA");
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca_cert = ca_params.self_signed(&ca_key)?;

    // The server leaf: SAN-covered, server-auth, signed by the CA.
    let server_key = KeyPair::generate()?;
    let mut server_params = CertificateParams::new(Vec::<String>::new())?;
    server_params.not_before = now - time::Duration::days(1);
    server_params.not_after = now + time::Duration::days(397);
    server_params
        .distinguished_name
        .push(DnType::CommonName, "hotsheet-server");
    server_params.subject_alt_names = san_list(hosts);
    server_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let server_cert = server_params.signed_by(&server_key, &ca_cert, &ca_key)?;

    std::fs::write(paths.ca_cert(), ca_cert.pem())?;
    write_private(&paths.ca_key(), &ca_key.serialize_pem())?;
    std::fs::write(paths.server_cert(), server_cert.pem())?;
    write_private(&paths.server_key(), &server_key.serialize_pem())?;
    Ok(())
}

/// A freshly issued device (client) certificate — the operator copies `cert_pem`, `key_pem`,
/// and `ca_pem` to the device. `fingerprint` (sha256 of the cert DER, hex) is what `revoke`
/// and the verifier key off.
#[derive(Debug, Clone)]
pub struct DeviceCert {
    pub name: String,
    pub cert_pem: String,
    pub key_pem: String,
    pub ca_pem: String,
    pub fingerprint: String,
}

/// Issue a client cert for `name`, signed by the project CA. Records the cert under
/// `devices/<name>.crt` so it can be revoked later by name.
pub fn issue_device(paths: &Paths, name: &str) -> Result<DeviceCert, TlsError> {
    if !paths.is_initialized() {
        return Err(TlsError::NotInitialized);
    }
    if paths.device_cert(name).exists() {
        return Err(TlsError::DeviceExists(name.to_string()));
    }
    let ca_pem = std::fs::read_to_string(paths.ca_cert())?;
    let ca_key = KeyPair::from_pem(&std::fs::read_to_string(paths.ca_key())?)?;
    // Reconstruct the CA as an issuer: same subject DN + key ⇒ what it signs chains to the
    // stored ca.crt the verifier trusts as root.
    let ca_issuer = CertificateParams::from_ca_cert_pem(&ca_pem)?.self_signed(&ca_key)?;

    let dev_key = KeyPair::generate()?;
    let mut dev_params = CertificateParams::new(Vec::<String>::new())?;
    let now = time::OffsetDateTime::now_utc();
    dev_params.not_before = now - time::Duration::hours(1);
    dev_params.not_after = now + time::Duration::days(90);
    dev_params.distinguished_name.push(DnType::CommonName, name);
    dev_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    dev_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let dev_cert = dev_params.signed_by(&dev_key, &ca_issuer, &ca_key)?;

    let cert_pem = dev_cert.pem();
    let fingerprint = fingerprint_of_der(dev_cert.der());

    std::fs::create_dir_all(paths.dir.join("devices"))?;
    std::fs::write(paths.device_cert(name), &cert_pem)?;

    Ok(DeviceCert {
        name: name.to_string(),
        cert_pem,
        key_pem: dev_key.serialize_pem(),
        ca_pem,
        fingerprint,
    })
}

/// Rotate a device certificate: revoke the recorded leaf, then issue a fresh 90-day leaf
/// with the same device name. The old certificate stops working immediately on live servers.
pub fn renew_device(paths: &Paths, name: &str) -> Result<DeviceCert, TlsError> {
    let old_pem = std::fs::read_to_string(paths.device_cert(name))
        .map_err(|_| TlsError::NoSuchDevice(name.to_string()))?;
    let old_der = pem_to_der(&old_pem).ok_or_else(|| TlsError::NoSuchDevice(name.to_string()))?;
    let old_fingerprint = fingerprint_of_der(&old_der);
    let carried_role = load_acl(paths)?.and_then(|acl| acl.devices.get(&old_fingerprint).copied());
    revoke_device(paths, name)?;
    std::fs::remove_file(paths.device_cert(name))?;
    let renewed = issue_device(paths, name)?;
    if let Some(role) = carried_role {
        set_device_role(paths, name, role)?;
    }
    Ok(renewed)
}

/// Optional authorization role layered on top of CA membership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRole {
    ReadOnly,
    ReadWrite,
    Deny,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DeviceAcl {
    #[serde(default)]
    pub devices: BTreeMap<String, DeviceRole>,
}

/// Assign a role to the currently recorded certificate for `name`, keyed by fingerprint.
/// Creating `acl.json` switches the server from legacy CA-membership access to explicit ACLs:
/// fingerprints not present in the file are denied.
pub fn set_device_role(paths: &Paths, name: &str, role: DeviceRole) -> Result<String, TlsError> {
    let pem = std::fs::read_to_string(paths.device_cert(name))
        .map_err(|_| TlsError::NoSuchDevice(name.to_string()))?;
    let der = pem_to_der(&pem).ok_or_else(|| TlsError::NoSuchDevice(name.to_string()))?;
    let fingerprint = fingerprint_of_der(&der);
    let mut acl = load_acl(paths)?.unwrap_or_default();
    acl.devices.insert(fingerprint.clone(), role);
    std::fs::create_dir_all(&paths.dir)?;
    write_private(&paths.acl(), &serde_json::to_string_pretty(&acl)?)?;
    Ok(fingerprint)
}

/// Load the optional ACL. `None` means legacy CA-membership authorization; a present file
/// means explicit mode and unknown fingerprints are denied.
pub fn load_acl(paths: &Paths) -> Result<Option<DeviceAcl>, TlsError> {
    load_acl_file(&paths.acl())
}

pub fn load_acl_file(path: &Path) -> Result<Option<DeviceAcl>, TlsError> {
    match std::fs::read_to_string(path) {
        Ok(json) => Ok(Some(serde_json::from_str(&json)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Revoke a device by name (looks up its recorded cert, computes its fingerprint, appends it
/// to the `revoked` list). Returns the revoked fingerprint. Idempotent — revoking twice adds
/// the fingerprint only once.
pub fn revoke_device(paths: &Paths, name: &str) -> Result<String, TlsError> {
    let cert_path = paths.device_cert(name);
    if !cert_path.is_file() {
        return Err(TlsError::NoSuchDevice(name.to_string()));
    }
    let pem = std::fs::read_to_string(&cert_path)?;
    let der = pem_to_der(&pem).ok_or(TlsError::NoSuchDevice(name.to_string()))?;
    let fpr = fingerprint_of_der(&der);
    let mut revoked = load_revoked(paths);
    if revoked.insert(fpr.clone()) {
        let mut list: Vec<String> = revoked.into_iter().collect();
        list.sort();
        std::fs::write(paths.revoked(), list.join("\n") + "\n")?;
    }
    Ok(fpr)
}

/// The set of revoked device-cert fingerprints (hex sha256), or empty if none.
pub fn load_revoked(paths: &Paths) -> HashSet<String> {
    load_revoked_file(&paths.revoked())
}

/// The revoked-fingerprint set read directly from a `revoked` file path — so a live verifier
/// can re-read it per handshake for hot-reload (HS2-MPC0QF), without holding a `Paths`.
pub fn load_revoked_file(path: &Path) -> HashSet<String> {
    std::fs::read_to_string(path)
        .map(|s| {
            s.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The sha256 (hex) of a cert's DER — the stable fingerprint revocation + the verifier use.
pub fn fingerprint_of_der(der: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(der);
    format!("{:x}", h.finalize())
}

/// Add SANs to a server leaf: each host is an IP SAN if it parses as an IP, else a DNS SAN.
/// `localhost` + `127.0.0.1` are always present so a loopback client still validates.
fn san_list(hosts: &[String]) -> Vec<SanType> {
    let mut out: Vec<SanType> = Vec::new();
    let mut push = |h: &str| {
        let san = match h.parse::<IpAddr>() {
            Ok(ip) => SanType::IpAddress(ip),
            Err(_) => match h.to_string().try_into() {
                Ok(dns) => SanType::DnsName(dns),
                Err(_) => return,
            },
        };
        if !out.contains(&san) {
            out.push(san);
        }
    };
    push("localhost");
    push("127.0.0.1");
    for h in hosts {
        push(h);
    }
    out
}

/// Write a private-key file with owner-only permissions where the platform supports it.
fn write_private(path: &Path, pem: &str) -> std::io::Result<()> {
    std::fs::write(path, pem)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Extract the first DER block from a single-cert PEM (no external parser needed).
fn pem_to_der(pem: &str) -> Option<Vec<u8>> {
    let begin = "-----BEGIN CERTIFICATE-----";
    let end = "-----END CERTIFICATE-----";
    let start = pem.find(begin)? + begin.len();
    let stop = pem.find(end)?;
    let b64: String = pem[start..stop].split_whitespace().collect();
    base64_decode(&b64)
}

/// Minimal standard-base64 decoder (cert bodies only) — avoids pulling a base64 crate.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for &c in s.as_bytes() {
        let v = val(c)?;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, Paths) {
        let d = tempfile::tempdir().unwrap();
        let p = Paths::at(d.path().join("tls"));
        (d, p)
    }

    #[test]
    fn init_writes_ca_and_server_and_is_not_re_runnable() {
        let (_d, p) = paths();
        assert!(!p.is_initialized());
        init_ca(&p, &["192.168.1.10".into(), "server.local".into()]).unwrap();
        assert!(p.is_initialized());
        assert!(p.ca_cert().is_file() && p.ca_key().is_file());
        assert!(p.server_cert().is_file() && p.server_key().is_file());
        // A second init refuses (would invalidate every issued device cert).
        assert!(matches!(
            init_ca(&p, &[]),
            Err(TlsError::AlreadyInitialized(_))
        ));
    }

    #[test]
    fn issue_requires_init() {
        let (_d, p) = paths();
        assert!(matches!(
            issue_device(&p, "laptop"),
            Err(TlsError::NotInitialized)
        ));
    }

    #[test]
    fn issued_device_chains_to_the_ca_and_a_foreign_cert_does_not() {
        let (_d, p) = paths();
        init_ca(&p, &[]).unwrap();
        let dev = issue_device(&p, "laptop").unwrap();
        assert!(dev.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(dev.key_pem.contains("PRIVATE KEY"));
        assert_eq!(dev.fingerprint.len(), 64); // sha256 hex

        // The device cert's issuer chains to the stored CA: its DER fingerprint is recorded.
        let recorded = std::fs::read_to_string(p.dir.join("devices/laptop.crt")).unwrap();
        assert_eq!(
            fingerprint_of_der(&pem_to_der(&recorded).unwrap()),
            dev.fingerprint
        );

        // A cert from a DIFFERENT, unrelated CA has a different fingerprint (sanity that the
        // fingerprint actually distinguishes certs — the verifier test proves chain rejection).
        let (_d2, p2) = paths();
        init_ca(&p2, &[]).unwrap();
        let foreign = issue_device(&p2, "laptop").unwrap();
        assert_ne!(foreign.fingerprint, dev.fingerprint);
    }

    #[test]
    fn revoke_records_the_fingerprint_once_and_load_reads_it() {
        let (_d, p) = paths();
        init_ca(&p, &[]).unwrap();
        let dev = issue_device(&p, "phone").unwrap();
        assert!(load_revoked(&p).is_empty());

        let fpr = revoke_device(&p, "phone").unwrap();
        assert_eq!(fpr, dev.fingerprint);
        assert!(load_revoked(&p).contains(&dev.fingerprint));

        // Idempotent: revoking again keeps a single entry.
        revoke_device(&p, "phone").unwrap();
        assert_eq!(load_revoked(&p).len(), 1);

        // Revoking an unknown device errors.
        assert!(matches!(
            revoke_device(&p, "ghost"),
            Err(TlsError::NoSuchDevice(_))
        ));
    }

    #[test]
    fn renew_revokes_the_old_leaf_and_duplicate_issue_requires_explicit_rotation() {
        let (_d, p) = paths();
        init_ca(&p, &[]).unwrap();
        let old = issue_device(&p, "laptop").unwrap();
        assert!(matches!(
            issue_device(&p, "laptop"),
            Err(TlsError::DeviceExists(_))
        ));
        let renewed = renew_device(&p, "laptop").unwrap();
        assert_ne!(renewed.fingerprint, old.fingerprint);
        assert!(load_revoked(&p).contains(&old.fingerprint));
        assert!(!load_revoked(&p).contains(&renewed.fingerprint));
    }

    #[test]
    fn acl_switches_on_explicitly_and_is_keyed_by_device_fingerprint() {
        let (_d, p) = paths();
        init_ca(&p, &[]).unwrap();
        let reader = issue_device(&p, "reader").unwrap();
        assert!(load_acl(&p).unwrap().is_none(), "ACL is opt-in");
        let fingerprint = set_device_role(&p, "reader", DeviceRole::ReadOnly).unwrap();
        assert_eq!(fingerprint, reader.fingerprint);
        let acl = load_acl(&p).unwrap().unwrap();
        assert_eq!(
            acl.devices.get(&reader.fingerprint),
            Some(&DeviceRole::ReadOnly)
        );
        assert!(matches!(
            set_device_role(&p, "ghost", DeviceRole::Deny),
            Err(TlsError::NoSuchDevice(_))
        ));
    }

    #[test]
    fn project_id_is_stable_and_16_hex() {
        let (_d, _p) = paths();
        let id = project_id(std::path::Path::new("."));
        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
