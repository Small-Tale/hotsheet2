//! Deterministic, review-only secret span hints for pending permission actions.
//! Results carry offsets and fixed labels, never a copy of the suspected value.

use crate::permission_pattern::{PATTERN_VERSION, PatternKind, PatternSpec};
use serde::Serialize;

const MAX_ACTION_BYTES: usize = 16 * 1024;
const MAX_SUGGESTIONS: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SecretSuggestion {
    /// UTF-16 offsets into the exact action shown to the browser.
    pub start: usize,
    pub end: usize,
    pub label: &'static str,
    /// `token` or `quoted`, matching the reviewed v1 template syntax.
    pub placeholder: &'static str,
}

#[derive(Clone, Copy)]
struct Word {
    start: usize,
    end: usize,
}

#[derive(Clone, Copy)]
struct Candidate {
    start: usize,
    end: usize,
    label: &'static str,
}

pub fn suggest_secret_spans(tool: &str, action: &str) -> Vec<SecretSuggestion> {
    if action.is_empty()
        || action.len() > MAX_ACTION_BYTES
        || action.contains(['\n', '\r', '\0', '`'])
        || action.contains("$(")
        || action.contains("{{")
        || action.contains("}}")
    {
        return Vec::new();
    }
    let bash = tool.eq_ignore_ascii_case("bash");
    let words = if bash {
        let Some(words) = shell_words(action) else {
            return Vec::new();
        };
        words
    } else {
        vec![Word {
            start: 0,
            end: action.len(),
        }]
    };
    let mut candidates = Vec::new();
    for (index, word) in words.iter().enumerate() {
        let (inner_start, inner_end) = strip_quotes(action, word.start, word.end);
        let value = &action[inner_start..inner_end];
        if bash {
            if let Some((key, _)) = value.split_once('=') {
                if secret_key(key) {
                    let value_start = inner_start + key.len() + 1;
                    let (start, end) = strip_quotes(action, value_start, inner_end);
                    candidates.push(Candidate {
                        start,
                        end,
                        label: "Secret assignment",
                    });
                }
            }
            if secret_key(value) && index + 1 < words.len() {
                let next = words[index + 1];
                let (start, end) = strip_quotes(action, next.start, next.end);
                if !action[start..end].starts_with('-') {
                    candidates.push(Candidate {
                        start,
                        end,
                        label: "Secret flag value",
                    });
                }
            }
        }
        header_candidates(action, inner_start, inner_end, &mut candidates);
        url_candidates(action, inner_start, inner_end, &mut candidates);
        path_candidates(action, inner_start, inner_end, &mut candidates);
    }
    candidates.sort_by_key(|candidate| (candidate.start, candidate.end));
    let mut accepted = Vec::new();
    let mut last_end = 0;
    for candidate in candidates {
        if accepted.len() >= MAX_SUGGESTIONS {
            break;
        }
        if candidate.start < last_end || candidate.start >= candidate.end {
            continue;
        }
        let value = &action[candidate.start..candidate.end];
        let Some(placeholder) = placeholder_for(value) else {
            continue;
        };
        let expression = format!(
            "{}{{{{{placeholder}}}}}{}",
            &action[..candidate.start],
            &action[candidate.end..]
        );
        let pattern = PatternSpec {
            version: PATTERN_VERSION,
            kind: PatternKind::Template,
            expression,
        };
        if pattern.validate(action).is_err() {
            continue;
        }
        accepted.push(SecretSuggestion {
            start: action[..candidate.start].encode_utf16().count(),
            end: action[..candidate.end].encode_utf16().count(),
            label: candidate.label,
            placeholder,
        });
        last_end = candidate.end;
    }
    accepted
}

