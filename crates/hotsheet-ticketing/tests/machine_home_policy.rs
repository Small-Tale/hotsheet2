//! HS2-1D4DQF policy: production code reads `HOTSHEET_HOME` only through
//! `hotsheet_ticketing::machine_home`. Separate copies of the resolver once turned a blank
//! value into the working directory, so the CLI wrote its SQLite `index/` into checkouts.
//!
//! HS2-15ZN8E extends it to the path overrides read by crates that do not depend on
//! `hotsheet-ticketing`: each read passes straight into a pure `resolve_*` function built on
//! the blank-as-unset rule (`hotsheet_sync::env_path`, or the desktop shell's local copy).

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

/// The path-valued overrides in `hotsheet-terminals` and the desktop shell, as string
/// literals and as the constants that name them.
const RESOLVED_OVERRIDES: [&str; 9] = [
    "\"HOTSHEET_LAUNCH_AGENTS_DIR\"",
    "\"HOTSHEET_LAUNCHCTL\"",
    "\"HOTSHEET_PROCESS_JOURNAL_HOME\"",
    "\"HOTSHEET_DESKTOP_STORAGE_DIRECTORY\"",
    "LAUNCH_AGENTS_DIR_ENV",
    "LAUNCHCTL_ENV",
    "PROCESS_JOURNAL_HOME_ENV",
    "STORAGE_DIRECTORY_ENV",
    "\"HOME\"",
];

#[test]
fn hs2_15zn8e_path_overrides_pass_through_a_blank_as_unset_resolver() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let repo = crates.parent().unwrap();
    let mut files = Vec::new();
    rust_files(&crates.join("hotsheet-terminals").join("src"), &mut files);
    rust_files(
        &repo
            .join("clients")
            .join("web")
            .join("src-tauri")
            .join("src"),
        &mut files,
    );
    assert!(files.len() > 10, "scanned too few sources: {}", files.len());

    let mut offenders = Vec::new();
    for path in files
        .iter()
        .filter(|path| !is_test_source(path.strip_prefix(repo).unwrap()))
    {
        let text = std::fs::read_to_string(path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            let Some(at) = line.find("var_os(") else {
                continue;
            };
            // rustfmt may wrap the argument onto the next line.
            let argument = format!(
                "{}{}",
                &line[at..],
                lines.get(index + 1).copied().unwrap_or_default()
            );
            if !RESOLVED_OVERRIDES
                .iter()
                .any(|name| argument.contains(name))
            {
                continue;
            }
            // The read is an argument to `resolve_*(` on this line or the two before it.
            let window = lines[index.saturating_sub(2)..=index].join("\n");
            if !window.contains("resolve_") {
                offenders.push(format!(
                    "{}:{}",
                    path.strip_prefix(repo).unwrap().display(),
                    index + 1
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "pass path overrides to a blank-as-unset resolve_* function (HS2-15ZN8E): {offenders:?}"
    );
}
