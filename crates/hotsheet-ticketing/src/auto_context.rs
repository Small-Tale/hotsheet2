//! Per-category and per-tag standing guidance injected into AI-facing ticket surfaces.

use hotsheet_model::Ticket;
use serde::{Deserialize, Serialize};

use crate::settings::{Settings, SettingsError};

/// The core-owned settings key. Its JSON shape is compatible with HS1.
pub const SETTING_KEY: &str = "auto_context";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoContextSource {
    Category,
    Tag,
}

/// A configured/default guidance entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoContextEntry {
    #[serde(rename = "type")]
    pub source: AutoContextSource,
    pub key: String,
    pub text: String,
}

/// One guidance block that applies to a ticket, retaining its provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TicketAutoContext {
    pub source: AutoContextSource,
    pub key: String,
    pub text: String,
}

impl From<&AutoContextEntry> for TicketAutoContext {
    fn from(value: &AutoContextEntry) -> Self {
        Self {
            source: value.source,
            key: value.key.clone(),
            text: value.text.clone(),
        }
    }
}

fn category(key: &str, text: &str) -> AutoContextEntry {
    AutoContextEntry {
        source: AutoContextSource::Category,
        key: key.into(),
        text: text.into(),
    }
}

/// HS1-compatible read-time defaults. They are never written to settings automatically.
pub fn defaults() -> Vec<AutoContextEntry> {
    vec![
        category(
            "issue",
            "Clarify the problem and its impact before acting. If it turns out to be a defect, add a test that guards against regressions; if it needs a larger change, immediately create follow-up tickets for the work without asking.",
        ),
        category(
            "bug",
            "Reproduce the bug first, then fix the root cause — not just the symptom. Add tests that would have caught it, covering BOTH the positive case (the fix behaves correctly) AND the negative / edge cases (bad input, error paths, boundaries) where applicable. Prefer both a unit test and an automated end-to-end test.",
        ),
        category(
            "feature",
            "Understand the existing architecture and conventions before implementing (read any overview / summary docs first). Add or update documentation for the new behavior. Add tests covering it. Use FEEDBACK NEEDED if ambiguity blocks the current ticket, and immediately create follow-up tickets without asking for anything left out of scope.",
        ),
        category(
            "requirement_change",
            "Update the requirements / spec docs to match the new behavior in the same change. New or updated tests may be required as well — add them where the behavior change warrants it. Ask clarifying questions if the desired behavior is ambiguous.",
        ),
        category(
            "task",
            "Make sure you understand the surrounding context before starting. Immediately create a follow-up ticket without asking for anything left incomplete.",
        ),
        category(
            "investigation",
            "Investigate and write up your findings; recommend concrete next steps. Don't implement the change yet — immediately create follow-up tickets without asking for the work you propose.",
        ),
        category(
            "concept",
            "Explore multiple directions before converging. Align with the existing design language and brand. Include rationale for the chosen concept, and ask clarifying questions on ambiguous requirements.",
        ),
        category(
            "revision",
            "Align with the existing design language and patterns. Address the specific requested changes; confirm you've understood the request before making them.",
        ),
        category(
            "feedback",
            "Make sure you understand the specific feedback and its intent before acting. Address each point, and ask clarifying questions if any point is ambiguous.",
        ),
        category(
            "asset",
            "Produce the asset to the required specs and formats. Follow the existing style and naming conventions; confirm dimensions / format before delivering.",
        ),
        category(
            "research",
            "Investigate and write up your findings with concrete recommendations. Don't start production work yet — immediately create follow-up tickets without asking for the work you propose.",
        ),
        category(
            "design",
            "Align with the existing design language and brand before proposing changes. Include mockups and rationale, and ask clarifying questions on ambiguous requirements.",
        ),
    ]
}

/// Load the effective configured array and layer it over built-ins by `type:key`.
/// An explicit empty-text entry remains present and suppresses that default at match time.
pub fn effective(settings: &Settings) -> Result<Vec<AutoContextEntry>, SettingsError> {
    let mut entries = defaults();
    for scope in [
        crate::settings::Scope::Global,
        crate::settings::Scope::Shared,
        crate::settings::Scope::Local,
    ] {
        let configured = match settings.get(SETTING_KEY, scope)? {
            Some(value) => {
                serde_json::from_value::<Vec<AutoContextEntry>>(value).map_err(|source| {
                    SettingsError::Invalid {
                        key: format!("{SETTING_KEY} ({scope:?})"),
                        source,
                    }
                })?
            }
            None => Vec::new(),
        };
        for item in configured {
            if let Some(existing) = entries
                .iter_mut()
                .find(|e| e.source == item.source && e.key == item.key)
            {
                *existing = item;
            } else {
                entries.push(item);
            }
        }
    }
    Ok(entries)
}

