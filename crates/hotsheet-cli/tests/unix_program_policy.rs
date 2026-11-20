//! HS2-6EW369 policy: test code names no hard-coded Unix program path (`/bin/sh`,
//! `/bin/echo`, `/usr/bin/env`, a `#!/bin/sh` script) outside a `#[cfg(unix)]` item.
//!
//! Such a path does not exist on Windows, so a test that spawns it never exercises Windows
//! behavior: the product rejects `/bin/sh` as neither a binary name nor an absolute path, or the
//! spawn fails with `NotFound`. Tests spawn the portable fixture program from
//! `crates/test-support/fixture_program.rs` instead. A test that genuinely needs a Unix shell
//! (a shell-scripted stand-in, Unix signals, a Unix socket) is an item under `#[cfg(unix)]`,
//! `#[cfg(not(windows))]`, or a Unix `target_os`, with its reason stated beside it.
//!
//! Test code is every file under a `tests` directory or named `tests.rs` or `*_tests.rs`, the
//! shared `test-support` files, and every item under `#[cfg(test)]` elsewhere. Comments are not
//! code and are ignored.

use std::path::{Path, PathBuf};

/// Assembled so this file does not match its own pattern.
const UNIX_PROGRAM: &str = concat!("/", "bin/");

/// Existing test files that still name a Unix program outside a Unix-only item, with the
/// number of occurrences each may keep. A count only ever decreases: port the test to the
/// fixture program or mark its item Unix-only, then lower or remove the entry.
const BASELINE: &[(&str, usize)] = &[
    ("hotsheet-aitools/src/launch_safety.rs", 1),
    ("hotsheet-aitools/src/session_push/claude.rs", 1),
    ("hotsheet-aitools/src/tests.rs", 1),
    (
        "hotsheet-cli/src/orchestration/tests/validation_portable.rs",
        1,
    ),
    ("hotsheet-cli/src/permission_hook.rs", 1),
    ("hotsheet-cli/tests/cli.rs", 1),
    ("hotsheet-cli/tests/terminal_relay.rs", 2),
    ("hotsheet-notify/src/external.rs", 2),
    ("hotsheet-plugins/src/mcp_local_layer.rs", 2),
    ("hotsheet-plugins/src/shared_host.rs", 3),
    ("hotsheet-server/src/commands.rs", 1),
    ("hotsheet-server/src/request_performance_tests.rs", 1),
    ("hotsheet-server/src/routes/terminals/launch.rs", 10),
    ("hotsheet-server/src/terminal_broker_startup_tests.rs", 3),
    ("hotsheet-server/tests/terminal_launch_relay.rs", 1),
    ("hotsheet-terminals/src/launch_agent_tests.rs", 3),
    ("hotsheet-ticketing/src/groups/git_sync/tests.rs", 4),
    ("hotsheet-ticketing/src/merge_driver.rs", 1),
    ("hotsheet-ticketing/src/orchestration/gate_tests.rs", 1),
];

fn rust_sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            rust_sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

fn is_test_file(relative: &Path) -> bool {
    let name = relative
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    relative
        .components()
        .any(|part| part.as_os_str() == "tests" || part.as_os_str() == "test-support")
        || name == "tests.rs"
        || name.ends_with("_tests.rs")
}

/// Byte classes of a Rust source: comment bytes, and the item spans after each attribute.
struct Scan {
    comment: Vec<bool>,
    /// `(attribute text, item start, item end)`, end exclusive.
    items: Vec<(String, usize, usize)>,
}

