//! HS2-ECJ188 lint: every canonicalization under `crates/` goes through
//! `hotsheet_sync::paths::canonicalize`. The standard library's returns a Windows
//! verbatim path (`\\?\C:\...`) that Git rejects and that hashes to a different checkout
//! id than the ordinary spelling of the same directory (HS2-CTV942).

use std::path::{Path, PathBuf};

/// The only file allowed to call the standard canonicalization.
const SANCTIONED: &str = "hotsheet-sync/src/paths.rs";

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
fn hs2_ecj188_no_canonicalize_outside_the_shared_path_helper() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    rust_sources(crates, &mut files);
    assert!(files.len() > 50, "scanned too few sources: {}", files.len());

    // Built at runtime so this file does not match itself.
    let method = format!(".{}()", "canonicalize");
    let function = format!("fs::{}(", "canonicalize");
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
            if code.contains(&method) || code.contains(&function) {
                offenders.push(format!("{rel}:{}: {}", index + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "use hotsheet_sync::paths::canonicalize instead (HS2-ECJ188):\n{}",
        offenders.join("\n")
    );
}
