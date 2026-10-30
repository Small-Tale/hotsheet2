use super::{OsKeychain, SecretStore};

#[test]
fn hs2_1y45mz_scoped_secret_adapter_prepares_only_its_explicit_envelope_home() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    // NativeKeychain refuses real credential access in unit tests (or uses an explicitly
    // activated synthetic port). This unknown synthetic account never names a real key.
    let _ = OsKeychain::at(first.path()).get("hs2-1y45mz-synthetic-missing-account");
    assert!(first.path().join("machine-secrets").is_dir());
    assert!(!second.path().join("machine-secrets").exists());
    let _ = OsKeychain::at(second.path()).get("hs2-1y45mz-synthetic-missing-account");
    assert!(second.path().join("machine-secrets").is_dir());
}
