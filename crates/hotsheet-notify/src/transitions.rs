//! Ticket transitions into notification messages (HS2-R5HTZN, HS2-8VAXXA). The server's
//! router and the CLI's no-server path both call [`transition_messages`], so a status
//! change reads the same whichever process noticed it.

use crate::message::{EventKind, Message, Severity, TicketFacts};

/// The parts of a ticket a transition compares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketFlags {
    pub status: String,
    pub feedback_needed: bool,
    pub blocked: bool,
}

/// One ticket's current state, as either process reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketState {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub category: String,
    pub priority: String,
    pub tags: Vec<String>,
    pub blocked_reason: Option<String>,
    pub flags: TicketFlags,
}

/// Where the ticket lives: the store id (rule `projects` and dedupe keys), linked checkout
/// ids, and the display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub store: String,
    pub checkouts: Vec<String>,
    pub project_name: String,
}

/// `not_started` → `Not Started`.
pub fn label(status: &str) -> String {
    status
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Ticket text is quoted into Markdown; escape the characters that would restyle it.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '*' | '_' | '`' | '[' | ']' | '<' | '>' | '\\' | '#') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Messages for each transition into a notable state. `previous: None` means no baseline:
/// a newly created ticket compares against a quiet one, anything else reports nothing.
pub fn transition_messages(
    previous: Option<&TicketFlags>,
    current: &TicketState,
    created: bool,
    origin: &Origin,
) -> Vec<Message> {
    let quiet = TicketFlags {
        status: current.flags.status.clone(),
        feedback_needed: false,
        blocked: false,
    };
    let Some(previous) = previous.or(created.then_some(&quiet)) else {
        return Vec::new();
    };
    let now = &current.flags;
    let facts = TicketFacts {
        slug: current.slug.clone(),
        title: Some(current.title.clone()),
        status: Some(now.status.clone()),
        previous_status: Some(previous.status.clone()),
        category: Some(current.category.clone()),
        priority: Some(current.priority.clone()),
        tags: current.tags.clone(),
    };
    let base = |kind: EventKind, title: String, severity: Severity, key: String| {
        let mut message = Message::new(kind, title);
        message.body = format!("**{}**", escape(&current.title));
        message.severity = severity;
        message.project = Some(origin.store.clone());
        message.project_name = Some(origin.project_name.clone());
        message.checkouts = origin.checkouts.clone();
        message.store = Some(origin.store.clone());
        message.ticket = Some(facts.clone());
        message.dedupe_key = Some(key);
        message
    };
    let (store, id) = (&origin.store, &current.id);
    let mut messages = Vec::new();
    if previous.status != now.status {
        let mut message = base(
            EventKind::TicketStatusChanged,
            format!("{} is now {}", current.slug, label(&now.status)),
            Severity::Info,
            format!("status:{store}:{id}:{}", now.status),
        );
        message.body.push_str(&format!(
            "\n\n{} → {}",
            label(&previous.status),
            label(&now.status)
        ));
        messages.push(message);
    }
    if now.feedback_needed && !previous.feedback_needed {
        messages.push(base(
            EventKind::TicketFeedbackNeeded,
            format!("{} needs your feedback", current.slug),
            Severity::Warning,
            format!("feedback:{store}:{id}"),
        ));
    }
    if now.blocked && !previous.blocked {
        let mut message = base(
            EventKind::TicketBlocked,
            format!("{} is blocked", current.slug),
            Severity::Warning,
            format!("blocked:{store}:{id}"),
        );
        if let Some(reason) = current
            .blocked_reason
            .as_deref()
            .filter(|r| !r.trim().is_empty())
        {
            message
                .body
                .push_str(&format!("\n\n{}", escape(reason.trim())));
        }
        messages.push(message);
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(status: &str, feedback_needed: bool, blocked: bool) -> TicketFlags {
        TicketFlags {
            status: status.into(),
            feedback_needed,
            blocked,
        }
    }

    fn state(f: TicketFlags) -> TicketState {
        TicketState {
            id: "01ID".into(),
            slug: "HS2-1".into(),
            title: "Fix *it*".into(),
            category: "bug".into(),
            priority: "high".into(),
            tags: vec!["release".into()],
            blocked_reason: Some(" waiting on <review> ".into()),
            flags: f,
        }
    }

    fn origin() -> Origin {
        Origin {
            store: "s1".into(),
            checkouts: vec!["co-1".into()],
            project_name: "Rocket".into(),
        }
    }

    #[test]
    fn reports_each_transition_into_a_notable_state() {
        let previous = flags("not_started", false, false);
        let messages = transition_messages(
            Some(&previous),
            &state(flags("started", true, true)),
            false,
            &origin(),
        );
        let kinds: Vec<_> = messages.iter().map(|m| m.event).collect();
        assert_eq!(
            kinds,
            [
                EventKind::TicketStatusChanged,
                EventKind::TicketFeedbackNeeded,
                EventKind::TicketBlocked
            ]
        );
        assert_eq!(messages[0].title, "HS2-1 is now Started");
        assert_eq!(
            messages[0].body,
            "**Fix \\*it\\***\n\nNot Started → Started"
        );
        assert_eq!(
            messages[0].dedupe_key.as_deref(),
            Some("status:s1:01ID:started")
        );
        assert_eq!(
            messages[2].body,
            "**Fix \\*it\\***\n\nwaiting on \\<review\\>"
        );
        let m = &messages[1];
        assert_eq!(m.project_name.as_deref(), Some("Rocket"));
        assert_eq!(m.checkouts, ["co-1"]);
        let facts = m.ticket.as_ref().unwrap();
        assert_eq!(facts.previous_status.as_deref(), Some("not_started"));
        assert_eq!(facts.title.as_deref(), Some("Fix *it*"));
    }

    #[test]
    fn unchanged_staying_and_unknown_baselines_report_nothing() {
        let same = flags("started", true, true);
        assert!(
            transition_messages(Some(&same), &state(same.clone()), false, &origin()).is_empty()
        );
        assert!(transition_messages(None, &state(same.clone()), false, &origin()).is_empty());
        // Created: compared against a quiet ticket in the same status.
        let created = transition_messages(
            None,
            &state(flags("not_started", false, true)),
            true,
            &origin(),
        );
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].event, EventKind::TicketBlocked);
        // Leaving a state is not notable.
        assert!(
            transition_messages(
                Some(&same),
                &state(flags("started", false, false)),
                false,
                &origin()
            )
            .is_empty()
        );
    }

    #[test]
    fn labels_read_naturally() {
        assert_eq!(label("not_started"), "Not Started");
        assert_eq!(label("completed"), "Completed");
        assert_eq!(escape("a*b_[c]"), "a\\*b\\_\\[c\\]");
    }
}
