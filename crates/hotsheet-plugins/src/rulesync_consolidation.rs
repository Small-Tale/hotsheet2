//! Queue a human-reviewable Rulesync consolidation task after instruction changes.

use std::path::Path;

use hotsheet_model::{Priority, Status, Timestamp, Ulid};
use hotsheet_ticketing::{
    FsStore, MutationContext, NewTicket, ProviderDraft, ProviderError, TicketProvider, TicketQuery,
    ops,
};
use sha2::{Digest, Sha256};

/// A project identifier, not its machine-local path, appears in ticket tags.
pub const TITLE: &str = "Consolidate Rulesync project guidance";
pub const DETAILS: &str = "Review `AGENTS.md` and `.rulesync/rules/project-local.md` after the Rulesync switch or rule update. Compare generated guidance with the local rule, preserve unique project policy, remove redundant or stale text only after review, then regenerate and check the instruction files. Do not copy customer content into the ticket or delete guidance without checking its source.";

pub fn tags(project: &Path) -> Vec<String> {
    let canonical = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());
    let hash = Sha256::digest(canonical.to_string_lossy().as_bytes());
    vec![
        "rulesync-consolidation".into(),
        format!("rulesync-project-{}", &format!("{hash:x}")[..16]),
    ]
}

fn revision_path(project: &Path, home: &Path) -> std::path::PathBuf {
    home.join("rulesync-reviews")
        .join(format!("{}.sha256", tags(project)[1]))
}

