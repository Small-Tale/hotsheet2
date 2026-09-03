//! The **derived `worklist.md`** (`docs/03` §3.6, docs/05 §5.9, HS2-90). HS1 generates a
//! Markdown worklist that any AI tool can read *without* the API; HS2 keeps that as a
//! **derived output** — regenerated (debounced) from the tickets on change, never a second
//! source of truth. It lives at `<checkout>/.hotsheet/worklist.md` and is **gitignored**.
//! A checkout may aggregate several stores; the stores sync normally while this local
//! projection is rebuilt from them.
//!
//! [`render`] is pure (tickets → Markdown) so it is unit-tested directly;
//! [`regenerate_checkout`] is the effectful writer used by clients and watchers.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use hotsheet_model::{Priority, Status, Ticket};

use crate::auto_context::{self, AutoContextEntry};
use crate::checkouts::Checkout;
use crate::ops::priority_rank;
use crate::settings::Settings;
use crate::store::{FsStore, StoreError};

/// The local derived file, relative to a code checkout.
pub const CHECKOUT_WORKLIST: &str = ".hotsheet/worklist.md";

/// Render active Up Next tickets in worker priority order. The file deliberately omits the
/// rest of the backlog: it is an executable queue, not a second ticket browser.
pub fn render(tickets: &[Ticket]) -> String {
    render_with_auto_context(tickets, &auto_context::defaults())
}

/// Render with an already-resolved effective auto-context list.
pub fn render_with_auto_context(tickets: &[Ticket], entries: &[AutoContextEntry]) -> String {
    let mut up_next: Vec<&Ticket> = tickets
        .iter()
        .filter(|ticket| ticket.up_next && ticket.status.is_active())
        .collect();
    up_next.sort_by(|a, b| {
        priority_rank(a.priority)
            .cmp(&priority_rank(b.priority))
            .then(b.created_at.as_str().cmp(a.created_at.as_str()))
    });

    let mut out = String::new();
    out.push_str("# Hot Sheet — Up Next\n\n");
    out.push_str("_Local projection of the current priority queue. Regenerated automatically; do not edit._\n\n");
    out.push_str("## Workflow\n\n");
    out.push_str(
        "Work these tickets in priority order, where reasonable. Before starting, read \
         the ticket in full and set it to `started`. Before completion, finish and verify \
         its scope, update required tests/coverage/docs, and scan for incomplete work. \
         Create every follow-up immediately, without asking: unfinished steps, open \
         questions, known gaps, out-of-scope work, and designed-but-unbuilt behavior each \
         get a ticket as soon as identified, never only a comment/TODO/note. Set the \
         current ticket to `completed` with a note containing the result, verification, \
         and every follow-up slug. Then run the repository's required gates, commit the \
         coherent ticket-sized change, and push it before starting another ticket. Use \
         `FEEDBACK NEEDED` only when a user decision or unavailable external state blocks \
         the current ticket; leave it `started` and name the blocker. FEEDBACK NEEDED \
         never replaces follow-ups for independently describable work. Do not set \
         `verified`; that is reserved for human review.\n\n",
    );
    out.push_str("Use `hotsheet-cli show <slug>` and `hotsheet-cli edit <slug> --status started|completed --note \"…\"`, or the equivalent Hot Sheet MCP tools. If neither is available, report that status could not be updated rather than silently skipping it.\n\n");
    out.push_str("## Tickets\n\n");
    if up_next.is_empty() {
        out.push_str("_(nothing queued)_\n\n");
    } else {
        for t in &up_next {
            out.push_str(&entry(t, entries));
        }
    }
    out
}

