//! HS2-1D4DQF policy: production code reads `HOTSHEET_HOME` only through
//! `hotsheet_ticketing::machine_home`. Separate copies of the resolver once turned a blank
//! value into the working directory, so the CLI wrote its SQLite `index/` into checkouts.

use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Test-only sources may read the variable directly to find the harness home.
fn is_test_source(path: &Path) -> bool {
    let name = path.file_name().unwrap().to_string_lossy();
    name == "tests.rs"
        || name.ends_with("_tests.rs")
        || path
            .components()
            .any(|part| part.as_os_str().to_string_lossy().ends_with("tests"))
}

#[test]
fn hs2_1d4dqf_only_machine_home_reads_hotsheet_home() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for krate in std::fs::read_dir(crates).unwrap() {
        let src = krate.unwrap().path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    assert!(
        files.len() > 100,
        "scanned too few sources: {}",
        files.len()
    );

    let patterns = ["var_os(\"HOTSHEET_HOME\")", "var(\"HOTSHEET_HOME\")"];
    let offenders: Vec<String> = files
        .iter()
        .filter(|path| !path.ends_with("hotsheet-ticketing/src/machine_home.rs"))
        .filter(|path| !is_test_source(path.strip_prefix(crates).unwrap()))
        .filter(|path| {
            let text = std::fs::read_to_string(path).unwrap();
            patterns.iter().any(|pattern| text.contains(pattern))
        })
        .map(|path| path.strip_prefix(crates).unwrap().display().to_string())
        .collect();
    assert!(
        offenders.is_empty(),
        "resolve HOTSHEET_HOME with hotsheet_ticketing::machine_home, not directly: {offenders:?}"
    );
}
