//! **The plugin-first lint** (HS2-9, docs/05 §5.1): core code must never branch on a
//! specific tool's *identity*. Tool-specific behavior belongs in the plugin (its manifest +
//! capabilities), so the host stays generic over the registry — the whole point of the
//! rewrite (HS1's `if (tool === "codex")` sprawl is what we're undoing).
//!
//! This is a source-scan guard: it derives the forbidden ids from the live registry
//! (`builtin_plugins()`), so a **new tool is covered automatically**, and fails if any
//! non-test source outside the plugin crate compares against or matches a tool id
//! (`== "codex"`, `"claude" =>`, `.id() == "antigravity"`, …). It passes today — its job is
//! to keep it that way.

use std::path::{Path, PathBuf};

use hotsheet_plugins::builtin_plugins;

#[test]
fn no_core_code_branches_on_a_tool_id() {
    let crates_dir = workspace_root().join("crates");
    let ids: Vec<String> = builtin_plugins()
        .iter()
        .map(|p| p.id().to_string())
        .collect();
    assert!(
        !ids.is_empty(),
        "expected at least one registered plugin id"
    );

    // A branch on a tool id looks like a comparison or a match arm against the id literal.
    let forbidden: Vec<String> = ids
        .iter()
        .flat_map(|id| [format!("== \"{id}\""), format!("\"{id}\" =>")])
        .collect();

    let mut violations = Vec::new();
    for file in rust_sources(&crates_dir) {
        // The plugin crate legitimately names tool ids (it IS the registry); tests and
        // fixtures name them freely. Only *core production code* is constrained.
        if is_in_plugins_crate(&file) || is_test_path(&file) {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        // Strip any inline `#[cfg(test)]` module (by convention it's the file's tail).
        let prod = match text.find("#[cfg(test)]") {
            Some(i) => &text[..i],
            None => &text,
        };
        for (n, line) in prod.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue; // comments may mention a tool by name
            }
            for pat in &forbidden {
                if line.contains(pat.as_str()) {
                    violations.push(format!(
                        "{}:{}: branches on a tool id (`{}`)",
                        file.strip_prefix(workspace_root())
                            .unwrap_or(&file)
                            .display(),
                        n + 1,
                        pat
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "core code must not branch on a tool id — move it into the plugin (docs/05 §5.1):\n  {}",
        violations.join("\n  ")
    );
}

/// Proves the guard has teeth: the forbidden patterns actually match a real tool-id branch
/// (so a green run above means "clean", not "no-op").
#[test]
fn the_lint_would_catch_a_tool_id_branch() {
    let id = &builtin_plugins()[0].id().to_string();
    let forbidden = [format!("== \"{id}\""), format!("\"{id}\" =>")];
    let offending_comparison = format!("    if tool == \"{id}\" {{");
    let offending_match = format!("        \"{id}\" => run_codex(),");
    assert!(
        forbidden
            .iter()
            .any(|p| offending_comparison.contains(p.as_str())),
        "the comparison form must be detected"
    );
    assert!(
        forbidden
            .iter()
            .any(|p| offending_match.contains(p.as_str())),
        "the match-arm form must be detected"
    );
}

/// The workspace root (this test's crate is `crates/hotsheet-plugins`).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root resolves")
}

fn is_in_plugins_crate(file: &Path) -> bool {
    file.components()
        .any(|c| c.as_os_str() == "hotsheet-plugins")
}

fn is_test_path(file: &Path) -> bool {
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name == "tests.rs"
        || name.ends_with("_tests.rs")
        || file.components().any(|c| c.as_os_str() == "tests")
}

/// Every `.rs` file under `crates/*/src` (recursive).
fn rust_sources(crates_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(crate_dirs) = std::fs::read_dir(crates_dir) else {
        return out;
    };
    for c in crate_dirs.flatten() {
        collect_rs(&c.path().join("src"), &mut out);
    }
    out
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}
