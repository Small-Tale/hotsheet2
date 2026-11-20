//! HS2-J5DCJH lint: every `tempfile` publication under `crates/` goes through
//! `hotsheet_sync::files::persist`. `NamedTempFile::persist` renames with `MoveFileExW`,
//! which fails on Windows with `Access is denied` while any reader holds the target open;
//! the shared helper renames with POSIX semantics and retries a transient sharing failure.

use std::path::{Path, PathBuf};

/// The only file allowed to call `tempfile`'s own persistence.
const SANCTIONED: &str = "hotsheet-sync/src/files.rs";

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if name != "target" && !name.starts_with('.') {
                rust_sources(&path, out);
            }
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
}

#[test]
fn hs2_j5dcjh_no_tempfile_persist_outside_the_shared_replace_helper() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    rust_sources(crates, &mut files);
    assert!(files.len() > 50, "scanned too few sources: {}", files.len());

    // Built at runtime so this file does not match itself. `persist_noclobber` never
    // replaces a target, so it is not covered.
    let method = format!(".{}(", "persist");
    let mut offenders = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(crates)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel == SANCTIONED {
            continue;
        }
        let source = std::fs::read_to_string(&file).unwrap();
        for (index, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if code.contains(&method) {
                offenders.push(format!("{rel}:{}: {}", index + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "use hotsheet_sync::files::persist instead (HS2-J5DCJH):\n{}",
        offenders.join("\n")
    );
}
