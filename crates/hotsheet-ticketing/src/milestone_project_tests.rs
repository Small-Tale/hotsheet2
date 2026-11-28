//! HS2-A820C1: milestones across a project's stores (docs/38 §38.2, §38.4, §38.6): the
//! list with roll-ups and unknown placeholders, link resolution, filters, Delete
//! Milestone… and Move Open Tickets to….

use hotsheet_model::{Status, Timestamp, Ulid};

use super::*;
use crate::milestone_rollup::{Health, RollupThresholds};
use crate::milestones::CloseReason;
use crate::{NewTicket, StoreMetadata, TicketPatch, ops};

struct Project {
    _dir: tempfile::TempDir,
    registry: MilestoneRegistry,
    stores: Vec<FsStore>,
}

impl Project {
    /// A checkout with a registry and two linked stores.
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let registry = MilestoneRegistry::for_checkout(&dir.path().join("checkout"));
        let stores = ["a", "b"]
            .iter()
            .map(|name| FsStore::init(dir.path().join(name), &StoreMetadata::new("HS")).unwrap())
            .collect();
        Self {
            _dir: dir,
            registry,
            stores,
        }
    }

    fn stores(&self) -> Vec<&FsStore> {
        self.stores.iter().collect()
    }

    fn members(&self) -> ProjectMembers {
        ProjectMembers::scan(&self.stores())
    }

    fn ticket(&self, store: usize, title: &str, milestone: Option<&str>) -> Ticket {
        ops::create(
            &self.stores[store],
            Ulid::new(),
            "HS",
            Timestamp::new("2026-11-01T00:00:00Z"),
            NewTicket {
                title: title.into(),
                category: "task".into(),
                details: "Details.".into(),
                milestone: milestone.map(str::to_string),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn set_status(&self, store: usize, ticket: &Ticket, status: Status) -> Ticket {
        ops::update(
            &self.stores[store],
            &ticket.id,
            Timestamp::new("2026-11-02T00:00:00Z"),
            TicketPatch {
                status: Some(status),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn read(&self, store: usize, ticket: &Ticket) -> Ticket {
        self.stores[store].read_ticket(&ticket.id).unwrap()
    }
}

fn clock() -> RollupClock {
    RollupClock {
        zone: ProjectTimeZone::utc(),
        now_ms: "2026-11-10T00:00:00Z"
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_millisecond(),
        thresholds: RollupThresholds::default(),
    }
}

#[test]
fn hs2_a820c1_listing_rolls_up_across_stores_and_lists_unknown_ids() {
    let p = Project::new();
    let empty = listing(Some(&p.registry), false, &p.members(), &clock()).unwrap();
    assert!(empty.registry && empty.milestones.is_empty());
    assert_eq!(empty.time_zone, "UTC");

    let m = p.registry.create("Toolchain 2.0", None, None).unwrap();
    let a = p.ticket(0, "a", Some(&m.id));
    p.ticket(1, "b", Some(&m.id));
    p.set_status(0, &a, Status::Completed);
    // A ticket naming an id this registry does not know (another project's store).
    p.ticket(1, "foreign", Some("ZZZZZZZZZZ"));
    p.ticket(1, "unlinked", None);

    let listed = listing(Some(&p.registry), false, &p.members(), &clock()).unwrap();
    let ids: Vec<(&str, MilestoneState, bool)> = listed
        .milestones
        .iter()
        .map(|s| {
            (
                s.milestone.id.as_str(),
                s.milestone.state,
                s.milestone.named,
            )
        })
        .collect();
    assert_eq!(
        ids,
        vec![
            (m.id.as_str(), MilestoneState::Open, true),
            ("ZZZZZZZZZZ", MilestoneState::Unknown, false),
        ]
    );
    let rollup = &listed.milestones[0].rollup;
    assert_eq!((rollup.progress.members, rollup.progress.finished), (2, 1));
    let connections: HashSet<String> = rollup
        .open_members
        .iter()
        .map(|member| member.connection_id.clone())
        .collect();
    assert_eq!(
        connections,
        HashSet::from([crate::git_connection_id(&p.stores[1])])
    );
    assert_eq!(listed.milestones[1].milestone.title, "Milestone ZZZZZZZZZZ");

    // The JSON keeps the registry fields at the top level beside `rollup`.
    let json = serde_json::to_value(&listed).unwrap();
    assert_eq!(json["milestones"][0]["title"], "Toolchain 2.0");
    assert_eq!(json["milestones"][0]["rollup"]["health"], "on_track");
    assert_eq!(json["milestones"][1]["state"], "unknown");

    // Closed milestones need `all`; without a registry every id is a placeholder.
    p.registry.close(&m.id, CloseReason::Completed).unwrap();
    let open_only = listing(Some(&p.registry), false, &p.members(), &clock()).unwrap();
    assert_eq!(open_only.milestones.len(), 1);
    let all = listing(Some(&p.registry), true, &p.members(), &clock()).unwrap();
    assert_eq!(all.milestones.len(), 2);
    assert_eq!(all.milestones[0].rollup.health, Health::Done);
    let storeless = listing(None, true, &p.members(), &clock()).unwrap();
    assert!(!storeless.registry);
    assert_eq!(storeless.milestones.len(), 2);
    assert!(
        storeless
            .milestones
            .iter()
            .all(|s| s.milestone.state == MilestoneState::Unknown)
    );
}

#[test]
fn hs2_a820c1_links_resolve_ids_and_titles_and_never_create() {
    let p = Project::new();
    let m = p.registry.create("Toolchain 2.0", None, None).unwrap();
    let link = |registry, input: &str| resolve_link(registry, Some(Some(input.to_string())));
    assert_eq!(
        link(Some(&p.registry), "toolchain  2.0").unwrap(),
        Some(Some(m.id.clone()))
    );
    assert_eq!(
        link(Some(&p.registry), &m.id).unwrap(),
        Some(Some(m.id.clone()))
    );
    assert_eq!(
        link(Some(&p.registry), "No Such Milestone")
            .unwrap_err()
            .code(),
        "milestone_not_found"
    );
    assert_eq!(p.registry.list().unwrap().len(), 1, "a link never creates");
    // Without a registry only ids are accepted.
    assert_eq!(
        link(None, "ZZZZZZZZZZ").unwrap(),
        Some(Some("ZZZZZZZZZZ".into()))
    );
    assert_eq!(
        link(None, "Toolchain 2.0").unwrap_err().code(),
        "milestone_registry_required"
    );
    // Clearing and leaving pass through; blank text clears.
    assert_eq!(resolve_link(None, Some(None)).unwrap(), Some(None));
    assert_eq!(resolve_link(None, None).unwrap(), None);
    assert_eq!(link(None, "  ").unwrap(), Some(None));
    // Duplicate titles (two clones) need the id.
    std::fs::write(
        p.registry.dir().join("4N8R2T6VWX.json"),
        "{\n  \"title\": \"Toolchain 2.0\",\n  \"state\": \"open\"\n}\n",
    )
    .unwrap();
    assert_eq!(
        link(Some(&p.registry), "Toolchain 2.0").unwrap_err().code(),
        "milestone_ambiguous"
    );
    // Filters match every id with the title, plus unknown ids by themselves.
    let mut ids = filter_ids(Some(&p.registry), "toolchain 2.0");
    ids.sort();
    let mut expected = vec!["4N8R2T6VWX".to_string(), m.id.clone()];
    expected.sort();
    assert_eq!(ids, expected);
    assert_eq!(
        filter_ids(Some(&p.registry), "ZZZZZZZZZZ"),
        vec!["ZZZZZZZZZZ"]
    );
    assert!(filter_ids(Some(&p.registry), "nothing").is_empty());
    assert!(filter_ids(None, "Toolchain 2.0").is_empty());
}

#[test]
fn hs2_a820c1_due_milestone_ids_are_open_milestones_due_by_the_date() {
    let p = Project::new();
    let soon = p.registry.create("Soon", None, Some("2026-12-01")).unwrap();
    let later = p
        .registry
        .create("Later", None, Some("2027-01-01"))
        .unwrap();
    p.registry.create("Undated", None, None).unwrap();
    assert_eq!(
        due_milestone_ids(Some(&p.registry), "2026-12-01"),
        vec![soon.id.clone()]
    );
    assert_eq!(due_milestone_ids(Some(&p.registry), "2027-06-01").len(), 2);
    p.registry.close(&soon.id, CloseReason::Completed).unwrap();
    assert_eq!(
        due_milestone_ids(Some(&p.registry), "2027-06-01"),
        vec![later.id]
    );
    assert!(due_milestone_ids(None, "2027-06-01").is_empty());
}

#[test]
fn hs2_a820c1_delete_clears_every_member_first_and_keeps_updated_at() {
    let p = Project::new();
    let m = p.registry.create("Toolchain 2.0", None, None).unwrap();
    let a = p.ticket(0, "a", Some(&m.id));
    let b = p.ticket(1, "b", Some(&m.id));
    let trashed = p.set_status(1, &p.ticket(1, "trashed", Some(&m.id)), Status::Deleted);
    let other = p.ticket(0, "other", Some("ZZZZZZZZZZ"));

    let outcome = delete_milestone(
        Some(&p.registry),
        &p.stores(),
        &p.members(),
        "Toolchain 2.0",
    )
    .unwrap();
    assert_eq!(outcome.updated, 3, "every member, Trash included");
    assert!(outcome.changed);
    assert_eq!(outcome.writes.len(), 2, "one write per store");
    assert!(p.registry.get(&m.id).unwrap().is_none(), "the file is gone");
    for (store, ticket) in [(0, &a), (1, &b), (1, &trashed)] {
        let read = p.read(store, ticket);
        assert_eq!(read.milestone, None);
        assert_eq!(read.updated_at, ticket.updated_at, "a link is not content");
    }
    assert_eq!(p.read(0, &other).milestone.as_deref(), Some("ZZZZZZZZZZ"));

    // Deleting again is refused: the milestone is gone.
    let again = delete_milestone(Some(&p.registry), &p.stores(), &p.members(), &m.id);
    assert!(matches!(
        again,
        Err(MilestoneOpError::Milestone(MilestoneError::NotFound(_)))
    ));
    // An unknown id only clears its members.
    let outcome =
        delete_milestone(Some(&p.registry), &p.stores(), &p.members(), "ZZZZZZZZZZ").unwrap();
    assert_eq!((outcome.updated, outcome.milestone.named), (1, false));
    assert_eq!(p.read(0, &other).milestone, None);
    // Without a registry delete is refused.
    assert!(matches!(
        delete_milestone(None, &p.stores(), &p.members(), "ZZZZZZZZZZ"),
        Err(MilestoneOpError::Milestone(
            MilestoneError::RegistryRequired
        ))
    ));
}

#[test]
fn hs2_a820c1_delete_refuses_a_conflicted_file_before_touching_tickets() {
    let p = Project::new();
    let m = p.registry.create("Toolchain 2.0", None, None).unwrap();
    let a = p.ticket(0, "a", Some(&m.id));
    std::fs::write(
        p.registry.dir().join(format!("{}.json", m.id)),
        "{\n<<<<<<< HEAD\n  \"title\": \"A\",\n=======\n  \"title\": \"B\",\n>>>>>>> x\n}\n",
    )
    .unwrap();
    let refused = delete_milestone(Some(&p.registry), &p.stores(), &p.members(), &m.id);
    assert!(matches!(
        refused,
        Err(MilestoneOpError::Milestone(MilestoneError::Conflict { .. }))
    ));
    assert_eq!(p.read(0, &a).milestone.as_deref(), Some(m.id.as_str()));
}

#[test]
fn hs2_a820c1_move_open_moves_only_unfinished_members_and_is_idempotent() {
    let p = Project::new();
    let from = p.registry.create("Alpha", None, None).unwrap();
    let to = p.registry.create("Beta", None, None).unwrap();
    let open = p.ticket(0, "open", Some(&from.id));
    let backlog = p.set_status(1, &p.ticket(1, "backlog", Some(&from.id)), Status::Backlog);
    let done = p.set_status(0, &p.ticket(0, "done", Some(&from.id)), Status::Completed);
    let dropped = ops::close(
        &p.stores[1],
        &p.ticket(1, "dropped", Some(&from.id)).id,
        Timestamp::new("2026-11-03T00:00:00Z"),
        hotsheet_model::CloseReason::NotPlanned,
        None,
    )
    .unwrap();

    let outcome = move_open_members(
        Some(&p.registry),
        &p.stores(),
        &p.members(),
        "alpha",
        "beta",
    )
    .unwrap();
    assert_eq!((outcome.updated, outcome.changed), (2, true));
    assert_eq!(p.read(0, &open).milestone.as_deref(), Some(to.id.as_str()));
    assert_eq!(
        p.read(1, &backlog).milestone.as_deref(),
        Some(to.id.as_str())
    );
    assert_eq!(
        p.read(0, &done).milestone.as_deref(),
        Some(from.id.as_str())
    );
    assert_eq!(
        p.read(1, &dropped).milestone.as_deref(),
        Some(from.id.as_str())
    );
    assert_eq!(p.read(0, &open).updated_at, open.updated_at);

    // Repeating moves nothing; moving onto itself moves nothing.
    let again = move_open_members(
        Some(&p.registry),
        &p.stores(),
        &p.members(),
        "alpha",
        "beta",
    )
    .unwrap();
    assert_eq!((again.updated, again.changed), (0, false));
    let same =
        move_open_members(Some(&p.registry), &p.stores(), &p.members(), "beta", "beta").unwrap();
    assert_eq!((same.updated, same.changed), (0, false));
    // An unknown target title is refused.
    assert!(matches!(
        move_open_members(
            Some(&p.registry),
            &p.stores(),
            &p.members(),
            "beta",
            "gamma"
        ),
        Err(MilestoneOpError::Milestone(MilestoneError::NotFound(_)))
    ));
}

#[test]
fn hs2_a820c1_show_finds_registry_milestones_and_unknown_ids() {
    let p = Project::new();
    let m = p
        .registry
        .create("Toolchain 2.0", None, Some("2026-11-05"))
        .unwrap();
    p.ticket(0, "late", Some(&m.id));
    p.ticket(1, "foreign", Some("ZZZZZZZZZZ"));
    let shown = show(Some(&p.registry), &p.members(), "toolchain 2.0", &clock()).unwrap();
    assert_eq!(shown.milestone.id, m.id);
    assert_eq!(shown.rollup.health, Health::OffTrack);
    assert!(shown.rollup.overdue);
    let unknown = show(Some(&p.registry), &p.members(), "ZZZZZZZZZZ", &clock()).unwrap();
    assert_eq!(unknown.milestone.state, MilestoneState::Unknown);
    assert_eq!(unknown.rollup.progress.members, 1);
    assert!(matches!(
        show(Some(&p.registry), &p.members(), "nothing", &clock()),
        Err(MilestoneError::NotFound(_))
    ));
}
