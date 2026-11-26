//! HS2-R8ZH4K policy: every production site that applies the AI completion rule
//! (`actor::check_completion`) also applies the remote-commit rule
//! (`completion_remote`), so an AI cannot complete a ticket whose commits exist only
//! locally through one surface while another refuses it. HS2-Z0VYZ5 wired the
//! provider-backed completion paths, so no file is exempt: the CLI provider commands and
//! the server provider routes are checked like every other completion site.

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
    let mut sites = Vec::new();
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
        sites.push(relative.clone());
        let remote = text.matches("check_remote_commits(").count()
            - text.matches("fn check_remote_commits(").count();
        if remote < completions {
            missing.push(format!(
                "{relative}: {completions} completion rule site(s), {remote} remote-commit check(s)"
            ));
        }
    }
    assert!(missing.is_empty(), "HS2-R8ZH4K: {missing:#?}");
}

/// HS2-Z0VYZ5: the provider-backed completion paths apply both rules rather than being
/// skipped, and the checkout-scoped provider close shares the provider close engine
/// instead of calling the provider directly.
#[test]
fn hs2_z0vyz5_provider_completion_paths_check_remote_commits() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let read = |relative: &str| std::fs::read_to_string(crates.join(relative)).unwrap();
    for (relative, sites) in [
        ("hotsheet-cli/src/dispatch/providers.rs", 2),
        ("hotsheet-server/src/routes/providers/tickets.rs", 2),
    ] {
        let text = read(relative);
        let text = production(&text);
        assert_eq!(
            text.matches("actor::check_completion(").count(),
            sites,
            "HS2-Z0VYZ5: {relative} completion rule sites"
        );
        assert_eq!(
            text.matches("check_remote_commits(").count()
                - text.matches("fn check_remote_commits(").count(),
            sites,
            "HS2-Z0VYZ5: {relative} remote-commit checks"
        );
        assert!(
            text.contains("completion_remote::record_provider("),
            "HS2-Z0VYZ5: {relative} records the no-commit opt-out through the provider"
        );
    }
    let checkout_routes = read("hotsheet-server/src/routes/checkout_tickets/mod.rs");
    assert!(
        !checkout_routes.contains(".close(\n"),
        "HS2-Z0VYZ5: the checkout close route must close provider tickets through \
         do_provider_close, which applies the completion rules"
    );
    assert!(checkout_routes.contains("do_provider_close("));
}
