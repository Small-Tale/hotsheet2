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
