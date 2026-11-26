//! HS2-R8ZH4K policy: every production site that applies the AI completion rule
//! (`actor::check_completion`) also applies the remote-commit rule
//! (`completion_remote`), so an AI cannot complete a ticket whose commits exist only
//! locally through one surface while another refuses it. Provider-backed completion sites
//! are exempt until HS2-Z0VYZ5 wires them; the exemption list only shrinks.

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

fn is_test_source(path: &Path) -> bool {
    let name = path.file_name().unwrap().to_string_lossy();
    name == "tests.rs" || name.ends_with("_tests.rs")
}

/// Production source before its `#[cfg(test)]` module, if any.
fn production(text: &str) -> &str {
    text.find("#[cfg(test)]\nmod tests")
        .map_or(text, |end| &text[..end])
}

/// Provider-backed completion paths, wired by HS2-Z0VYZ5.
const EXEMPT: &[&str] = &[
    "hotsheet-cli/src/dispatch/providers.rs",
    "hotsheet-server/src/routes/providers/tickets.rs",
];

#[test]
fn hs2_r8zh4k_every_ai_completion_site_checks_remote_commits() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for krate in std::fs::read_dir(crates).unwrap() {
        let src = krate.unwrap().path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    let mut missing = Vec::new();
    let mut exempt_seen = Vec::new();
    for file in files.iter().filter(|file| !is_test_source(file)) {
        let relative = file
            .strip_prefix(crates)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if relative == "hotsheet-ticketing/src/actor.rs" {
            continue;
        }
        let text = std::fs::read_to_string(file).unwrap();
        let text = production(&text);
        let completions = text.matches("actor::check_completion(").count();
        if completions == 0 {
            continue;
        }
        if EXEMPT.contains(&relative.as_str()) {
            exempt_seen.push(relative);
            continue;
        }
        let remote = text.matches("check_remote_commits(").count()
            - text.matches("fn check_remote_commits(").count();
        if remote < completions {
            missing.push(format!(
                "{relative}: {completions} completion rule site(s), {remote} remote-commit check(s)"
            ));
        }
    }
    assert!(missing.is_empty(), "HS2-R8ZH4K: {missing:#?}");
    let mut stale: Vec<_> = EXEMPT
        .iter()
        .filter(|path| !exempt_seen.iter().any(|seen| seen == *path))
        .collect();
    stale.sort();
    assert!(
        stale.is_empty(),
        "HS2-R8ZH4K: remove exemptions that no longer apply the completion rule: {stale:?}"
    );
}
