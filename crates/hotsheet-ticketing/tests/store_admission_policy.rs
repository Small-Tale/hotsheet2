//! HS2-726EXJ policy: every operational entry point that selects a Git ticket store as a
//! source goes through the one shared admission check in `store_git.rs` (`admit_store`,
//! `FsStore::open_admitted`, or the registry's `admit_linked_git_source`). A new or
//! refactored entry point that opens a store directly would re-admit unowned,
//! repository-less, and unavailable stores, or the machine bootstrap anchor.

use std::path::{Path, PathBuf};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(workspace().join(relative))
        .unwrap_or_else(|error| panic!("reading {relative}: {error}"))
}

/// The body of the first function whose signature starts with `signature`, by brace
/// matching from the signature's opening brace.
fn body<'a>(source: &'a str, signature: &str, file: &str) -> &'a str {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("{file}: `{signature}` not found; update this policy"));
    let open = start + source[start..].find('{').unwrap();
    let mut depth = 0usize;
    for (offset, character) in source[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[start..=open + offset];
                }
            }
            _ => {}
        }
    }
    panic!("{file}: unbalanced body for `{signature}`");
}

/// `(file, function signature, the admission call its body must make)`.
const ENTRY_POINTS: &[(&str, &str, &str)] = &[
    (
        "crates/hotsheet-cli/src/lib.rs",
        "pub fn link_store(",
        "admit_store(",
    ),
    (
        "crates/hotsheet-ticketing/src/checkouts.rs",
        "fn admit_linked_git_source(",
        "admit_store(",
    ),
    (
        "crates/hotsheet-ticketing/src/checkouts.rs",
        "fn finish_registration(",
        "admit_linked_git_source(",
    ),
    (
        "crates/hotsheet-ticketing/src/checkouts.rs",
        "fn relink_git_source_inner(",
        "admit_linked_git_source(",
    ),
    (
        "crates/hotsheet-ticketing/src/worklist.rs",
        "pub fn regenerate_checkout(",
        "open_admitted(",
    ),
    (
        "crates/hotsheet-server/src/hosting.rs",
        "fn host_store_with_permission_loading(",
        "admit_store(",
    ),
    (
        "crates/hotsheet-cli/src/dispatch/providers.rs",
        "pub(crate) fn configured_provider(",
        "admit_store(",
    ),
    (
        "crates/hotsheet-mcp/src/lib.rs",
        "fn open_source(",
        "open_admitted(",
    ),
    (
        "crates/hotsheet-mcp/src/lib.rs",
        "pub fn open(path: &Path)",
        "open_admitted(",
    ),
    (
        "crates/hotsheet-mcp/src/lib.rs",
        "fn own_provider(",
        "is_machine_anchor(",
    ),
    (
        "crates/hotsheet-mcp/src/lib.rs",
        "fn open_dest(",
        "open_source(",
    ),
    (
        "crates/hotsheet-mcp/src/lib.rs",
        "fn claim_checkout_next(",
        "open_source(",
    ),
    (
        "crates/hotsheet-mcp/src/lib.rs",
        "fn checkout_context(",
        "open_source(",
    ),
];

#[test]
fn hs2_726exj_source_selection_entry_points_admit_their_store() {
    for (file, signature, call) in ENTRY_POINTS {
        let source = read(file);
        let body = body(&source, signature, file);
        assert!(
            body.contains(call),
            "{file}: `{signature}` must select its store through `{call}` (HS2-726EXJ)"
        );
        assert!(
            !body.contains("FsStore::open(") || *call == "admit_store(",
            "{file}: `{signature}` opens a store without admission (HS2-726EXJ)"
        );
    }
}

/// The serverless MCP backend opens every source through `open_source`; a direct
/// `FsStore::open(` in its production code would bypass admission.
#[test]
fn hs2_726exj_the_serverless_mcp_backend_never_opens_a_store_directly() {
    let source = read("crates/hotsheet-mcp/src/lib.rs");
    let start = source.find("mod core_backend {").unwrap();
    let production = &source[start..];
    let production = &production[..production.find("#[cfg(test)]").unwrap()];
    assert_eq!(
        production.matches("FsStore::open(").count(),
        0,
        "use `open_source` in the serverless MCP backend (HS2-726EXJ)"
    );
}

/// The server's primary store and setup's anchor exclusion use the store role rather than
/// a raw path comparison.
#[test]
fn hs2_726exj_the_primary_store_and_setup_use_the_store_role() {
    let main = read("crates/hotsheet-server/src/main.rs");
    assert!(main.contains("FsStore::open_admitted(\n        &cli.path,"));
    assert!(!main.contains("FsStore::open(&cli.path)"));
    let setup = read("crates/hotsheet-cli/src/dispatch/setup.rs");
    assert!(setup.contains("StoreRole::of(store)"));
    assert!(!setup.contains("store != home.join(\"server-bootstrap.hs2\")"));
}
