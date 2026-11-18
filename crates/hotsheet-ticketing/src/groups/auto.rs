//! Automatic grouping of new tickets (HS2-FN018Q): the one-turn prompt the server sends the
//! project's default AI tool, and the validation of its JSON reply. The rules match the app's
//! Add to Group › Automatic (HS2-4C32A0): an id must name a listed group, a new name must be a
//! valid group name, and a new name that already exists joins that group. Pure, so the server's
//! subscriber and its tests share one contract.

use std::collections::{BTreeMap, BTreeSet};

use hotsheet_model::{group_key, normalize_group_name};
use serde_json::{Value, json};

/// The first words of every automatic-grouping prompt; a test fake recognizes the turn by them.
pub const AUTO_GROUP_PROMPT_PREFIX: &str = "Choose a Hot Sheet ticket group for each new ticket";

/// The most tickets one turn describes; a larger burst is split into several turns.
pub const AUTO_GROUP_BATCH_LIMIT: usize = 40;

/// A new ticket as the prompt describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoGroupTicket {
    pub slug: String,
    pub title: String,
    pub category: String,
}

/// One validated choice for one ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoGroupChoice {
    /// An existing group's registry id.
    Existing { slug: String, id: String },
    /// A new group's normalized display name, to be created (or joined) in the registry.
    New { slug: String, name: String },
}

impl AutoGroupChoice {
    #[must_use]
    pub fn slug(&self) -> &str {
        match self {
            Self::Existing { slug, .. } | Self::New { slug, .. } => slug,
        }
    }
}

/// A reply the server cannot use: nothing is written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct AutoGroupReplyError(pub String);

/// The one-turn prompt: the instructions, then the new tickets and the project's groups as JSON.
#[must_use]
pub fn auto_group_prompt(tickets: &[AutoGroupTicket], groups: &BTreeMap<String, String>) -> String {
    let context = json!({
        "tickets": tickets
            .iter()
            .take(AUTO_GROUP_BATCH_LIMIT)
            .map(|ticket| json!({
                "slug": ticket.slug,
                "title": ticket.title,
                "category": ticket.category,
            }))
            .collect::<Vec<_>>(),
        "groups": groups
            .iter()
            .map(|(id, name)| json!({ "id": id, "name": name }))
            .collect::<Vec<_>>(),
    });
    [
        format!("{AUTO_GROUP_PROMPT_PREFIX} below."),
        "Prefer an existing group whose name fits a ticket. Only when none fits, propose a short new group name (at most 120 characters, one line); tickets that belong together may share a new name.".to_owned(),
        r#"Do not call tools, run commands, or change any data. Reply with one JSON object only: {"assignments": [{"slug": "<a ticket slug>", "group_id": "<an id from groups>"} or {"slug": "<a ticket slug>", "new_group": "<name>"}, ...]}. Leave out a ticket that fits no group."#.to_owned(),
        String::new(),
        context.to_string(),
    ]
    .join("\n")
}

/// The JSON object in a reply: the whole reply, a fenced block, or the first `{…}` span.
fn reply_object(reply: &str) -> Result<Value, AutoGroupReplyError> {
    let text = reply.trim();
    let fenced = text.find("```").and_then(|start| {
        let rest = &text[start + 3..];
        let rest = rest.strip_prefix("json").unwrap_or(rest);
        rest.find("```").map(|end| rest[..end].trim())
    });
    let span = match (text.find('{'), text.rfind('}')) {
        (Some(start), Some(end)) if end > start => Some(&text[start..=end]),
        _ => None,
    };
    for candidate in [Some(text), fenced, span].into_iter().flatten() {
        if let Ok(value) = serde_json::from_str::<Value>(candidate.trim()) {
            if value.is_object() {
                return Ok(value);
            }
        }
    }
    Err(AutoGroupReplyError(
        "the AI's answer was not the expected JSON".into(),
    ))
}

