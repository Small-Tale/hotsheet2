use std::fs;

use hotsheet_ticketing::{FsStore, Scope, Settings, checkouts::CheckoutRegistry};

/// These bytes are append-only compatibility artifacts. At the first public release,
/// copy its complete emitted corpus into a new release-named fixture directory; never
/// mutate an older fixture to accommodate a newer reader.
#[test]
fn reads_retained_prerelease_store_project_and_settings_formats() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("tickets")).unwrap();
    fs::write(
        root.path().join("hotsheet-store.json"),
        include_str!("fixtures/compatibility/prerelease-store.json"),
    )
    .unwrap();
    fs::write(
        root.path().join("hotsheet-settings.json"),
        include_str!("fixtures/compatibility/prerelease-settings.json"),
    )
    .unwrap();
    let checkout_path = root.path().join("checkouts.json");
    fs::write(
        &checkout_path,
        include_str!("fixtures/compatibility/prerelease-checkouts.json"),
    )
    .unwrap();

    assert_eq!(
        FsStore::open(root.path())
            .unwrap()
            .metadata()
            .unwrap()
            .schema_version,
        1
    );
    assert_eq!(
        Settings::new(root.path()).map(Scope::Shared).unwrap()["theme_hint"],
        "dark"
    );
    assert!(
        CheckoutRegistry::new(checkout_path)
            .list()
            .unwrap()
            .is_empty()
    );
}
