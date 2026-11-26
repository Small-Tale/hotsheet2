//! JSON with comments and trailing commas (JSONC), as OpenCode accepts in `opencode.json`
//! (HS2-RMQ5XX). Setup must merge Hot Sheet's MCP entry into such a file without losing the
//! user's keys or comments, so this module both parses JSONC and edits one member in place.
//!
//! Comments and trailing commas are blanked to spaces byte for byte, so every offset in the
//! blanked text is the same offset in the original. Edits locate spans in the blanked text and
//! splice the original, leaving every other byte (comments included) untouched.

use serde_json::Value;

/// Parse JSONC into a value. Strict JSON parses identically.
pub(crate) fn parse(text: &str) -> Result<Value, String> {
    serde_json::from_str(&blank(text)?).map_err(|error| error.to_string())
}

/// Whether the text uses JSONC extensions (comments or trailing commas).
pub(crate) fn has_extensions(text: &str) -> bool {
    blank(text).is_ok_and(|blanked| blanked != text)
}

/// Set `root[path[0]]...[path[n-1]] = value`, creating missing intermediate objects and
/// replacing a non-object intermediate, by splicing only the affected member. The root must
/// be an object. Callers verify the result re-parses to the intended value.
pub(crate) fn set_member(text: &str, path: &[&str], value: &Value) -> Result<String, String> {
    let (blanked, trailing) = blank_parts(text)?;
    let bytes = blanked.as_bytes();
    let mut pos = skip_ws(bytes, 0);
    let root = parse_node(bytes, &mut pos)?;
    if !matches!(root.kind, Kind::Object { .. }) {
        return Err("the top level is not an object".into());
    }
    let source = Source {
        text,
        bytes,
        trailing: &trailing,
    };
    let (range, replacement) = edit_object(&source, &root, path, value)?;
    let mut out = String::with_capacity(text.len() + replacement.len());
    out.push_str(&text[..range.0]);
    out.push_str(&replacement);
    out.push_str(&text[range.1..]);
    Ok(out)
}

/// Remove `root[path[0]]...[path[n-1]]` by splicing out only that member: its key and
/// value, its separator, and a comment trailing it on its own line (HS2-DWS3NS). Comments
/// on other lines stay. `Ok(None)` when the member does not exist. Callers verify the
/// result re-parses to the intended value.
pub(crate) fn remove_member(text: &str, path: &[&str]) -> Result<Option<String>, String> {
    let (blanked, trailing) = blank_parts(text)?;
    let bytes = blanked.as_bytes();
    let mut pos = skip_ws(bytes, 0);
    let mut object = parse_node(bytes, &mut pos)?;
    let Some((last_key, parents)) = path.split_last() else {
        return Err("empty member path".into());
    };
    for key in parents {
        let Kind::Object { members, .. } = object.kind else {
            return Ok(None);
        };
        let Some(member) = members.into_iter().rev().find(|member| member.key == *key) else {
            return Ok(None);
        };
        object = member.value;
    }
    let Kind::Object { members, .. } = &object.kind else {
        return Ok(None);
    };
    // Duplicate keys: the last one wins, as in the parsed value.
    let Some(index) = members.iter().rposition(|member| member.key == *last_key) else {
        return Ok(None);
    };
    let member = &members[index];
    let is_last = index + 1 == members.len();

    // The member's own separator: a real comma right after it, or a blanked trailing one.
    let next = skip_ws(bytes, member.value.end);
    let own_comma = if bytes.get(next) == Some(&b',') {
        Some(next)
    } else {
        trailing
            .iter()
            .copied()
            .find(|&i| member.value.end <= i && i < next)
    };
    let after = own_comma.map_or(member.value.end, |i| i + 1);

    // Take whole lines when the member starts its line and nothing but whitespace or a
    // line comment follows it there.
    let line_start = text[..member.key_start].rfind('\n').map_or(0, |i| i + 1);
    let starts_line = text[line_start..member.key_start]
        .bytes()
        .all(|byte| byte == b' ' || byte == b'\t');
    let line_end = text[after..].find('\n').map(|i| after + i);
    let open_block = line_end.is_some_and(|end| {
        let rest = &text[after..end];
        rest.rfind("/*") > rest.rfind("*/")
    });
    let (start, end) = match line_end {
        Some(end)
            if starts_line
                && !open_block
                && bytes[after..end].iter().all(u8::is_ascii_whitespace) =>
        {
            (line_start, end + 1)
        }
        _ => {
            // Inline: drop the member and the spaces after its separator.
            let spaces = text[after..]
                .bytes()
                .take_while(|byte| *byte == b' ' || *byte == b'\t')
                .count();
            let end = if own_comma.is_some() {
                after + spaces
            } else {
                after
            };
            (member.key_start, end)
        }
    };

    let mut out = String::with_capacity(text.len());
    // The last member without its own comma leaves the previous separator dangling. A file
    // that already uses trailing commas keeps it as one (so removing a member that setup
    // appended restores the file exactly); otherwise remove that one comma and keep
    // whatever follows it (a comment on its line).
    let previous_comma = (is_last && own_comma.is_none() && index > 0 && trailing.is_empty())
        .then(|| {
            let previous = &members[index - 1];
            let comma = skip_ws(bytes, previous.value.end);
            (bytes.get(comma) == Some(&b',')).then_some(comma)
        })
        .flatten();
    match previous_comma {
        Some(comma) if comma < start => {
            out.push_str(&text[..comma]);
            out.push_str(&text[comma + 1..start]);
        }
        _ => out.push_str(&text[..start]),
    }
    out.push_str(&text[end..]);
    Ok(Some(out))
}

