//! HS2-XY0VTN: the checked-in `src/gen/` equals a fresh generation from `proto/`, so the
//! contract and its Rust types cannot drift. Runs under `--all-features` (the root gate
//! and CI) because it needs the vendored protoc.
#![cfg(feature = "regen")]

use std::path::Path;

use hotsheet_plugin_protocol::regen::{GENERATED_FILES, generate};

#[test]
fn hs2_xy0vtn_checked_in_contract_code_matches_a_fresh_generation() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = tempfile::tempdir().expect("temp dir");
    generate(crate_dir, out.path()).expect("generates");
    for file in GENERATED_FILES {
        let fresh = std::fs::read_to_string(out.path().join(file)).expect("generated");
        let checked_in =
            std::fs::read_to_string(crate_dir.join("src/gen").join(file)).expect("checked in");
        assert!(
            fresh == checked_in,
            "src/gen/{file} is stale; run `cargo run -p hotsheet-plugin-protocol --features regen --bin hotsheet-plugin-protocol-regen`"
        );
    }
}