/// Skip a string, raw string, or char literal starting at `i`, returning the index after it,
/// or `None` when `i` does not start one.
fn skip_literal(bytes: &[u8], i: usize) -> Option<usize> {
    let at = |index: usize| bytes.get(index).copied().unwrap_or(0);
    let mut start = i;
    if at(start) == b'b' && matches!(at(start + 1), b'"' | b'r' | b'\'') {
        start += 1;
    }
    if at(start) == b'r' && matches!(at(start + 1), b'"' | b'#') {
        let mut hashes = 0;
        let mut j = start + 1;
        while at(j) == b'#' {
            hashes += 1;
            j += 1;
        }
        if at(j) != b'"' {
            return None;
        }
        j += 1;
        loop {
            if j >= bytes.len() {
                return Some(j);
            }
            if bytes[j] == b'"' && (1..=hashes).all(|k| at(j + k) == b'#') {
                return Some(j + 1 + hashes);
            }
            j += 1;
        }
    }
    if at(start) == b'"' {
        let mut j = start + 1;
        while j < bytes.len() {
            match bytes[j] {
                b'\\' => j += 2,
                b'"' => return Some(j + 1),
                _ => j += 1,
            }
        }
        return Some(j);
    }
    if at(start) == b'\'' {
        // A char literal ('x', '\n', '\u{..}'); a lifetime ('a) is not a literal.
        if at(start + 1) == b'\\' {
            let mut j = start + 2;
            while j < bytes.len() && bytes[j] != b'\'' {
                j += 1;
            }
            return Some(j + 1);
        }
        let head = &bytes[start + 1..bytes.len().min(start + 5)];
        let head = std::str::from_utf8(head).unwrap_or_else(|error| {
            std::str::from_utf8(&head[..error.valid_up_to()]).unwrap_or_default()
        });
        let width = head.chars().next().map_or(1, char::len_utf8);
        if at(start + 1 + width) == b'\'' {
            return Some(start + 2 + width);
        }
    }
    None
}

/// Whether `i` starts an identifier-adjacent `b`/`r` that is really part of a name.
fn inside_identifier(bytes: &[u8], i: usize) -> bool {
    i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')
}

/// The end (exclusive) of the item that starts at `start`: the first `;` at depth zero, or
/// the `}` that closes a brace opened at depth zero.
fn item_end(bytes: &[u8], start: usize, comment: &[bool]) -> usize {
    let mut depth = 0usize;
    let mut i = start;
    while i < bytes.len() {
        if comment[i] {
            i += 1;
            continue;
        }
        if !inside_identifier(bytes, i) {
            if let Some(next) = skip_literal(bytes, i) {
                i = next;
                continue;
            }
        }
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i + 1;
                }
            }
            b';' if depth == 0 => return i + 1,
            _ => {}
        }
        i += 1;
    }
    bytes.len()
}

/// The end (exclusive) of the attribute whose `[` is at `open`.
fn attribute_end(bytes: &[u8], open: usize, comment: &[bool]) -> usize {
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        if !comment[i] && !inside_identifier(bytes, i) {
            if let Some(next) = skip_literal(bytes, i) {
                i = next;
                continue;
            }
        }
        if !comment[i] {
            match bytes[i] {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        return i + 1;
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    bytes.len()
}

fn scan(text: &str) -> Scan {
    let bytes = text.as_bytes();
    let mut comment = vec![false; bytes.len()];
    // First pass: comments, skipping literals so `//` inside a string is not a comment.
    let mut i = 0;
    while i < bytes.len() {
        if !inside_identifier(bytes, i) {
            if let Some(next) = skip_literal(bytes, i) {
                i = next;
                continue;
            }
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            let end = text[i..].find('\n').map_or(bytes.len(), |n| i + n);
            comment[i..end].fill(true);
            i = end;
        } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let end = text[i + 2..].find("*/").map_or(bytes.len(), |n| i + n + 4);
            comment[i..end].fill(true);
            i = end;
        } else {
            i += 1;
        }
    }
    // Second pass: every outer attribute and the item it applies to.
    let mut items = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if comment[i] {
            i += 1;
            continue;
        }
        if !inside_identifier(bytes, i) {
            if let Some(next) = skip_literal(bytes, i) {
                i = next;
                continue;
            }
        }
        if bytes[i] == b'#' && bytes.get(i + 1) == Some(&b'[') {
            let attribute_end = attribute_end(bytes, i + 1, &comment);
            let attribute = text[i..attribute_end].to_string();
            if attribute.starts_with("#[cfg(") {
                let end = item_end(bytes, attribute_end, &comment);
                items.push((attribute, attribute_end, end));
            }
            i = attribute_end;
            continue;
        }
        i += 1;
    }
    Scan { comment, items }
}