/// Replace comments and trailing commas with spaces, keeping newlines and byte offsets.
fn blank(text: &str) -> Result<String, String> {
    blank_parts(text).map(|(blanked, _)| blanked)
}

/// [`blank`], plus the offsets of the trailing commas it removed.
fn blank_parts(text: &str) -> Result<(String, Vec<usize>), String> {
    let src = text.as_bytes();
    let mut out = src.to_vec();
    let mut i = 0;
    while i < src.len() {
        match src[i] {
            b'"' => i = skip_string(src, i)?,
            b'/' if src.get(i + 1) == Some(&b'/') => {
                while i < src.len() && src[i] != b'\n' {
                    if src[i] != b'\r' {
                        out[i] = b' ';
                    }
                    i += 1;
                }
            }
            b'/' if src.get(i + 1) == Some(&b'*') => {
                let start = i;
                i += 2;
                loop {
                    if i + 1 >= src.len() {
                        return Err(format!("unterminated block comment at byte {start}"));
                    }
                    if src[i] == b'*' && src[i + 1] == b'/' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                for byte in &mut out[start..i] {
                    if *byte != b'\n' && *byte != b'\r' {
                        *byte = b' ';
                    }
                }
            }
            _ => i += 1,
        }
    }
    // Trailing commas: a comma whose next significant byte closes the container.
    let mut trailing = Vec::new();
    let mut i = 0;
    while i < out.len() {
        match out[i] {
            b'"' => i = skip_string(&out, i)?,
            b',' => {
                let next = skip_ws(&out, i + 1);
                if matches!(out.get(next), Some(b'}' | b']')) {
                    out[i] = b' ';
                    trailing.push(i);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    // Only ASCII bytes outside strings were replaced by ASCII spaces.
    let blanked = String::from_utf8(out).map_err(|error| error.to_string())?;
    Ok((blanked, trailing))
}

/// Index just past the string starting at `start` (which is a `"`).
fn skip_string(bytes: &[u8], start: usize) -> Result<usize, String> {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Ok(i + 1),
            _ => i += 1,
        }
    }
    Err(format!("unterminated string at byte {start}"))
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

struct Node {
    start: usize,
    end: usize,
    kind: Kind,
}

enum Kind {
    Object { open: usize, members: Vec<Member> },
    Other,
}

struct Member {
    key: String,
    key_start: usize,
    value: Node,
}

/// Parse one strict-JSON value from the blanked text, recording object member spans.
fn parse_node(bytes: &[u8], pos: &mut usize) -> Result<Node, String> {
    let start = *pos;
    let unexpected = |at: usize| format!("unexpected input at byte {at}");
    match bytes.get(start) {
        Some(b'{') => {
            let mut members = Vec::new();
            *pos = skip_ws(bytes, start + 1);
            if bytes.get(*pos) == Some(&b'}') {
                *pos += 1;
            } else {
                loop {
                    if bytes.get(*pos) != Some(&b'"') {
                        return Err(unexpected(*pos));
                    }
                    let key_start = *pos;
                    *pos = skip_string(bytes, key_start)?;
                    let key: String = serde_json::from_slice(&bytes[key_start..*pos])
                        .map_err(|error| error.to_string())?;
                    *pos = skip_ws(bytes, *pos);
                    if bytes.get(*pos) != Some(&b':') {
                        return Err(unexpected(*pos));
                    }
                    *pos = skip_ws(bytes, *pos + 1);
                    let value = parse_node(bytes, pos)?;
                    members.push(Member {
                        key,
                        key_start,
                        value,
                    });
                    *pos = skip_ws(bytes, *pos);
                    match bytes.get(*pos) {
                        Some(b',') => *pos = skip_ws(bytes, *pos + 1),
                        Some(b'}') => {
                            *pos += 1;
                            break;
                        }
                        _ => return Err(unexpected(*pos)),
                    }
                }
            }
            Ok(Node {
                start,
                end: *pos,
                kind: Kind::Object {
                    open: start,
                    members,
                },
            })
        }
        Some(b'[') => {
            *pos = skip_ws(bytes, start + 1);
            if bytes.get(*pos) == Some(&b']') {
                *pos += 1;
            } else {
                loop {
                    parse_node(bytes, pos)?;
                    *pos = skip_ws(bytes, *pos);
                    match bytes.get(*pos) {
                        Some(b',') => *pos = skip_ws(bytes, *pos + 1),
                        Some(b']') => {
                            *pos += 1;
                            break;
                        }
                        _ => return Err(unexpected(*pos)),
                    }
                }
            }
            Ok(Node {
                start,
                end: *pos,
                kind: Kind::Other,
            })
        }
        Some(b'"') => {
            *pos = skip_string(bytes, start)?;
            Ok(Node {
                start,
                end: *pos,
                kind: Kind::Other,
            })
        }
        Some(_) => {
            while *pos < bytes.len()
                && !bytes[*pos].is_ascii_whitespace()
                && !matches!(bytes[*pos], b',' | b'}' | b']')
            {
                *pos += 1;
            }
            if *pos == start {
                return Err(unexpected(start));
            }
            Ok(Node {
                start,
                end: *pos,
                kind: Kind::Other,
            })
        }
        None => Err("unexpected end of input".into()),
    }
}

/// The original text with its blanked twin and the trailing commas the blanking removed.
#[derive(Clone, Copy)]
struct Source<'a> {
    text: &'a str,
    bytes: &'a [u8],
    trailing: &'a [usize],
}

/// Leading whitespace of the line containing `at`.
fn line_indent(text: &str, at: usize) -> &str {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let rest = &text[line_start..];
    &rest[..rest.len() - rest.trim_start_matches([' ', '\t']).len()]
}

/// The file's line ending, so inserted lines match it.
fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// Pretty JSON whose continuation lines start at `indent`.
fn render(value: &Value, indent: &str, newline: &str) -> String {
    serde_json::to_string_pretty(value)
        .unwrap_or_default()
        .replace('\n', &format!("{newline}{indent}"))
}

/// The value to write for `path` below a member that is missing or not an object.
fn nest(path: &[&str], value: &Value) -> Value {
    path.iter().rev().fold(
        value.clone(),
        |inner, key| serde_json::json!({ *key: inner }),
    )
}

/// The byte range to replace and its replacement for setting `path` inside `object`.
fn edit_object(
    source: &Source,
    object: &Node,
    path: &[&str],
    value: &Value,
) -> Result<((usize, usize), String), String> {
    let Source {
        text,
        bytes,
        trailing,
    } = *source;
    let Kind::Object { open, members } = &object.kind else {
        return Err("expected an object".into());
    };
    let Some((key, rest)) = path.split_first() else {
        return Err("empty member path".into());
    };
    // Duplicate keys: the last one wins, as in the parsed value.
    if let Some(member) = members.iter().rev().find(|member| member.key == *key) {
        if !rest.is_empty() && matches!(member.value.kind, Kind::Object { .. }) {
            return edit_object(source, &member.value, rest, value);
        }
        let indent = line_indent(text, member.key_start);
        return Ok((
            (member.value.start, member.value.end),
            render(&nest(rest, value), indent, newline(text)),
        ));
    }
    let member_value = nest(rest, value);
    let nl = newline(text);
    let key_json = serde_json::to_string(key).map_err(|error| error.to_string())?;
    let Some(last) = members.last() else {
        let indent = line_indent(text, *open);
        let child = format!("{indent}  ");
        let member = format!("{key_json}: {}", render(&member_value, &child, nl));
        let close = object.end - 1;
        return Ok(if text[open + 1..close].trim().is_empty() {
            (
                (open + 1, close),
                format!("{nl}{child}{member}{nl}{indent}"),
            )
        } else {
            ((open + 1, open + 1), format!("{nl}{child}{member}"))
        });
    };
    let child = line_indent(text, last.key_start).to_string();
    let member = format!("{key_json}: {}", render(&member_value, &child, nl));
    // Any separator after the last member is a trailing comma, blanked in `bytes`.
    let comma_at = trailing
        .iter()
        .copied()
        .find(|&i| last.value.end <= i && i < object.end);
    let has_comma = comma_at.is_some();
    let anchor = comma_at.map_or(last.value.end, |i| i + 1);
    let line_end = text[anchor..].find('\n').map_or(text.len(), |i| anchor + i);
    let line_end = if text[..line_end].ends_with('\r') {
        line_end - 1
    } else {
        line_end
    };
    // Insert after anything that trails the last member on its line (a comment), adding the
    // missing separator right after the value so the comment stays with its member.
    let comma = if has_comma { "" } else { "," };
    // A block comment that runs past the line end would swallow the inserted member.
    let open_block = text[anchor..line_end].rfind("/*") > text[anchor..line_end].rfind("*/");
    if line_end < text.len()
        && !open_block
        && bytes[anchor..line_end].iter().all(u8::is_ascii_whitespace)
    {
        let replacement = format!(
            "{comma}{}{nl}{child}{member}",
            &text[last.value.end..line_end]
        );
        return Ok(((last.value.end, line_end), replacement));
    }
    Ok((
        (anchor, anchor),
        format!(
            "{comma} {key_json}: {}",
            serde_json::to_string(&member_value).unwrap_or_default()
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_comments_and_trailing_commas_like_strict_json() {
        let text = "// head\n{\n  /* block\n  comment */ \"a\": \"x // not a comment\",\n  \"b\": [1, 2,],\n}\n";
        assert_eq!(
            parse(text).unwrap(),
            json!({ "a": "x // not a comment", "b": [1, 2] })
        );
        assert!(has_extensions(text));
        assert!(!has_extensions("{\"a\": 1}"));
        assert_eq!(
            parse("{\"a\": \"/*\\\"*/\"}").unwrap(),
            json!({ "a": "/*\"*/" })
        );
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(parse("{ \"a\": 1 ").is_err());
        assert!(parse("{ /* open").is_err());
        assert!(parse("{ \"a: 1 }").is_err());
        assert!(set_member("[1]", &["a"], &json!(1)).is_err());
    }

    fn check(text: &str, path: &[&str], value: Value) -> String {
        let out = set_member(text, path, &value).unwrap();
        let mut expected = parse(text).unwrap();
        let (last, parents) = path.split_last().unwrap();
        let mut slot = &mut expected;
        for key in parents {
            if !slot.get(*key).is_some_and(Value::is_object) {
                slot[*key] = json!({});
            }
            slot = &mut slot[*key];
        }
        slot[*last] = value;
        assert_eq!(parse(&out).unwrap(), expected, "{out}");
        out
    }

    #[test]
    fn inserts_into_an_existing_object_keeping_comments() {
        let text = "{\n  // servers\n  \"mcp\": {\n    \"other\": { \"type\": \"local\" } // theirs\n  },\n  \"theme\": \"dark\", // keep\n}\n";
        let out = check(text, &["mcp", "hotsheet"], json!({ "type": "local" }));
        assert!(out.contains("// servers") && out.contains("// theirs") && out.contains("// keep"));
        assert!(out.contains("}, // theirs\n    \"hotsheet\""));
        assert!(out.ends_with("\"theme\": \"dark\", // keep\n}\n"));
    }

    #[test]
    fn replaces_an_existing_member_in_place() {
        let text = "{\n  \"mcp\": {\n    \"hotsheet\": { \"old\": true }, // ours\n  },\n}";
        let out = check(text, &["mcp", "hotsheet"], json!({ "new": [1] }));
        assert!(out.contains("// ours"));
        assert!(!out.contains("old"));
    }

    #[test]
    fn creates_missing_and_replaces_non_object_parents() {
        let missing = check("{ /* c */ }", &["mcp", "x"], json!(1));
        assert!(missing.contains("/* c */"));
        check("{}", &["mcp", "x"], json!(1));
        check("{\n  \"a\": 1 // last\n}", &["mcp", "x"], json!(1));
        check("{\"a\": 1, \"mcp\": 3}", &["mcp", "x"], json!(1));
        check("{\"a\": 1,}", &["mcp", "x"], json!(1));
        // A comma inside a trailing comment is not a separator.
        let comma = check("{\n  \"a\": 1 // x, y\n}", &["mcp", "x"], json!(1));
        assert!(comma.contains("\"a\": 1, // x, y\n"), "{comma}");
        // A block comment left open at the line end keeps the member before it.
        let block = check("{\n  \"a\": 1 /* x\n  y */\n}", &["mcp", "x"], json!(1));
        assert!(block.contains("/* x\n  y */"), "{block}");
        check(
            "{\r\n  \"a\": 1\r\n}\r\n",
            &["mcp", "x"],
            json!({ "y": [] }),
        );
    }

    fn check_remove(text: &str, path: &[&str]) -> String {
        let out = remove_member(text, path).unwrap().unwrap();
        let mut expected = parse(text).unwrap();
        let (last, parents) = path.split_last().unwrap();
        let mut slot = &mut expected;
        for key in parents {
            slot = &mut slot[*key];
        }
        slot.as_object_mut().unwrap().remove(*last);
        assert_eq!(parse(&out).unwrap(), expected, "{out}");
        out
    }

    #[test]
    fn hs2_dws3ns_removes_one_member_with_its_line_and_separator() {
        let text = "{\n  // servers\n  \"mcp\": {\n    \"other\": { \"type\": \"local\" }, // theirs\n    \"hotsheet\": {\n      \"type\": \"local\"\n    }, // ours\n    /* third */\n    \"third\": 1\n  },\n}\n";
        assert_eq!(
            check_remove(text, &["mcp", "hotsheet"]),
            "{\n  // servers\n  \"mcp\": {\n    \"other\": { \"type\": \"local\" }, // theirs\n    /* third */\n    \"third\": 1\n  },\n}\n"
        );
        // The last member without a trailing comma takes the previous separator, keeping
        // the comment that follows it.
        let last = "{\n  \"mcp\": {\n    \"other\": 1, // theirs\n    \"hotsheet\": 2\n  }\n}";
        assert_eq!(
            check_remove(last, &["mcp", "hotsheet"]),
            "{\n  \"mcp\": {\n    \"other\": 1 // theirs\n  }\n}"
        );
        // A file that uses trailing commas keeps the dangling one as a trailing comma.
        let styled = "{\n  \"mcp\": {\n    \"other\": 1, // theirs\n    \"hotsheet\": 2\n  },\n}";
        assert_eq!(
            check_remove(styled, &["mcp", "hotsheet"]),
            "{\n  \"mcp\": {\n    \"other\": 1, // theirs\n  },\n}"
        );
        // With a trailing comma, the previous separator becomes the trailing one.
        let trailing = "{\n  \"a\": 1,\n  \"hotsheet\": 2,\n}\n";
        assert_eq!(check_remove(trailing, &["hotsheet"]), "{\n  \"a\": 1,\n}\n");
        let first = "{\r\n  \"hotsheet\": 2,\r\n  \"a\": 1\r\n}\r\n";
        assert_eq!(
            check_remove(first, &["hotsheet"]),
            "{\r\n  \"a\": 1\r\n}\r\n"
        );
    }

    #[test]
    fn hs2_dws3ns_removes_inline_members_and_reports_missing_ones() {
        assert_eq!(
            check_remove("{\"a\": 1, \"hotsheet\": 2, \"b\": 3}", &["hotsheet"]),
            "{\"a\": 1, \"b\": 3}"
        );
        assert_eq!(
            check_remove("{\"a\": 1, \"hotsheet\": 2}", &["hotsheet"]),
            "{\"a\": 1 }"
        );
        assert_eq!(check_remove("{\"hotsheet\": 2}", &["hotsheet"]), "{}");
        // A block comment opened after the member stays intact.
        let block = check_remove(
            "{\n  \"hotsheet\": 1, /* a\n  b */\n  \"c\": 2\n}",
            &["hotsheet"],
        );
        assert!(block.contains("/* a\n  b */"), "{block}");
        assert_eq!(remove_member("{\"a\": 1}", &["hotsheet"]).unwrap(), None);
        assert_eq!(remove_member("{\"a\": 1}", &["mcp", "x"]).unwrap(), None);
        assert_eq!(remove_member("{\"mcp\": 3}", &["mcp", "x"]).unwrap(), None);
        assert!(remove_member("{ \"a\": ", &["a"]).is_err());
    }

    #[test]
    fn edits_a_commented_opencode_config() {
        let text = "// Project OpenCode config\n{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  /* keep this model */\n  \"model\": \"synthetic/model-a\",\n  \"mcp\": {\n    \"other\": { \"type\": \"local\", \"command\": [\"other-mcp\"] }, // theirs\n  },\n}\n";
        let out = check(
            text,
            &["mcp", "hotsheet"],
            json!({ "type": "local", "command": ["hotsheet-mcp"] }),
        );
        assert!(
            out.contains("[\"other-mcp\"] }, // theirs\n    \"hotsheet\": {\n      \"command\"")
        );
        assert!(out.starts_with("// Project OpenCode config\n{") && out.ends_with("  },\n}\n"));
    }
}
