//! HS2-CSXZ7K policy: server tests build JSON request bodies with `serde_json`, never by
//! formatting a `Path::display()` into a JSON string literal.
//!
//! A Windows path holds backslashes, which are escapes in JSON. A body such as
//! `{"path":"C:\Users\..."}` is invalid there, so registration answered 400 and thirteen
//! multi-store tests failed before reaching the behavior they cover.

use std::path::{Path, PathBuf};

fn rust_sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// The text of each `format!(...)` call in `source`, with its line number. Parentheses
/// inside string literals do not count toward the balance.
fn format_calls(source: &str) -> Vec<(usize, &str)> {
    let opener = concat!("format", "!(");
    let bytes = source.as_bytes();
    let mut calls = Vec::new();
    let mut from = 0;
    while let Some(offset) = source[from..].find(opener) {
        let start = from + offset;
        let mut index = start + opener.len();
        let mut depth = 1;
        while depth > 0 && index < bytes.len() {
            match bytes[index] {
                b'r' if source[index..].starts_with("r#\"") => {
                    index = source[index + 3..]
                        .find("\"#")
                        .map_or(bytes.len(), |end| index + 3 + end + 2);
                    continue;
                }
                b'"' => {
                    index += 1;
                    while index < bytes.len() && bytes[index] != b'"' {
                        index += if bytes[index] == b'\\' { 2 } else { 1 };
                    }
                }
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            index += 1;
        }
        let end = index.min(bytes.len());
        calls.push((
            source[..start].matches('\n').count() + 1,
            &source[start..end],
        ));
        from = end;
    }
    calls
}

/// A formatted string that looks like a JSON object and interpolates a displayed path.
fn formats_a_path_into_json(call: &str) -> bool {
    let json_like = call.contains(r#"{{""#) || call.contains(r#"{{\""#);
    json_like && call.contains(concat!(".display", "()"))
}

#[test]
fn hs2_csxz7k_server_tests_never_format_a_displayed_path_into_json() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    rust_sources(&crate_dir.join("tests"), &mut sources);
    rust_sources(&crate_dir.join("src"), &mut sources);
    assert!(sources.len() > 50, "the scan must see the server sources");
    let offenders: Vec<String> = sources
        .iter()
        .flat_map(|path| {
            let text = std::fs::read_to_string(path).unwrap();
            let name = path.strip_prefix(crate_dir).unwrap().display().to_string();
            format_calls(&text)
                .into_iter()
                .filter(|(_, call)| formats_a_path_into_json(call))
                .map(|(line, _)| format!("{name}:{line}"))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "build JSON bodies with serde_json::json!, not format! with display() (HS2-CSXZ7K): {offenders:?}"
    );
}

#[test]
fn hs2_csxz7k_the_scan_finds_a_formatted_path_body() {
    let sample = concat!(
        "let body = format",
        "!(r#\"{{\"path\":\"{}\"}}\"#, dir.path().display",
        "());\n",
        "let ok = format",
        "!(\"{}\", dir.path().display",
        "());\n",
        "let also = format",
        "!(\"{{\\\"path\\\":\\\"{}\\\"}}\", store.display",
        "());\n",
    );
    let flagged: Vec<usize> = format_calls(sample)
        .into_iter()
        .filter(|(_, call)| formats_a_path_into_json(call))
        .map(|(line, _)| line)
        .collect();
    assert_eq!(flagged, [1, 3]);
}