/// A `cfg` predicate that never holds on Windows.
fn unix_only(attribute: &str) -> bool {
    let predicate: String = attribute.chars().filter(|c| !c.is_whitespace()).collect();
    predicate.starts_with("#[cfg(unix")
        || predicate.starts_with("#[cfg(not(windows)")
        || predicate.starts_with("#[cfg(all(unix")
        || predicate.starts_with("#[cfg(target_os=\"macos\"")
        || predicate.starts_with("#[cfg(target_os=\"linux\"")
        || predicate.starts_with("#[cfg(any(target_os=\"macos\",target_os=\"linux\")")
        || predicate.starts_with("#[cfg(any(target_os=\"linux\",target_os=\"macos\")")
}

/// Whether the `/bin/` at `offset` starts a system program path (`/bin/`, `/usr/bin/`,
/// `/usr/local/bin/`) rather than ending a longer one such as `<snapshot>/bin/hotsheet-cli`.
fn starts_a_path(text: &str, offset: usize) -> bool {
    let before = &text[..offset];
    let start = ["/usr/local", "/usr"]
        .iter()
        .find(|prefix| before.ends_with(*prefix))
        .map_or(offset, |prefix| offset - prefix.len());
    !text[..start]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
}

fn is_cfg_test(attribute: &str) -> bool {
    attribute.replace(char::is_whitespace, "") == "#[cfg(test)]"
}

/// Byte offsets of Unix program paths in test code outside Unix-only items.
fn offenders(text: &str, whole_file_is_test: bool) -> Vec<usize> {
    if !text.contains(UNIX_PROGRAM) {
        return Vec::new();
    }
    let Scan { comment, items } = scan(text);
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.contains("#![cfg(unix)]") || compact.contains("#![cfg(not(windows))]") {
        return Vec::new();
    }
    let within = |offset: usize, test: fn(&str) -> bool| {
        items
            .iter()
            .any(|(attribute, start, end)| test(attribute) && (*start..*end).contains(&offset))
    };
    text.match_indices(UNIX_PROGRAM)
        .map(|(offset, _)| offset)
        .filter(|offset| starts_a_path(text, *offset))
        .filter(|offset| !comment[*offset])
        .filter(|offset| whole_file_is_test || within(*offset, is_cfg_test))
        .filter(|offset| !within(*offset, unix_only))
        .collect()
}

/// The out-of-line modules `text` (the file at `path`) declares, each with whether its
/// declaration is Unix-only and whether it is `#[cfg(test)]`.
/// An out-of-line module file, whether its declaration is Unix-only, and whether it is test code.
type ChildModule = (PathBuf, bool, bool);

fn child_modules(path: &Path, text: &str) -> Vec<ChildModule> {
    let declarations = text
        .match_indices("mod ")
        .map(|(offset, _)| offset)
        .filter(|offset| {
            *offset == 0 || matches!(text.as_bytes()[*offset - 1], b' ' | b'\n' | b')')
        })
        .filter_map(|offset| {
            let rest = &text[offset + 4..];
            let module: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            (!module.is_empty() && rest[module.len()..].starts_with(';'))
                .then_some((offset, module))
        })
        .collect::<Vec<_>>();
    if declarations.is_empty() {
        return Vec::new();
    }
    let Scan { comment, items } = scan(text);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let parent = path.parent().unwrap();
    let base = if matches!(name, "mod.rs" | "lib.rs" | "main.rs") {
        parent.to_path_buf()
    } else {
        parent.join(path.file_stem().unwrap())
    };
    let mut children = Vec::new();
    for (offset, module) in declarations {
        if comment[offset] {
            continue;
        }
        let gated = |test: fn(&str) -> bool| {
            items
                .iter()
                .any(|(attribute, start, end)| test(attribute) && (*start..*end).contains(&offset))
        };
        // A `#[path]` module is not followed; none of them names a Unix program today.
        for candidate in [
            base.join(format!("{module}.rs")),
            base.join(&module).join("mod.rs"),
        ] {
            if candidate.is_file() {
                children.push((candidate, gated(unix_only), gated(is_cfg_test)));
            }
        }
    }
    children
}

