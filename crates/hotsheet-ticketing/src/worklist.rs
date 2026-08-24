//! The **derived `worklist.md`** (`docs/03` §3.6, docs/05 §5.9, HS2-90). HS1 generates a
//! Markdown worklist that any AI tool can read *without* the API; HS2 keeps that as a
//! **derived output** — regenerated (debounced) from the tickets on change, never a second
//! source of truth. It lives at `<store>/worklist.md` and is **gitignored** (it rebuilds
//! from the committed tickets, so committing it would only add churn).
//!
//! [`render`] is pure (tickets → Markdown) so it is unit-tested directly; [`regenerate`]
//! is the effectful writer the watcher calls.

use std::io;
use std::path::Path;

use hotsheet_model::{Priority, Status, Ticket};

use crate::ops::{is_open, priority_rank};
use crate::store::{FsStore, StoreError};

/// The derived file's name at the store root.
pub const WORKLIST_FILE: &str = "worklist.md";

/// Render the worklist Markdown from a set of tickets. Open tickets only (terminal/hidden
/// statuses are excluded), ordered the way a worker reads them: **Up Next first, then by
/// priority, then newest-first** within a priority. Deterministic — same input, same bytes.
pub fn render(tickets: &[Ticket]) -> String {
    let mut open: Vec<&Ticket> = tickets.iter().filter(|t| is_open(t)).collect();
    // Sort key: up_next (true first), then priority rank (highest first), then created
    // recency (newest first — ULIDs/timestamps sort chronologically).
    open.sort_by(|a, b| {
        (!a.up_next)
            .cmp(&!b.up_next)
            .then(priority_rank(a.priority).cmp(&priority_rank(b.priority)))
            .then(b.created_at.as_str().cmp(a.created_at.as_str()))
    });

    let mut out = String::new();
    out.push_str("# Worklist\n\n");
    out.push_str("_Derived from the tickets — regenerated on change. Do not edit by hand._\n\n");

    let up_next: Vec<&&Ticket> = open.iter().filter(|t| t.up_next).collect();
    out.push_str("## Up Next\n\n");
    if up_next.is_empty() {
        out.push_str("_(nothing queued)_\n\n");
    } else {
        for t in &up_next {
            out.push_str(&line(t));
        }
        out.push('\n');
    }

    out.push_str("## Open\n\n");
    let rest: Vec<&&Ticket> = open.iter().filter(|t| !t.up_next).collect();
    if rest.is_empty() {
        out.push_str("_(no other open tickets)_\n");
    } else {
        for t in &rest {
            out.push_str(&line(t));
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

/// Regenerate `<store>/worklist.md` from the store's current tickets and ensure it is
/// gitignored. Called by the watcher on change (debounced) and by `hotsheet-cli worklist`.
/// Returns the number of open tickets written.
pub fn regenerate(store: &FsStore) -> Result<usize, StoreError> {
    let tickets = store.list_tickets()?;
    let n = tickets.iter().filter(|t| is_open(t)).count();
    let body = render(&tickets);
    let path = store.root().join(WORKLIST_FILE);
    std::fs::write(&path, body)?;
    ensure_gitignored(store.root(), WORKLIST_FILE)?;
    Ok(n)
}

/// Add `name` to the store's `.gitignore` if it isn't already listed (derived output).
fn ensure_gitignored(root: &Path, name: &str) -> io::Result<()> {
    let gi = root.join(".gitignore");
    let existing = std::fs::read_to_string(&gi).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == name) {
        return Ok(());
    }
    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(name);
    out.push('\n');
    std::fs::write(&gi, out)
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
    fn render_orders_up_next_then_priority_and_excludes_terminal() {
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
        ];
        let md = render(&tickets);

        // Completed ticket is excluded (terminal).
        assert!(!md.contains("done"), "terminal statuses excluded");
        // Up Next section holds the queued one; Open holds the rest ordered by priority.
        let up = md.find("## Up Next").unwrap();
        let open = md.find("## Open").unwrap();
        let queued = md.find("high queued").unwrap();
        let def = md.find("default open").unwrap();
        let low = md.find("low open").unwrap();
        assert!(up < queued && queued < open, "queued sits under Up Next");
        assert!(
            open < def && def < low,
            "default (higher) precedes low in Open"
        );
    }

    #[test]
    fn regenerate_writes_and_gitignores() {
        let (_d, store) = store();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5EEE").unwrap();
        create(
            &store,
            id,
            "HS",
            ts("0"),
            NewTicket {
                title: "alpha".into(),
                ..Default::default()
            },
        )
        .unwrap();

        let n = regenerate(&store).unwrap();
        assert_eq!(n, 1);
        let body = std::fs::read_to_string(store.root().join(WORKLIST_FILE)).unwrap();
        assert!(body.contains("alpha"));
        let gi = std::fs::read_to_string(store.root().join(".gitignore")).unwrap();
        assert!(
            gi.lines().any(|l| l.trim() == WORKLIST_FILE),
            "worklist.md gitignored"
        );

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
        let n2 = regenerate(&store).unwrap();
        assert_eq!(n2, 0, "completed ticket no longer counted");
        let body2 = std::fs::read_to_string(store.root().join(WORKLIST_FILE)).unwrap();
        assert!(!body2.contains("alpha"), "dropped from Open");

        // Idempotent gitignore — regenerating twice doesn't duplicate the line.
        regenerate(&store).unwrap();
        let gi2 = std::fs::read_to_string(store.root().join(".gitignore")).unwrap();
        assert_eq!(gi2.lines().filter(|l| l.trim() == WORKLIST_FILE).count(), 1);
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
}