fn shell_words(action: &str) -> Option<Vec<Word>> {
    let mut words = Vec::new();
    let mut start = None;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in action.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if ch == '\'' || ch == '"' {
            match quote {
                Some(open) if open == ch => quote = None,
                None => quote = Some(ch),
                _ => (),
            }
        }
        if quote.is_none() && ";|&<>`".contains(ch) {
            return None;
        }
        if quote.is_none() && ch.is_whitespace() {
            if let Some(begin) = start.take() {
                words.push(Word {
                    start: begin,
                    end: index,
                });
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    if escaped || quote.is_some() {
        return None;
    }
    if let Some(begin) = start {
        words.push(Word {
            start: begin,
            end: action.len(),
        });
    }
    Some(words)
}

fn strip_quotes(action: &str, mut start: usize, mut end: usize) -> (usize, usize) {
    if end > start + 1 {
        let bytes = action.as_bytes();
        if matches!(bytes[start], b'\'' | b'"') && bytes[start] == bytes[end - 1] {
            start += 1;
            end -= 1;
        }
    }
    (start, end)
}

fn secret_key(key: &str) -> bool {
    let key = key
        .trim_start_matches('-')
        .to_ascii_lowercase()
        .replace('-', "_");
    matches!(
        key.as_str(),
        "token"
            | "secret"
            | "password"
            | "passwd"
            | "api_key"
            | "access_key"
            | "client_secret"
            | "auth_token"
            | "private_key"
    ) || key.ends_with("_token")
        || key.ends_with("_secret")
        || key.ends_with("_password")
}

fn header_candidates(action: &str, start: usize, end: usize, candidates: &mut Vec<Candidate>) {
    let text = &action[start..end];
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("authorization:") {
        if let Some(index) = lower.find("bearer ") {
            let value_start = start + index + "bearer ".len();
            candidates.push(Candidate {
                start: value_start,
                end,
                label: "Bearer token",
            });
        }
    } else if lower.starts_with("x-api-key:") {
        let value_start = start + "x-api-key:".len() + text["x-api-key:".len()..].len()
            - text["x-api-key:".len()..].trim_start().len();
        candidates.push(Candidate {
            start: value_start,
            end,
            label: "API key header",
        });
    }
}

fn url_candidates(action: &str, start: usize, end: usize, candidates: &mut Vec<Candidate>) {
    let text = &action[start..end];
    let lower = text.to_ascii_lowercase();
    let Some(scheme) = lower.find("https://").or_else(|| lower.find("http://")) else {
        return;
    };
    let authority_start = scheme
        + if lower[scheme..].starts_with("https://") {
            8
        } else {
            7
        };
    let authority_end = lower[authority_start..]
        .find(['/', '?', '#'])
        .map_or(lower.len(), |offset| authority_start + offset);
    let authority = &text[authority_start..authority_end];
    if let Some(at) = authority.find('@') {
        if let Some(colon) = authority[..at].find(':') {
            candidates.push(Candidate {
                start: start + authority_start + colon + 1,
                end: start + authority_start + at,
                label: "URL credential",
            });
        }
    }
    let mut cursor = authority_end;
    while cursor < text.len() {
        let Some(marker) = text[cursor..].find(['?', '&']) else {
            break;
        };
        cursor += marker + 1;
        let next = text[cursor..]
            .find(['&', '#'])
            .map_or(text.len(), |offset| cursor + offset);
        if let Some(equal) = text[cursor..next].find('=') {
            if secret_key(&text[cursor..cursor + equal]) {
                candidates.push(Candidate {
                    start: start + cursor + equal + 1,
                    end: start + next,
                    label: "URL query secret",
                });
            }
        }
        cursor = next;
    }
}

fn path_candidates(action: &str, start: usize, end: usize, candidates: &mut Vec<Candidate>) {
    let text = &action[start..end];
    if let Some(marker) = text.find("/secrets/") {
        let segment_start = marker + "/secrets/".len();
        let segment_end = text[segment_start..]
            .find('/')
            .map_or(text.len(), |offset| segment_start + offset);
        candidates.push(Candidate {
            start: start + segment_start,
            end: start + segment_end,
            label: "Secret path segment",
        });
    }
}

fn placeholder_for(value: &str) -> Option<&'static str> {
    if value.len() < 4
        || value.len() > 128
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "none" | "null" | "test" | "demo" | "dummy" | "example" | "redacted" | "placeholder"
        )
    {
        return None;
    }
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"_./:@%+=,-".contains(&byte))
    {
        Some("token")
    } else if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"_./:@%+=, -".contains(&byte))
    {
        Some("quoted")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_stable_nonoverlapping_secrets_without_copying_values() {
        let action = "API_TOKEN=abc123 curl -H 'Authorization: Bearer def456' 'https://api.test/?api_key=ghi789&mode=fast'";
        let suggestions = suggest_secret_spans("Bash", action);
        assert_eq!(
            suggestions
                .iter()
                .map(|hint| hint.label)
                .collect::<Vec<_>>(),
            vec!["Secret assignment", "Bearer token", "URL query secret"]
        );
        assert_eq!(
            suggestions
                .iter()
                .map(|hint| &action[hint.start..hint.end])
                .collect::<Vec<_>>(),
            vec!["abc123", "def456", "ghi789"]
        );
        assert!(!format!("{suggestions:?}").contains("abc123"));
    }

    #[test]
    fn rejects_ambiguous_shell_forms_and_public_examples() {
        for action in [
            "API_TOKEN=abc123; echo hi",
            "API_TOKEN='unclosed",
            "API_TOKEN=$HOME npm test",
            "API_TOKEN=demo npm test",
            "API_TOKEN=abc123\necho hi",
            "API_TOKEN=abc123 | cat",
            "API_TOKEN=abc123 echo $(whoami)",
            "curl --token --verbose https://site.test",
        ] {
            assert!(suggest_secret_spans("Bash", action).is_empty(), "{action}");
        }
    }

    #[test]
    fn handles_quoted_values_urls_paths_and_unicode_prefix_offsets() {
        let action = "echo 🔒 --password='with space'";
        let suggestions = suggest_secret_spans("Bash", action);
        assert_eq!(suggestions.len(), 1);
        let hint = &suggestions[0];
        assert_eq!(
            &action.encode_utf16().collect::<Vec<_>>()[hint.start..hint.end].to_vec(),
            &"with space".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(hint.placeholder, "quoted");
        assert_eq!(
            suggest_secret_spans("WebFetch", "https://user:pass123@host.test/?token=abc456").len(),
            2
        );
        assert_eq!(
            suggest_secret_spans("Read", "/project/secrets/private-key.pem").len(),
            1
        );
    }

    #[test]
    fn caps_multiple_suggestions_and_keeps_identical_results() {
        let action = "TOKEN=aaaa SECRET=bbbb API_KEY=cccc PASSWORD=dddd ACCESS_KEY=eeee npm test";
        let first = suggest_secret_spans("Bash", action);
        assert_eq!(first.len(), MAX_SUGGESTIONS);
        assert_eq!(first, suggest_secret_spans("Bash", action));
        assert!(first.windows(2).all(|pair| pair[0].end <= pair[1].start));
    }

    #[test]
    fn suggested_template_keeps_every_unsuggested_byte_literal() {
        let action = "API_TOKEN=abc123 npm test --readonly";
        let hint = &suggest_secret_spans("Bash", action)[0];
        let pattern = PatternSpec {
            version: PATTERN_VERSION,
            kind: PatternKind::Template,
            expression: format!(
                "{}{{{{{}}}}}{}",
                &action[..hint.start],
                hint.placeholder,
                &action[hint.end..]
            ),
        };
        assert!(pattern.validate(action).is_ok());
        assert!(pattern.matches("API_TOKEN=changed npm test --readonly"));
        assert!(!pattern.matches("API_TOKEN=changed npm publish --readonly"));
        assert!(!pattern.matches("API_TOKEN=changed npm test --readonly; echo unsafe"));
    }
}