/// Validate a reply against the batch and the project's groups. Assignments for tickets not in
/// the batch, repeats of a ticket, unknown ids, and invalid names are dropped individually; a
/// reply with no usable object is an error. A new name whose key matches an existing group
/// becomes that group rather than a duplicate.
pub fn parse_auto_group_reply(
    reply: &str,
    tickets: &[AutoGroupTicket],
    groups: &BTreeMap<String, String>,
) -> Result<Vec<AutoGroupChoice>, AutoGroupReplyError> {
    let value = reply_object(reply)?;
    let Some(assignments) = value.get("assignments").and_then(Value::as_array) else {
        return Err(AutoGroupReplyError(
            "the AI's answer had no assignments list".into(),
        ));
    };
    let batch: BTreeSet<&str> = tickets.iter().map(|ticket| ticket.slug.as_str()).collect();
    let by_key: BTreeMap<String, &String> = groups
        .iter()
        .map(|(id, name)| (group_key(name), id))
        .collect();
    let mut seen = BTreeSet::new();
    let mut choices = Vec::new();
    for assignment in assignments {
        let Some(slug) = assignment
            .get("slug")
            .and_then(Value::as_str)
            .map(str::trim)
        else {
            continue;
        };
        if !batch.contains(slug) || !seen.insert(slug.to_owned()) {
            continue;
        }
        if let Some(id) = assignment.get("group_id").and_then(Value::as_str) {
            let id = id.trim();
            if groups.contains_key(id) {
                choices.push(AutoGroupChoice::Existing {
                    slug: slug.to_owned(),
                    id: id.to_owned(),
                });
            }
            continue;
        }
        if let Some(name) = assignment.get("new_group").and_then(Value::as_str) {
            let Ok(Some(name)) = normalize_group_name(name) else {
                continue;
            };
            choices.push(match by_key.get(&group_key(&name)) {
                Some(id) => AutoGroupChoice::Existing {
                    slug: slug.to_owned(),
                    id: (*id).clone(),
                },
                None => AutoGroupChoice::New {
                    slug: slug.to_owned(),
                    name,
                },
            });
        }
    }
    Ok(choices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tickets() -> Vec<AutoGroupTicket> {
        ["HS-1", "HS-2", "HS-3"]
            .into_iter()
            .map(|slug| AutoGroupTicket {
                slug: slug.into(),
                title: format!("Synthetic {slug}"),
                category: "task".into(),
            })
            .collect()
    }

    fn groups() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("g1".to_owned(), "Billing".to_owned()),
            ("g2".to_owned(), "Search".to_owned()),
        ])
    }

    #[test]
    fn hs2_fn018q_prompt_lists_tickets_and_groups_as_json() {
        let prompt = auto_group_prompt(&tickets(), &groups());
        assert!(prompt.starts_with(AUTO_GROUP_PROMPT_PREFIX));
        let context: Value = serde_json::from_str(prompt.lines().last().unwrap()).unwrap();
        assert_eq!(context["tickets"].as_array().unwrap().len(), 3);
        assert_eq!(context["tickets"][0]["slug"], "HS-1");
        assert_eq!(context["groups"][1], json!({"id": "g2", "name": "Search"}));
        let many: Vec<_> = (0..AUTO_GROUP_BATCH_LIMIT + 5)
            .map(|n| AutoGroupTicket {
                slug: format!("HS-{n}"),
                title: "t".into(),
                category: "bug".into(),
            })
            .collect();
        let context: Value =
            serde_json::from_str(auto_group_prompt(&many, &groups()).lines().last().unwrap())
                .unwrap();
        assert_eq!(
            context["tickets"].as_array().unwrap().len(),
            AUTO_GROUP_BATCH_LIMIT
        );
    }

    #[test]
    fn hs2_fn018q_reply_maps_ids_names_and_existing_names() {
        let reply = r#"Here you go:
```json
{"assignments": [
  {"slug": "HS-1", "group_id": "g1"},
  {"slug": "HS-2", "new_group": "  search  "},
  {"slug": "HS-3", "new_group": "Release   notes"}
]}
```"#;
        assert_eq!(
            parse_auto_group_reply(reply, &tickets(), &groups()).unwrap(),
            vec![
                AutoGroupChoice::Existing {
                    slug: "HS-1".into(),
                    id: "g1".into()
                },
                // An existing name, in any spelling, joins that group.
                AutoGroupChoice::Existing {
                    slug: "HS-2".into(),
                    id: "g2".into()
                },
                AutoGroupChoice::New {
                    slug: "HS-3".into(),
                    name: "Release notes".into()
                },
            ]
        );
    }

    #[test]
    fn hs2_fn018q_reply_drops_unusable_assignments_individually() {
        let reply = r#"{"assignments": [
            {"slug": "HS-9", "group_id": "g1"},
            {"slug": "HS-1", "group_id": "missing"},
            {"slug": "HS-2", "new_group": ""},
            {"slug": "HS-2", "new_group": "bad\u0007name"},
            {"slug": "HS-3", "group_id": "g2"},
            {"slug": "HS-3", "group_id": "g1"},
            {"group_id": "g1"}
        ]}"#;
        assert_eq!(
            parse_auto_group_reply(reply, &tickets(), &groups()).unwrap(),
            vec![AutoGroupChoice::Existing {
                slug: "HS-3".into(),
                id: "g2".into()
            }]
        );
    }

    #[test]
    fn hs2_fn018q_reply_without_assignments_is_an_error() {
        for reply in ["no json here", "[1, 2]", r#"{"group_id": "g1"}"#] {
            assert!(
                parse_auto_group_reply(reply, &tickets(), &groups()).is_err(),
                "{reply}"
            );
        }
        assert_eq!(
            parse_auto_group_reply(r#"{"assignments": []}"#, &tickets(), &groups()).unwrap(),
            vec![]
        );
    }
}
