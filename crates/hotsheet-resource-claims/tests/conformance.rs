//! HS2-322ZSZ: the built-in local backend passes the same conformance suite an external
//! plugin must (`hotsheet_resource_claims::conformance`). The Git-ref backend runs it from
//! its own unit tests, where the bare-remote fixtures live.

use std::sync::Arc;
use std::time::Duration;

use hotsheet_resource_claims::conformance::{self, Fixture};
use hotsheet_resource_claims::{ClaimKey, LocalBackend, Namespace, StaticKeyProvider, SystemClock};

#[test]
fn hs2_322zsz_the_local_backend_passes_the_conformance_suite() {
    let dir = tempfile::tempdir().unwrap();
    // The suite parks real waiters, and the local backend measures a wait's deadline on its
    // own clock, so it runs on the system clock and lets real time pass.
    let backend = LocalBackend::new(
        dir.path().join("resource-claims"),
        Arc::new(SystemClock),
        Arc::new(StaticKeyProvider::new(ClaimKey::from_bytes([5; 32]))),
    );
    conformance::run(&Fixture {
        backend: &backend,
        namespace: Namespace::parse("conformance").unwrap(),
        advance: &std::thread::sleep,
        short_lease: Duration::from_millis(900),
    });
}
