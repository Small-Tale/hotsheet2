//! **Setup tests stay off the shared machine home** (HS2-SDPD95). Cargo forces one
//! repository-local `HOTSHEET_HOME` (`target/test-hotsheet-home`, HS2-T9VXEP) on every test
//! process, so test binaries running in parallel share that directory. Setup discovers
//! machine plugins under `<HOTSHEET_HOME>/plugins`, so a test that runs setup through the
//! default search path reads state another binary may be changing.
//!
//! This source-scan guard makes every setup test inject its plugin search path:
//!
//! * test code never calls the default-path wrappers (`run_setup`, `refresh_setup`, `find`,
//!   `default_dirs()`), only the `*_in` variants with an explicit directory list;
//! * test code never repoints the process-wide `HOTSHEET_HOME` with `set_var`;
//! * a server test that sends `POST /setup/<tool>` builds its state with an injected
//!   plugin path (`hermetic_setup(..)` or `.with_plugin_dirs(..)`).
//!
//! Spawned `hotsheet-cli` processes are covered separately: the CLI test helpers pass a
//! per-test `HOTSHEET_HOME`.

use std::path::{Path, PathBuf};

/// Calls that resolve the machine plugin path from the process environment.
const DEFAULT_PATH_CALLS: &[&str] = &[
    "run_setup(",
    "refresh_setup(",
    "hotsheet_plugins::find(",
    "default_dirs()",
    "set_var(\"HOTSHEET_HOME\"",
];

/// Markers that show a server test injected its plugin search path.
const INJECTION_MARKERS: &[&str] = &["hermetic_setup(", "with_plugin_dirs("];

#[test]
fn setup_tests_inject_their_plugin_search_path() {
    let mut violations = Vec::new();
    for file in rust_files(&workspace_root().join("crates")) {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let Some((first_line, test_code)) = test_code(&file, &text) else {
            continue;
        };
        let shown = file
            .strip_prefix(workspace_root())
            .unwrap_or(&file)
            .display()
            .to_string();
        violations.extend(
            default_path_calls(test_code)
                .into_iter()
                .map(|(line, call)| {
                    format!(
                        "{shown}:{}: `{call}` reads the shared machine home; use the `_in` \
                         variant with an explicit plugin dir",
                        first_line + line - 1
                    )
                }),
        );
        violations.extend(
            uninjected_setup_requests(test_code)
                .into_iter()
                .map(|name| format!("{shown}: `{name}` posts /setup without hermetic_setup(..) or .with_plugin_dirs(..)")),
        );
    }
    assert!(
        violations.is_empty(),
        "setup tests must not use the shared machine plugin path (HS2-SDPD95):\n  {}",
        violations.join("\n  ")
    );
}

/// The guard has teeth: each forbidden form is reported, and the injected forms are not.
#[test]
fn the_guard_catches_default_path_setup_in_tests() {
    let offending = "\
        let reports = run_setup(d.path(), d.path(), Some(\"codex\"), false).unwrap();\n\
        hotsheet_cli::setup::refresh_setup(&store, &project).unwrap();\n\
        let p = hotsheet_plugins::find(\"mytool\").unwrap();\n\
        unsafe { std::env::set_var(\"HOTSHEET_HOME\", home.path()) };\n";
    let found: Vec<&str> = default_path_calls(offending)
        .into_iter()
        .map(|(_, call)| call)
        .collect();
    assert_eq!(
        found,
        [
            "run_setup(",
            "refresh_setup(",
            "hotsheet_plugins::find(",
            "set_var(\"HOTSHEET_HOME\""
        ]
    );
    let injected = "\
        run_setup_in(d.path(), d.path(), Some(\"codex\"), false, &[]).unwrap();\n\
        refresh_setup_in(&store, &project, &dirs).unwrap();\n\
        // run_setup(…) in a comment is fine\n";
    assert!(default_path_calls(injected).is_empty());

    let server_tests = "\
#[tokio::test]
async fn bare_setup() {
    let (_d, st) = state();
    app(st).oneshot(authed(\"POST\", \"/setup/codex\", None)).await;
}

#[tokio::test]
async fn injected_setup() {
    let (_d, st) = state();
    let st = hermetic_setup(st);
    app(st).oneshot(authed(\"POST\", \"/setup/codex\", None)).await;
}
";
    assert_eq!(uninjected_setup_requests(server_tests), ["bare_setup"]);
}

/// Each `(line, call)` where test code resolves the machine plugin path itself.
fn default_path_calls(code: &str) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    for (index, line) in code.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        for call in DEFAULT_PATH_CALLS {
            if line
                .match_indices(call)
                .any(|(at, _)| !is_identifier_tail(&line[..at]))
            {
                found.push((index + 1, *call));
            }
        }
    }
    found
}

/// Whether `prefix` ends inside an identifier, so `x_run_setup(` is not `run_setup(`.
fn is_identifier_tail(prefix: &str) -> bool {
    prefix
        .chars()
        .next_back()
        .is_some_and(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// Names of test functions that post `/setup/<tool>` without an injected plugin path.
fn uninjected_setup_requests(code: &str) -> Vec<String> {
    test_functions(code)
        .into_iter()
        .filter(|(_, body)| body.contains("\"/setup/") || body.contains(".uri(\"/setup/"))
        .filter(|(_, body)| !INJECTION_MARKERS.iter().any(|m| body.contains(m)))
        .map(|(name, _)| name)
        .collect()
}

/// Split source into `(name, text)` per top-level function, from one column-0 `fn` or
/// `async fn` to the next.
fn test_functions(code: &str) -> Vec<(String, String)> {
    let mut functions: Vec<(String, String)> = Vec::new();
    for line in code.lines() {
        let header = line
            .strip_prefix("async fn ")
            .or_else(|| line.strip_prefix("fn "));
        if let Some(header) = header {
            let name = header
                .split(['(', '<'])
                .next()
                .unwrap_or_default()
                .trim()
                .to_string();
            functions.push((name, String::new()));
        }
        if let Some((_, body)) = functions.last_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    functions
}

/// The test portion of a source file and its first line number: all of an integration-test
/// or `tests.rs` file, else the inline `#[cfg(test)] mod …` tail (the workspace
/// convention), else nothing. This guard itself is skipped: it names the forbidden calls
/// as data.
fn test_code<'a>(file: &Path, text: &'a str) -> Option<(usize, &'a str)> {
    if file.ends_with("hotsheet-plugins/tests/setup_isolation_guard.rs") {
        return None;
    }
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let in_tests_dir = file.components().any(|c| c.as_os_str() == "tests");
    if in_tests_dir || name == "tests.rs" || name.ends_with("_tests.rs") {
        return Some((1, text));
    }
    // An inline module body (`mod tests {`), not a `mod tests;` file declaration.
    let at = text
        .match_indices("#[cfg(test)]\nmod ")
        .find_map(|(at, _)| {
            let header = text[at..].lines().nth(1).unwrap_or_default();
            header.trim_end().ends_with('{').then_some(at)
        })?;
    Some((text[..at].lines().count() + 1, &text[at..]))
}

/// The workspace root (this test's crate is `crates/hotsheet-plugins`).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root resolves")
}

/// Every `.rs` file under `crates/*/{src,tests}` (recursive).
fn rust_files(crates_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(crate_dirs) = std::fs::read_dir(crates_dir) else {
        return out;
    };
    for entry in crate_dirs.flatten() {
        collect_rs(&entry.path().join("src"), &mut out);
        collect_rs(&entry.path().join("tests"), &mut out);
    }
    out
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|x| x.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}
