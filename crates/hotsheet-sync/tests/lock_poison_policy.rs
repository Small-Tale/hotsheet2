//! HS2-GSW2AA lint: production Rust never panics on a poisoned lock.
//!
//! `hotsheet_sync` holds the single policy (HS2-ZGQJZP): recover a poisoned lock and keep
//! serving. A `.lock().unwrap()`, `.read().unwrap()`, `.write().unwrap()` or a
//! `Condvar::wait*(..).unwrap()` in a server handler or a long-lived thread turns one panic
//! into a panic in every later caller. Production code uses `LockExt::lock_or_recover`,
//! `RwLockExt::{read_or_recover, write_or_recover}` and `CondvarExt::wait*_or_recover`.
//!
//! Test code may unwrap. This lint skips `tests/` directories, files whose name mentions
//! `test`, items marked `#[cfg(test)]` (including out-of-line `#[cfg(test)] mod x;` files),
//! and the known exceptions below.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Production files allowed a poisoning unwrap. Each entry says why; the list only shrinks.
const ALLOWED: &[(&str, &str)] = &[(
    "hotsheet-resource-claims/src/conformance.rs",
    "conformance suite run only by implementations' tests",
)];

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if !name.starts_with('.') && !name.contains("test") {
                rust_sources(&path, out);
            }
        } else if name.ends_with(".rs") && !name.contains("test") {
            out.push(path);
        }
    }
}

/// Is `line` an attribute that compiles its item only under test?
fn test_only_attribute(line: &str) -> bool {
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    compact.starts_with("#[cfg(test)]")
        || compact.starts_with("#[cfg(any(test,")
        || compact.starts_with("#[cfg(all(test,")
}

/// The source with every `#[cfg(test)]` item blanked out, plus the names of out-of-line
/// test-only modules (`#[cfg(test)] mod name;`). Relies on rustfmt layout: an item's
/// closing brace sits alone at the item's own indentation.
fn production_text(source: &str) -> (String, Vec<String>) {
    let lines: Vec<&str> = source.lines().collect();
    let mut kept = Vec::with_capacity(lines.len());
    let mut test_modules = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !test_only_attribute(lines[i]) {
            kept.push(lines[i]);
            i += 1;
            continue;
        }
        // Skip the attribute and any further attributes or comments before the item.
        i += 1;
        while i < lines.len() {
            let next = lines[i].trim_start();
            if next.starts_with("#[") || next.starts_with("//") {
                i += 1;
            } else {
                break;
            }
        }
        let Some(item) = lines.get(i) else { break };
        let indent = item.len() - item.trim_start().len();
        let trimmed = item.trim();
        if trimmed.ends_with(';') {
            if let Some(name) = trimmed
                .trim_start_matches("pub(crate) ")
                .trim_start_matches("pub ")
                .strip_prefix("mod ")
            {
                test_modules.push(name.trim_end_matches(';').trim().to_owned());
            }
            i += 1;
            continue;
        }
        if trimmed.ends_with('{') {
            let close = format!("{}}}", " ".repeat(indent));
            i += 1;
            while i < lines.len() && !lines[i].starts_with(&close) {
                i += 1;
            }
            i += 1;
            continue;
        }
        // A multi-line signature or a one-line item: skip to the line closing it.
        while i < lines.len() {
            let line = lines[i];
            i += 1;
            if line.len() - line.trim_start().len() == indent
                && (line.trim_end().ends_with('}') || line.trim_end().ends_with(';'))
            {
                break;
            }
        }
    }
    (kept.join("\n"), test_modules)
}