fn entry(ticket: &Ticket, entries: &[AutoContextEntry]) -> String {
    let mut out = line(ticket);
    for context in auto_context::resolve(ticket, entries) {
        for line in context.text.lines() {
            out.push_str("  > ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// One worklist row: `- [SLUG] Title · priority · status`.
fn line(t: &Ticket) -> String {
    format!(
        "- [{}] {} · {} · {}\n",
        t.slug,
        escape_inline_markdown(&t.title),
        priority_label(t.priority),
        status_label(t.status),
    )
}

/// Render user-authored text as one inert Markdown line. The ticket itself retains
/// the exact title; only this derived Markdown projection is escaped.
fn escape_inline_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\n' | '\r' => out.push(' '),
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '-' | '.'
            | '!' | '|' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn priority_label(p: Priority) -> &'static str {
    match p {
        Priority::Highest => "highest",
        Priority::High => "high",
        Priority::Default => "default",
        Priority::Low => "low",
        Priority::Lowest => "lowest",
    }
}

fn status_label(s: Status) -> &'static str {
    match s {
        Status::NotStarted => "not_started",
        Status::Started => "started",
        Status::Completed => "completed",
        Status::Verified => "verified",
        Status::Backlog => "backlog",
        Status::Archive => "archive",
        Status::Deleted => "deleted",
        Status::Moved => "moved",
    }
}

/// Regenerate a checkout-local worklist from one store. This is useful for an ad-hoc
/// checkout that has not entered the machine registry yet.
pub fn regenerate_to(store: &FsStore, path: &Path) -> Result<usize, StoreError> {
    let tickets = store.list_tickets()?;
    let n = tickets
        .iter()
        .filter(|ticket| ticket.up_next && ticket.status.is_active())
        .count();
    let entries = auto_context::effective(&Settings::new(store.root()))
        .map_err(|e| StoreError::Io(io::Error::new(io::ErrorKind::InvalidData, e)))?;
    let body = render_with_auto_context(&tickets, &entries);
    write_worklist(path, &body)?;
    Ok(n)
}

/// Regenerate the one local worklist for a registered checkout by aggregating all of its
/// linked git ticket stores. The stores remain authoritative/syncable; this projection is
/// machine-local and lives under the code checkout.
pub fn regenerate_checkout(checkout: &Checkout) -> Result<usize, StoreError> {
    let mut by_id: BTreeMap<hotsheet_model::Ulid, Ticket> = BTreeMap::new();
    let mut entries = Vec::new();
    for root in &checkout.stores {
        let store = FsStore::open(root)?;
        for ticket in store.list_tickets()? {
            match by_id.get(&ticket.id) {
                Some(existing) if existing.status != Status::Moved => {}
                _ => {
                    by_id.insert(ticket.id, ticket);
                }
            }
        }
        entries.extend(
            auto_context::effective(&Settings::new(store.root()))
                .map_err(|e| StoreError::Io(io::Error::new(io::ErrorKind::InvalidData, e)))?,
        );
    }
    let tickets: Vec<Ticket> = by_id.into_values().collect();
    let n = tickets
        .iter()
        .filter(|ticket| ticket.up_next && ticket.status.is_active())
        .count();
    let body = render_with_auto_context(&tickets, &entries);
    write_worklist(
        &PathBuf::from(&checkout.root).join(CHECKOUT_WORKLIST),
        &body,
    )?;
    Ok(n)
}

fn write_worklist(path: &Path, body: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.ends_with(CHECKOUT_WORKLIST) {
        let checkout = path
            .parent()
            .and_then(Path::parent)
            .expect("checkout worklist always has .hotsheet parent");
        let Some(ignore) = local_git_exclude(checkout) else {
            return std::fs::write(path, body);
        };
        let mut contents = std::fs::read_to_string(&ignore).unwrap_or_default();
        if !contents
            .lines()
            .any(|line| line.trim() == CHECKOUT_WORKLIST)
        {
            if !contents.is_empty() && !contents.ends_with('\n') {
                contents.push('\n');
            }
            contents.push_str(CHECKOUT_WORKLIST);
            contents.push('\n');
            if let Some(parent) = ignore.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(ignore, contents)?;
        }
    }
    std::fs::write(path, body)
}

/// Resolve git's machine-local exclude file without changing committed `.gitignore`.
/// This also follows the gitdir/commondir indirection used by linked worktrees.
fn local_git_exclude(checkout: &Path) -> Option<PathBuf> {
    let dot_git = checkout.join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let pointer = std::fs::read_to_string(dot_git).ok()?;
        let raw = pointer.strip_prefix("gitdir:")?.trim();
        let path = PathBuf::from(raw);
        if path.is_absolute() {
            path
        } else {
            checkout.join(path)
        }
    };
    let common = std::fs::read_to_string(git_dir.join("commondir"))
        .ok()
        .map(|value| {
            let path = PathBuf::from(value.trim());
            if path.is_absolute() {
                path
            } else {
                git_dir.join(path)
            }
        })
        .unwrap_or(git_dir);
    Some(common.join("info/exclude"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{NewTicket, TicketPatch, create, update};
    use hotsheet_model::{Timestamp, Ulid};

    fn ts(s: &str) -> Timestamp {
        Timestamp::new(format!("2026-08-22T00:00:0{s}Z"))
    }

    fn store() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &crate::store::StoreMetadata::new("HS")).unwrap();
        (dir, store)
    }

    #[test]
    fn render_includes_only_active_up_next_tickets() {
        let mk = |id: &str, title: &str, pri: Priority, up_next: bool, status: Status| {
            let mut t = Ticket::new(
                Ulid::from_string(id).unwrap(),
                format!("HS-{}", &id[..4]),
                title,
                "task",
                ts("0"),
                ts("0"),
            );
            t.priority = pri;
            t.up_next = up_next;
            t.status = status;
            t
        };
        let tickets = vec![
            mk(
                "01ARZ3NDEKTSV4RRFFQ69G5AAA",
                "low open",
                Priority::Low,
                false,
                Status::NotStarted,
            ),
            mk(
                "01ARZ3NDEKTSV4RRFFQ69G5BBB",
                "high queued",
                Priority::High,
                true,
                Status::Started,
            ),
            mk(
                "01ARZ3NDEKTSV4RRFFQ69G5CCC",
                "done",
                Priority::High,
                false,
                Status::Completed,
            ),
            mk(
                "01ARZ3NDEKTSV4RRFFQ69G5DDD",
                "default open",
                Priority::Default,
                false,
                Status::NotStarted,
            ),
            mk(
                "01ARZ3NDEKTSV4RRFFQ69G5EEE",
                "stale backlog flag",
                Priority::Highest,
                true,
                Status::Backlog,
            ),
        ];
        let md = render(&tickets);

        assert!(md.contains("## Workflow"));
        assert!(md.contains("## Tickets"));
        assert!(!md.contains("done"), "terminal statuses excluded");
        assert!(md.contains("high queued"));
        assert!(!md.contains("default open"));
        assert!(!md.contains("low open"));
        assert!(!md.contains("stale backlog flag"));
        assert!(!md.contains("## Open"));
    }

    #[test]
    fn render_requires_immediate_follow_ups_and_reserves_feedback_for_blockers() {
        let md = render(&[]);

        assert!(md.contains("Create every follow-up immediately, without asking"));
        assert!(md.contains("every follow-up slug"));
        assert!(md.contains("`FEEDBACK NEEDED` only when"));
        assert!(md.contains("FEEDBACK NEEDED never replaces follow-ups"));
    }

    #[test]
    fn regenerate_to_writes_a_checkout_local_projection() {
        let (checkout, store) = store();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5EEE").unwrap();
        create(
            &store,
            id,
            "HS",
            ts("0"),
            NewTicket {
                title: "alpha".into(),
                up_next: true,
                ..Default::default()
            },
        )
        .unwrap();

        let output = checkout.path().join("project/.hotsheet/worklist.md");
        regenerate_to(&store, &output).unwrap();
        let body = std::fs::read_to_string(&output).unwrap();
        assert!(body.contains("alpha"));
        assert!(!store.root().join("worklist.md").exists());

        // Closing the ticket drops it from the regenerated worklist.
        update(
            &store,
            &id,
            ts("1"),
            TicketPatch {
                status: Some(Status::Completed),
                ..Default::default()
            },
        )
        .unwrap();
        regenerate_to(&store, &output).unwrap();
        let body2 = std::fs::read_to_string(&output).unwrap();
        assert!(!body2.contains("alpha"), "dropped from the queue");
    }

    #[test]
    fn regenerate_checkout_aggregates_all_registered_stores_locally() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let first = FsStore::init(
            root.path().join("first.hs2"),
            &crate::store::StoreMetadata::new("ONE"),
        )
        .unwrap();
        let second = FsStore::init(
            root.path().join("second.hs2"),
            &crate::store::StoreMetadata::new("TWO"),
        )
        .unwrap();
        for (store, id, title) in [
            (&first, "01ARZ3NDEKTSV4RRFFQ69G5AAA", "from first"),
            (&second, "01ARZ3NDEKTSV4RRFFQ69G5BBB", "from second"),
        ] {
            create(
                store,
                Ulid::from_string(id).unwrap(),
                store.metadata().unwrap().ticket_prefix.as_str(),
                ts("0"),
                NewTicket {
                    title: title.into(),
                    category: "task".into(),
                    up_next: true,
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let checkout = Checkout {
            id: "project-test".into(),
            root: project.to_string_lossy().into_owned(),
            alias: "project".into(),
            repository: None,
            stores: vec![
                first.root().to_string_lossy().into_owned(),
                second.root().to_string_lossy().into_owned(),
            ],
            sources: Vec::new(),
            default_source: None,
        };

        assert_eq!(regenerate_checkout(&checkout).unwrap(), 2);
        let body = std::fs::read_to_string(project.join(CHECKOUT_WORKLIST)).unwrap();
        assert!(body.contains("from first"));
        assert!(body.contains("from second"));
        assert!(!first.root().join("worklist.md").exists());
        assert!(!second.root().join("worklist.md").exists());
    }

    #[test]
    fn render_escapes_user_authored_titles() {
        let mut ticket = Ticket::new(
            Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5EEF").unwrap(),
            "HS-SAFE",
            "[link](javascript:x) <script>\n# heading",
            "bug",
            ts("0"),
            ts("0"),
        );
        ticket.up_next = true;
        let rendered = render(&[ticket]);
        assert!(rendered.contains("\\[link\\]\\(javascript:x\\) &lt;script&gt; \\# heading"));
        assert!(!rendered.contains("<script>"));
        assert!(!rendered.contains("[link](javascript:x)"));
    }

    #[test]
    fn render_injects_default_and_tag_context_under_each_ticket() {
        let mut ticket = Ticket::new(
            Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5EFG").unwrap(),
            "HS-CONTEXT",
            "fix it",
            "bug",
            ts("0"),
            ts("0"),
        );
        ticket.up_next = true;
        ticket.tags = vec!["Security".into()];
        let mut entries = auto_context::defaults();
        entries.push(AutoContextEntry {
            source: crate::auto_context::AutoContextSource::Tag,
            key: "security".into(),
            text: "Threat model first.".into(),
        });
        let rendered = render_with_auto_context(&[ticket], &entries);
        assert!(rendered.contains("  > Reproduce the bug first"));
        assert!(rendered.contains("  > Threat model first."));
    }
}