/// Resolve category first (exact match), then matching tags case-insensitively in
/// deterministic alphabetical-key order. Empty/whitespace text suppresses injection.
pub fn resolve(ticket: &Ticket, entries: &[AutoContextEntry]) -> Vec<TicketAutoContext> {
    resolve_fields(&ticket.category, &ticket.tags, entries)
}

/// Resolve from the fields carried by a compact query row.
pub fn resolve_fields(
    category: &str,
    ticket_tags: &[String],
    entries: &[AutoContextEntry],
) -> Vec<TicketAutoContext> {
    let mut out = Vec::new();
    if let Some(entry) = entries
        .iter()
        .find(|e| e.source == AutoContextSource::Category && e.key == category)
        && !entry.text.trim().is_empty()
    {
        out.push(entry.into());
    }
    let mut tags: Vec<&AutoContextEntry> = entries
        .iter()
        .filter(|e| {
            e.source == AutoContextSource::Tag
                && ticket_tags.iter().any(|t| t.eq_ignore_ascii_case(&e.key))
        })
        .collect();
    tags.sort_by(|a, b| a.key.cmp(&b.key));
    out.extend(
        tags.into_iter()
            .filter(|e| !e.text.trim().is_empty())
            .map(TicketAutoContext::from),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_model::{Timestamp, Ulid};
    use serde_json::json;

    fn ticket(category: &str, tags: &[&str]) -> Ticket {
        let ts = Timestamp::new("2026-08-25T00:00:00Z");
        let mut t = Ticket::new(Ulid::new(), "HS-TEST", "test", category, ts.clone(), ts);
        t.tags = tags.iter().map(|s| (*s).into()).collect();
        t
    }

    #[test]
    fn category_then_case_insensitive_tags_sorted_by_entry_key() {
        let entries = vec![
            category("bug", "category"),
            AutoContextEntry {
                source: AutoContextSource::Tag,
                key: "zebra".into(),
                text: "z".into(),
            },
            AutoContextEntry {
                source: AutoContextSource::Tag,
                key: "Alpha".into(),
                text: "a".into(),
            },
        ];
        let got = resolve(&ticket("bug", &["ZEBRA", "alpha"]), &entries);
        assert_eq!(
            got.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(),
            ["category", "a", "z"]
        );
    }

    #[test]
    fn unfinished_work_defaults_require_immediate_follow_ups_without_asking() {
        let entries = defaults();
        for category in ["issue", "feature", "task", "investigation", "research"] {
            let guidance = resolve(&ticket(category, &[]), &entries);
            assert_eq!(guidance.len(), 1, "missing {category} guidance");
            let text = guidance[0].text.to_ascii_lowercase();
            assert!(
                text.contains("immediately create") && text.contains("follow-up"),
                "{category} guidance must require immediate follow-ups"
            );
            assert!(
                text.contains("without asking"),
                "{category} guidance must forbid asking first"
            );
        }
    }

    #[test]
    fn override_and_empty_suppression_preserve_other_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::new(dir.path());
        settings
            .set(
                SETTING_KEY,
                json!([
                    {"type":"category", "key":"bug", "text":""},
                    {"type":"tag", "key":"security", "text":"threat model first"}
                ]),
                crate::settings::Scope::Shared,
            )
            .unwrap();
        let entries = effective(&settings).unwrap();
        assert!(resolve(&ticket("bug", &[]), &entries).is_empty());
        assert_eq!(
            resolve(&ticket("feature", &["SECURITY"]), &entries).len(),
            2
        );
    }

    #[test]
    fn category_is_exact_and_invalid_setting_is_reported() {
        assert!(resolve(&ticket("Bug", &[]), &defaults()).is_empty());
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::new(dir.path());
        settings
            .set(
                SETTING_KEY,
                json!({"bad":"shape"}),
                crate::settings::Scope::Local,
            )
            .unwrap();
        assert!(effective(&settings).is_err());
    }

    #[test]
    fn scopes_merge_per_entry_in_precedence_order() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::with_global_home(dir.path(), dir.path().join("home"));
        settings
            .set(
                SETTING_KEY,
                json!([{"type":"tag", "key":"global", "text":"g"}]),
                crate::settings::Scope::Global,
            )
            .unwrap();
        settings
            .set(
                SETTING_KEY,
                json!([
                    {"type":"tag", "key":"shared", "text":"s"},
                    {"type":"category", "key":"task", "text":"shared task"}
                ]),
                crate::settings::Scope::Shared,
            )
            .unwrap();
        settings
            .set(
                SETTING_KEY,
                json!([{"type":"category", "key":"task", "text":"local task"}]),
                crate::settings::Scope::Local,
            )
            .unwrap();
        let got = resolve(
            &ticket("task", &["GLOBAL", "shared"]),
            &effective(&settings).unwrap(),
        );
        assert_eq!(
            got.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
            ["local task", "g", "s"]
        );
    }
}