#[test]
fn hs2_6ew369_tests_hard_code_no_unix_program_paths_outside_cfg_unix() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let this = Path::new(file!()).file_name().unwrap();
    let mut sources = Vec::new();
    rust_sources(crates, &mut sources);
    assert!(
        sources.len() > 100,
        "the scan must see the workspace sources"
    );
    let texts: std::collections::BTreeMap<&PathBuf, String> = sources
        .iter()
        .map(|path| (path, std::fs::read_to_string(path).unwrap()))
        .collect();
    // A module declared under a Unix-only cfg is Unix-only as a whole, and so are the modules
    // it declares; a module declared under #[cfg(test)] is test code, likewise.
    let children: Vec<(&PathBuf, Vec<ChildModule>)> = texts
        .iter()
        .map(|(path, text)| (*path, child_modules(path, text)))
        .collect();
    let mut unix_files = std::collections::BTreeSet::new();
    let mut test_files = std::collections::BTreeSet::new();
    loop {
        let before = (unix_files.len(), test_files.len());
        for (path, modules) in &children {
            for (child, unix, test) in modules.iter().cloned() {
                if unix || unix_files.contains(*path) {
                    unix_files.insert(child.clone());
                }
                if test || test_files.contains(*path) {
                    test_files.insert(child);
                }
            }
        }
        if (unix_files.len(), test_files.len()) == before {
            break;
        }
    }
    let mut found = Vec::new();
    for (path, text) in texts
        .iter()
        .filter(|(path, _)| path.file_name() != Some(this) && !unix_files.contains(**path))
    {
        let relative = path.strip_prefix(crates).unwrap();
        let hits = offenders(text, is_test_file(relative) || test_files.contains(*path));
        if !hits.is_empty() {
            let lines = hits
                .iter()
                .map(|offset| text[..*offset].lines().count().max(1))
                .collect::<Vec<_>>();
            let name = relative.to_string_lossy().replace('\\', "/");
            found.push((name, hits.len(), lines));
        }
    }
    let unexpected = found
        .iter()
        .filter(|(name, count, _)| {
            BASELINE
                .iter()
                .find(|(allowed, _)| allowed == name)
                .is_none_or(|(_, allowed)| count > allowed)
        })
        .collect::<Vec<_>>();
    assert!(
        unexpected.is_empty(),
        "test code names a Unix program path outside #[cfg(unix)] (HS2-6EW369); spawn \
         crates/test-support/fixture_program.rs instead, or mark the item #[cfg(unix)] with its \
         reason: {unexpected:?}"
    );
    let stale = BASELINE
        .iter()
        .filter(|(allowed, count)| {
            found
                .iter()
                .find(|(name, _, _)| name == allowed)
                .is_none_or(|(_, found, _)| found < count)
        })
        .collect::<Vec<_>>();
    assert!(
        stale.is_empty(),
        "lower these HS2-6EW369 baseline counts to what the files still hold: {stale:?}"
    );
}

#[test]
fn hs2_6ew369_the_scan_tells_unix_only_items_comments_and_test_code_apart() {
    let program = UNIX_PROGRAM;
    let source = format!(
        r##"// "{program}sh" in a comment
fn product() {{ let _ = "{program}sh"; }}
#[cfg(test)]
mod tests {{
    #[cfg(unix)]
    fn unix_only() {{ let _ = "{program}sh"; }}
    #[cfg(unix)]
    const SCRIPT: &str = r#"#!{program}sh"#;
    #[test]
    fn portable() {{ let _ = '}}'; let _ = "{{ {program}echo"; }}
}}
"##
    );
    let hits = offenders(&source, false);
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert!(source[..hits[0]].ends_with("\"{ "));
    // In a test file every non-Unix occurrence counts, product code included.
    assert_eq!(offenders(&source, true).len(), 2);
    let gated = format!("#![cfg(unix)]\nfn f() {{ let _ = \"{program}sh\"; }}\n");
    assert!(offenders(&gated, true).is_empty());
}
