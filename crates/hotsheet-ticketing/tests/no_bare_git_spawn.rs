//! HS2-RRD417 lint: every `git` subprocess under `crates/` goes through
//! `hotsheet_ticketing::git::command()`, which strips the inherited repository-locating
//! `GIT_*` environment. A bare `Command::new` of git anywhere else could let a hook's
//! `GIT_DIR` redirect Hot Sheet into the wrong repository.

use std::path::{Path, PathBuf};

/// The only file allowed to spawn `git` directly.
const SANCTIONED: &str = "hotsheet-ticketing/src/git.rs";

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
fn no_bare_git_command_outside_the_shared_constructor() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    rust_sources(crates, &mut files);
    assert!(files.len() > 50, "scanned too few sources: {}", files.len());

    // Built at runtime so this file does not match itself.
    let needle = format!("Command::new({:?})", "git");
    let mut offenders = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(crates)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if rel.replace('\\', "/") == SANCTIONED {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        // Ignore whitespace so padded and line-wrapped spellings count too.
        let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.contains(&needle) {
            offenders.push(rel);
        }
    }
    assert!(
        offenders.is_empty(),
        "spawn git via hotsheet_ticketing::git::command() (or command_in), not a bare \
         {needle}: {offenders:?}"
    );
}

/// Crates whose production code launches processes for users and agents (HS2-J79CZF).
const LAUNCHING_CRATES: &[&str] = &[
    "hotsheet-aitools/src",
    "hotsheet-server/src",
    "hotsheet-cli/src",
    "hotsheet-terminals/src",
];

/// Signal-only helpers that never run git or user code, so they may inherit the env.
const ALLOWED_BARE: &[&str] = &["\"kill\"", "\"taskkill\""];

/// `text` with every inline `#[cfg(test)]` / `#[cfg(all(test, ...))]` module body removed.
fn without_test_modules(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("#[cfg(") {
        let attribute_end = rest[at..].find(']').map_or(rest.len(), |end| at + end + 1);
        let attribute = &rest[at..attribute_end];
        let after = rest[attribute_end..].trim_start();
        if !(attribute.contains("test") && after.starts_with("mod ")) {
            out.push_str(&rest[..attribute_end]);
            rest = &rest[attribute_end..];
            continue;
        }
        out.push_str(&rest[..at]);
        let body = rest.len() - after.len();
        let Some(open) = rest[body..].find(['{', ';']).map(|i| body + i) else {
            return out;
        };
        if rest.as_bytes()[open] == b';' {
            // `mod name_tests;` lives in its own file, which the scan reads separately.
            rest = &rest[open + 1..];
            continue;
        }
        let mut depth = 0usize;
        let mut close = rest.len();
        for (offset, ch) in rest[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = open + offset + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = &rest[close..];
    }
    out.push_str(rest);
    out
}

#[test]
fn test_module_stripping_keeps_production_code() {
    let source = "fn a() {}\n#[cfg(test)]\nmod tests { fn b() { x(); } }\nfn c() {}\n\
                  #[cfg(all(test, unix))]\nmod more { }\n#[cfg(unix)]\nfn d() {}\n";
    let kept = without_test_modules(source);
    assert!(kept.contains("fn a()") && kept.contains("fn c()") && kept.contains("fn d()"));
    assert!(!kept.contains("fn b()") && !kept.contains("mod more"));
}

#[test]
fn launching_crates_start_processes_through_the_scrubbed_launch_constructor() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for dir in LAUNCHING_CRATES {
        rust_sources(&crates.join(dir), &mut files);
    }
    assert!(files.len() > 20, "scanned too few sources: {}", files.len());

    // Built at runtime so this file does not match itself.
    let needle = format!("{}::new(", "Command");
    let mut offenders = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        // Production code only: inline test modules may spawn fixtures directly.
        let compact: String = without_test_modules(&text)
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        for (at, _) in compact.match_indices(&needle) {
            // `CommandBuilder::new(` (the PTY) scrubs through `scrub_env` instead.
            if compact[..at].ends_with("Builder") {
                continue;
            }
            let rest = &compact[at + needle.len()..];
            if ALLOWED_BARE.iter().any(|allowed| rest.starts_with(allowed)) {
                continue;
            }
            offenders.push(format!(
                "{}: {}",
                file.strip_prefix(crates).unwrap().display(),
                &compact[at..(at + needle.len() + 24).min(compact.len())]
            ));
        }
    }
    assert!(
        offenders.is_empty(),
        "launch processes via hotsheet_ticketing::git::launch() so inherited GIT_DIR and \
         friends are dropped (HS2-J79CZF): {offenders:?}"
    );
}
