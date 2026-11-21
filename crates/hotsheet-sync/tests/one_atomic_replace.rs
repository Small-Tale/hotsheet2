//! HS2-J5DCJH lint: every `tempfile` publication under `crates/` goes through
//! `hotsheet_sync::files::persist`. `NamedTempFile::persist` renames with `MoveFileExW`,
//! which fails on Windows with `Access is denied` while any reader holds the target open;
//! the shared helper renames with POSIX semantics and retries a transient sharing failure.
//!
//! HS2-13KK94 lint: product code renames a file over another only through
//! `hotsheet_sync::files::replace`, which adds a bounded retry of a transient Windows sharing
//! failure. A direct `std::fs::rename` is allowed only at the sanctioned sites below, which
//! move directories or claim files rather than publish a staged file. Test code is exempt.

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

/// Direct renames that do not replace a published file, with the reason each stays.
const SANCTIONED_RENAMES: &[(&str, &str, &str)] = &[
    (
        "hotsheet-sync/src/files.rs",
        "|from, to| std::fs::rename(from, to)",
        "the helper itself",
    ),
    (
        "hotsheet-ticketing/src/sync_stale_locks.rs",
        "std::fs::rename(lock, &to)",
        "moves a stale Git lock aside; NotFound means another process took it",
    ),
    (
        "hotsheet-ticketing/src/screensaver.rs",
        "fs::rename(&staged, &destination)",
        "moves a staged bundle directory into place",
    ),
    (
        "hotsheet-ticketing/src/store.rs",
        "fs::rename(stage_root.join(item.id.to_string()), &final_dir)",
        "moves a staged attachment directory into place",
    ),
    (
        "hotsheet-ticketing/src/store.rs",
        "fs::rename(source, dir.join(&name))",
        "renames an attachment to a new name; it replaces nothing",
    ),
    (
        "hotsheet-aitools/src/launch_safety.rs",
        "std::fs::rename(storage, &runtime)",
        "moves a CODEX_HOME directory",
    ),
    (
        "hotsheet-cli/src/orchestration/validation_output.rs",
        "fs::rename(&self.temporary, &target)",
        "keeps output under its digest only when no copy exists",
    ),
    (
        "hotsheet-cli/src/orchestration/validation_runner.rs",
        "fs::rename(&spec_path, &taken)",
        "claims a spec: exactly one runner's rename may succeed",
    ),
    (
        "test-support/workspace_bin.rs",
        "std::fs::rename(&staging, snapshot)",
        "publishes a test binary snapshot directory",
    ),
];

/// The lines of `source` outside `#[cfg(test)]` modules (brace-counted).
fn product_lines(source: &str) -> Vec<(usize, &str)> {
    let mut lines = Vec::new();
    let mut pending_test_attribute = false;
    let mut skip_depth: Option<i64> = None;
    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(depth) = skip_depth.as_mut() {
            *depth += line.matches('{').count() as i64 - line.matches('}').count() as i64;
            if *depth <= 0 {
                skip_depth = None;
            }
            continue;
        }
        if trimmed == "#[cfg(test)]" {
            pending_test_attribute = true;
            continue;
        }
        if pending_test_attribute && trimmed.starts_with("mod ") && trimmed.ends_with('{') {
            pending_test_attribute = false;
            skip_depth = Some(1);
            continue;
        }
        if pending_test_attribute && !trimmed.starts_with("#[") {
            pending_test_attribute = false;
        }
        lines.push((index + 1, line));
    }
    lines
}

fn is_test_source(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    rel.contains("/tests/")
        || name == "tests.rs"
        || name.ends_with("_tests.rs")
        || name.starts_with("tests_")
}

#[test]
fn hs2_13kk94_files_are_replaced_only_through_the_shared_helper() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    rust_sources(crates, &mut files);
    // Built at runtime so this file does not match itself.
    let call = format!("fs::{}(", "rename");
    let mut offenders = Vec::new();
    let mut used = vec![false; SANCTIONED_RENAMES.len()];
    for file in files {
        let rel = file
            .strip_prefix(crates)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if is_test_source(&rel) {
            continue;
        }
        let source = std::fs::read_to_string(&file).unwrap();
        for (number, line) in product_lines(&source) {
            let code = line.split("//").next().unwrap_or("");
            if !code.contains(&call) {
                continue;
            }
            match SANCTIONED_RENAMES
                .iter()
                .position(|(path, needle, _)| *path == rel && code.contains(needle))
            {
                Some(index) => used[index] = true,
                None => offenders.push(format!("{rel}:{number}: {}", line.trim())),
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "publish a staged file with hotsheet_sync::files::replace, or sanction a directory \
         move or claim in SANCTIONED_RENAMES with its reason (HS2-13KK94):\n{}",
        offenders.join("\n")
    );
    let stale: Vec<_> = SANCTIONED_RENAMES
        .iter()
        .zip(&used)
        .filter(|(_, used)| !**used)
        .map(|((path, needle, _), _)| format!("{path}: {needle}"))
        .collect();
    assert!(
        stale.is_empty(),
        "sanctioned renames no longer in the source; remove them:\n{}",
        stale.join("\n")
    );
}

#[test]
fn hs2_13kk94_the_scan_skips_test_modules_only() {
    let source = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn b() { let _ = 1; }\n}\nfn c() {}\n";
    let kept: Vec<_> = product_lines(source).into_iter().map(|(n, _)| n).collect();
    assert_eq!(kept, [1, 6]);
    // A cfg(test) item that is not a module is skipped only for that line's attribute.
    let item = "#[cfg(test)]\nfn helper() {}\nfn d() {}\n";
    let kept: Vec<_> = product_lines(item).into_iter().map(|(n, _)| n).collect();
    assert_eq!(kept, [2, 3]);
    assert!(is_test_source("hotsheet-server/tests/http.rs"));
    assert!(is_test_source(
        "hotsheet-cli/src/coordination/speculation_tests.rs"
    ));
    assert!(!is_test_source("hotsheet-server/src/watcher.rs"));
}