/// Drop line comments so prose mentioning the pattern is not a finding.
fn strip_comments(text: &str) -> String {
    text.lines()
        .map(|line| match line.find("//") {
            Some(at) if !line[..at].contains('"') => &line[..at],
            _ => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every poisoning unwrap in `text`, as whitespace-free snippets.
fn findings(text: &str) -> Vec<String> {
    let compact: String = strip_comments(text)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let mut found = Vec::new();
    for guard in [".lock()", ".read()", ".write()"] {
        for tail in [".unwrap()", ".expect("] {
            let needle = format!("{guard}{tail}");
            found.extend(compact.matches(needle.as_str()).map(str::to_owned));
        }
    }
    for wait in [
        ".wait(",
        ".wait_timeout(",
        ".wait_while(",
        ".wait_timeout_while(",
        ".wait_timeout_ms(",
    ] {
        let mut from = 0;
        while let Some(at) = compact[from..].find(wait) {
            let start = from + at;
            let mut depth = 0usize;
            let mut end = None;
            for (offset, c) in compact[start + wait.len() - 1..].char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(start + wait.len() - 1 + offset + 1);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let Some(end) = end else { break };
            let rest = &compact[end..];
            if rest.starts_with(".unwrap()") || rest.starts_with(".expect(") {
                found.push(compact[start..end.min(start + 80)].to_owned());
            }
            from = end;
        }
    }
    found
}

/// Files that a `#[cfg(test)] mod name;` declaration in `file` compiles only under test.
fn test_module_paths(file: &Path, names: &[String]) -> Vec<PathBuf> {
    let parent = file.parent().unwrap();
    let stem = file.file_stem().unwrap().to_str().unwrap();
    let base = if matches!(stem, "mod" | "lib" | "main") {
        parent.to_path_buf()
    } else {
        parent.join(stem)
    };
    names
        .iter()
        .flat_map(|name| [base.join(format!("{name}.rs")), base.join(name)])
        .collect()
}

#[test]
fn hs2_gsw2aa_production_code_never_panics_on_a_poisoned_lock() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(crates).unwrap() {
        rust_sources(&entry.unwrap().path().join("src"), &mut files);
    }
    files.sort();
    let mut test_only: BTreeSet<PathBuf> = BTreeSet::new();
    let mut texts = Vec::new();
    for file in &files {
        let source = std::fs::read_to_string(file).unwrap();
        let (text, modules) = production_text(&source);
        test_only.extend(test_module_paths(file, &modules));
        texts.push((file, text));
    }
    let mut violations = Vec::new();
    let mut allowed_seen = BTreeSet::new();
    for (file, text) in texts {
        if test_only.iter().any(|skip| file.starts_with(skip)) {
            continue;
        }
        let relative = file
            .strip_prefix(crates)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let found = findings(&text);
        if found.is_empty() {
            continue;
        }
        if ALLOWED.iter().any(|(path, _)| *path == relative) {
            allowed_seen.insert(relative);
            continue;
        }
        violations.push(format!("{relative}: {}", found.join(", ")));
    }
    assert!(
        violations.is_empty(),
        "use LockExt/RwLockExt/CondvarExt *_or_recover instead (HS2-GSW2AA):\n{}",
        violations.join("\n")
    );
    for (path, why) in ALLOWED {
        assert!(
            allowed_seen.contains(*path),
            "{path} no longer needs its exception ({why}); delete the entry"
        );
    }
}

#[test]
fn hs2_gsw2aa_the_scanner_finds_each_shape_and_skips_test_items() {
    let source = r#"
fn production(lock: &Mutex<u8>, cvar: &Condvar) {
    let a = lock.lock().unwrap();
    let b = lock
        .lock()
        .expect("poisoned");
    let c = cvar.wait_timeout(a, Duration::from_millis(1)).unwrap();
    let d = cvar
        .wait_timeout_while(b, timeout, |value| *value == 0)
        .unwrap();
    // A comment naming .lock().unwrap() is not a finding.
    let fine = lock.lock_or_recover();
}

#[cfg(test)]
mod tests {
    fn test_only(lock: &Mutex<u8>) {
        let _ = lock.lock().unwrap();
    }
}

#[cfg(test)]
mod fixtures;
"#;
    let (text, modules) = production_text(source);
    let found = findings(&text);
    assert_eq!(found.len(), 4, "{found:?}");
    assert_eq!(modules, ["fixtures"]);
}
