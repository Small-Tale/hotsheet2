//! HS2-4TCTEW policy: tests spawn workspace binaries only through the stable snapshot in
//! `crates/test-support/workspace_bin.rs`.
//!
//! The uplifted `target/<profile>/<name>` path disappears briefly whenever another Cargo
//! command runs against the same target directory. Spawning it directly
//! (`Command::cargo_bin` or the compile-time `CARGO_BIN_EXE_<name>` path) therefore fails
//! intermittently with `No such file or directory` under concurrent builds.

use std::path::{Path, PathBuf};

fn rust_sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn hs2_4tctew_tests_spawn_workspace_binaries_only_through_the_stable_snapshot() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let helper = crates.join("test-support/workspace_bin.rs");
    let this = Path::new(file!()).file_name().unwrap();
    // Assembled so this file does not match its own patterns.
    let banned = [concat!("cargo", "_bin("), concat!("CARGO_BIN", "_EXE")];
    let mut sources = Vec::new();
    for entry in std::fs::read_dir(crates).unwrap() {
        let dir = entry.unwrap().path();
        if dir.is_dir() {
            rust_sources(&dir, &mut sources);
        }
    }
    assert!(
        sources.len() > 100,
        "the scan must see the workspace sources"
    );
    let offenders = sources
        .iter()
        .filter(|path| **path != helper && path.file_name() != Some(this))
        .filter(|path| !path.components().any(|part| part.as_os_str() == "target"))
        .filter(|path| {
            let text = std::fs::read_to_string(path).unwrap();
            banned.iter().any(|pattern| text.contains(pattern))
        })
        .map(|path| path.strip_prefix(crates).unwrap().display().to_string())
        .collect::<Vec<_>>();
    assert!(
        offenders.is_empty(),
        "spawn workspace binaries with workspace_bin::path (HS2-4TCTEW), not directly: {offenders:?}"
    );
}