fn content_fingerprint(project: &Path) -> std::io::Result<Option<String>> {
    let mut digest = Sha256::new();
    let mut present = false;
    for (label, path) in [
        ("agents", project.join("AGENTS.md")),
        ("lock", project.join("rulesync.lock")),
    ] {
        digest.update(label.as_bytes());
        match std::fs::read(path) {
            Ok(bytes) => {
                present = true;
                digest.update([1]);
                digest.update(bytes.len().to_le_bytes());
                digest.update(bytes);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => digest.update([0]),
            Err(error) => return Err(error),
        }
    }
    Ok(present.then(|| format!("{:x}", digest.finalize())))
}

/// Detect a direct Rulesync update the next time Hot Sheet reads guidance.
pub fn needs_review(project: &Path, home: &Path) -> std::io::Result<bool> {
    let Some(current) = content_fingerprint(project)? else {
        return Ok(false);
    };
    match std::fs::read_to_string(revision_path(project, home)) {
        Ok(previous) => Ok(previous.trim() != current),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

/// Remember only a digest after a review ticket was queued successfully.
pub fn mark_reviewed(project: &Path, home: &Path) -> std::io::Result<()> {
    let Some(current) = content_fingerprint(project)? else {
        return Ok(());
    };
    let path = revision_path(project, home);
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut staged = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    use std::io::Write;
    writeln!(staged, "{current}")?;
    staged.persist(&path).map_err(|error| error.error)?;
    Ok(())
}

/// Reuse an open review task for this project; a later change after completion creates
/// a new task. This only writes to the supplied, already selected Git ticket store.
pub fn ensure_ticket(
    store: &FsStore,
    project: &Path,
) -> Result<String, hotsheet_ticketing::StoreError> {
    store.with_ticket_transaction(|| {
        let tags = tags(project);
        let existing = ops::query(
            store,
            &TicketQuery {
                tags: tags.clone(),
                open_only: true,
                ..TicketQuery::default()
            },
        )?;
        if let Some(ticket) = existing.first() {
            return Ok(ticket.slug.clone());
        }
        let prefix = store.metadata()?.ticket_prefix;
        let ticket = ops::create(
            store,
            Ulid::new(),
            &prefix,
            Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
            NewTicket {
                title: TITLE.into(),
                category: "task".into(),
                priority: Priority::Default,
                status: Status::NotStarted,
                details: DETAILS.into(),
                tags,
                up_next: true,
                blocked_by: vec![],
            },
        )?;
        Ok(ticket.slug)
    })
}

/// The same open-ticket rule for an external project source.
pub fn ensure_provider_ticket(
    provider: &dyn TicketProvider,
    project: &Path,
) -> Result<String, ProviderError> {
    let tags = tags(project);
    let existing = provider.query(&TicketQuery {
        tags: tags.clone(),
        open_only: true,
        ..TicketQuery::default()
    })?;
    if let Some(ticket) = existing.first() {
        return Ok(ticket.slug.clone());
    }
    let ticket = provider.create(
        MutationContext {
            now: Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
            generated_id: Ulid::new(),
        },
        ProviderDraft {
            title: TITLE.into(),
            category: "task".into(),
            priority: Priority::Default,
            status: Status::NotStarted,
            details: DETAILS.into(),
            tags,
            up_next: true,
            blocked_by: vec![],
            transfer: None,
        },
    )?;
    Ok(ticket.slug)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_ticketing::{GitProvider, StoreMetadata};

    #[test]
    fn hs2_34s102_reuses_open_ticket_and_creates_after_completion() {
        let project = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let store = FsStore::init(source.path(), &StoreMetadata::new("TEST")).unwrap();
        let first = ensure_ticket(&store, project.path()).unwrap();
        assert_eq!(ensure_ticket(&store, project.path()).unwrap(), first);
        let mut ticket = ops::query(&store, &TicketQuery::default())
            .unwrap()
            .into_iter()
            .find(|ticket| ticket.slug == first)
            .unwrap();
        assert_eq!(ticket.status, Status::NotStarted);
        assert!(ticket.up_next);
        assert!(ticket.details.contains("project-local.md"));
        assert!(
            !ticket
                .details
                .contains(&project.path().display().to_string())
        );
        ticket.status = Status::Completed;
        ticket.up_next = false;
        store.write_ticket_committing(&ticket).unwrap();
        let second = ensure_ticket(&store, project.path()).unwrap();
        assert_ne!(second, first);
        assert_eq!(ensure_ticket(&store, project.path()).unwrap(), second);
        let another_project = tempfile::tempdir().unwrap();
        assert_ne!(
            ensure_ticket(&store, another_project.path()).unwrap(),
            second,
            "projects sharing one store get distinct review tasks"
        );
    }

    #[test]
    fn hs2_34s102_provider_source_uses_same_review_contract() {
        let project = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let store = FsStore::init(source.path(), &StoreMetadata::new("TEST")).unwrap();
        let provider = GitProvider::new("linked-git", store);
        let first = ensure_provider_ticket(&provider, project.path()).unwrap();
        assert_eq!(
            ensure_provider_ticket(&provider, project.path()).unwrap(),
            first
        );
        let rows = provider
            .query(&TicketQuery {
                tags: tags(project.path()),
                open_only: true,
                ..TicketQuery::default()
            })
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].details, DETAILS);
    }

    #[test]
    fn hs2_34s102_interleaved_git_requests_share_one_follow_up() {
        let project = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let store = FsStore::init(source.path(), &StoreMetadata::new("TEST")).unwrap();
        let slugs = std::thread::scope(|scope| {
            (0..4)
                .map(|_| scope.spawn(|| ensure_ticket(&store, project.path()).unwrap()))
                .collect::<Vec<_>>()
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(slugs.iter().all(|slug| slug == &slugs[0]));
        assert_eq!(store.list_tickets().unwrap().len(), 1);
    }

    #[test]
    fn hs2_34s102_direct_rulesync_revision_detection_tracks_both_outputs() {
        let project = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        assert!(!needs_review(project.path(), home.path()).unwrap());
        std::fs::write(project.path().join("AGENTS.md"), "first").unwrap();
        assert!(needs_review(project.path(), home.path()).unwrap());
        mark_reviewed(project.path(), home.path()).unwrap();
        assert!(!needs_review(project.path(), home.path()).unwrap());
        std::fs::write(project.path().join("rulesync.lock"), "lock-v2").unwrap();
        assert!(needs_review(project.path(), home.path()).unwrap());
        mark_reviewed(project.path(), home.path()).unwrap();
        assert!(!needs_review(project.path(), home.path()).unwrap());
        std::fs::write(project.path().join("AGENTS.md"), "second").unwrap();
        assert!(needs_review(project.path(), home.path()).unwrap());
    }
}
