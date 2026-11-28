//! Milestone commands (HS2-9PYNCN, HS2-A820C1, docs/38 §38.6): `milestones [--all]
//! [--json]` with roll-ups and `milestone create|show|edit|close|reopen|delete|move-open`,
//! plus the milestone resolution `new`, `edit`, `batch` and `ls` share. They call the same
//! `hotsheet_ticketing::milestones` registry and `milestone_project` workflows as the server
//! routes and MCP.
//!
//! The registry lives in the project's code checkout. The CLI finds it as it finds the group
//! registry: the nearest ancestor of the working directory linked to this store. Members are
//! read from every Git store a registered checkout links (else this store), and due dates use
//! the project time zone. Without a checkout (`-C <store>` from elsewhere) there is no
//! registry: the list shows only ids tickets name, ticket links take ids, and every registry
//! write is refused with `milestone_registry_required`. Each write commits as it returns.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;
use hotsheet_ticketing::FsStore;
use hotsheet_ticketing::milestone_project::{
    self as project, MemberOutcome, MilestoneSummary, MilestoneWriteResult, ProjectMembers,
    RollupClock,
};
use hotsheet_ticketing::milestones::{
    CloseReason, Milestone, MilestoneEdit, MilestoneError, MilestoneRegistry, MilestoneState,
    MilestoneWrite,
};
use hotsheet_ticketing::wire::TicketRow;

#[derive(Subcommand)]
pub(crate) enum MilestoneCmd {
    /// Create an open milestone in the project's registry. Prints its id. A title another
    /// milestone has (ignoring case and spacing) is refused.
    Create {
        /// The milestone's title (one line, at most 120 characters).
        title: String,
        /// Due date, `YYYY-MM-DD`.
        #[arg(long)]
        due: Option<String>,
        /// Read the Markdown description from this file (`-` for stdin).
        #[arg(long)]
        description_file: Option<String>,
    },
    /// Show one milestone.
    Show {
        /// The milestone's id or title.
        milestone: String,
        /// Print the milestone as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Change a milestone's title, description, or due date. Unchanged values commit nothing.
    Edit {
        /// The milestone's id or title.
        milestone: String,
        /// The new title.
        #[arg(long)]
        title: Option<String>,
        /// The new due date, `YYYY-MM-DD`.
        #[arg(long, conflicts_with = "clear_due")]
        due: Option<String>,
        /// Remove the due date.
        #[arg(long)]
        clear_due: bool,
        /// Read the new Markdown description from this file (`-` for stdin); an empty file
        /// clears it.
        #[arg(long)]
        description_file: Option<String>,
    },
    /// Close a milestone, as completed (the default) or not planned. Its tickets are not
    /// changed. Closing a closed milestone for the same reason changes nothing.
    Close {
        /// The milestone's id or title.
        milestone: String,
        /// `completed` or `not_planned`.
        #[arg(long, default_value = "completed")]
        reason: String,
    },
    /// Reopen a closed milestone. Reopening an open one changes nothing.
    Reopen {
        /// The milestone's id or title.
        milestone: String,
    },
    /// Delete a milestone: first clear it from every ticket (one commit per store), then
    /// delete its registry file. The tickets stay; an unknown id only has its tickets
    /// cleared.
    Delete {
        /// The milestone's id or title.
        milestone: String,
    },
    /// Move every open ticket of a milestone (not started, started, Backlog) to another
    /// milestone. Finished and dropped tickets stay.
    MoveOpen {
        /// The milestone to move from: id or title.
        from: String,
        /// The milestone to move to: id or title.
        to: String,
    },
}

fn checkout_root(store: &FsStore) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    super::groups::checkout_root_for(store.root(), &cwd)
}

/// The milestone registry of the project this CLI invocation runs in, if any.
fn registry_for(store: &FsStore) -> Option<MilestoneRegistry> {
    checkout_root(store).map(|root| MilestoneRegistry::for_checkout(&root))
}

/// The Git stores of the project: a registered checkout's linked stores, else this store.
fn project_stores(store: &FsStore) -> Vec<FsStore> {
    let canonical = |path: &Path| {
        hotsheet_sync::paths::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    };
    let linked = checkout_root(store).and_then(|root| {
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(
            hotsheet_plugins::hotsheet_home().join("checkouts.json"),
        )
        .list()
        .ok()?
        .into_iter()
        .find(|checkout| canonical(Path::new(&checkout.root)) == canonical(&root))
        .map(|checkout| {
            checkout
                .stores
                .iter()
                .filter_map(|path| FsStore::open(path).ok())
                .collect::<Vec<_>>()
        })
    });
    match linked {
        Some(stores) if !stores.is_empty() => stores,
        _ => vec![store.clone()],
    }
}

/// The project time zone in effect (UTC without a checkout).
fn clock_for(store: &FsStore) -> RollupClock {
    let zone = checkout_root(store)
        .and_then(|root| {
            hotsheet_ticketing::project_time_zone::project_time_zone(
                &hotsheet_ticketing::Settings::for_project(root),
            )
            .ok()
        })
        .unwrap_or_else(hotsheet_ticketing::project_time_zone::ProjectTimeZone::utc);
    if let Some(diagnostic) = zone.diagnostic() {
        eprintln!("warning: {diagnostic}");
    }
    RollupClock::now(zone)
}

/// Resolve the milestone a ticket write names (HS2-A820C1): an id or, inside a checkout, a
/// title of a listed milestone. An unknown title is refused; a link never creates one.
pub(crate) fn resolve_link(
    store: &FsStore,
    change: Option<Option<String>>,
) -> Result<Option<Option<String>>, MilestoneError> {
    if !matches!(change, Some(Some(_))) {
        return Ok(change);
    }
    project::resolve_link(registry_for(store).as_ref(), change)
}

/// The ids an `ls --milestone` filter matches.
pub(crate) fn filter_ids(store: &FsStore, input: &str) -> Vec<String> {
    project::filter_ids(registry_for(store).as_ref(), input)
}

/// The open milestones due on or before `date`, for `ls --due-before`.
pub(crate) fn due_milestone_ids(store: &FsStore, date: &str) -> Vec<String> {
    project::due_milestone_ids(registry_for(store).as_ref(), date)
}

/// Milestone titles for `ls` rows: the registry title, or `Milestone <id>` when unknown.
pub(crate) struct Titles(Vec<Milestone>);

impl Titles {
    pub(crate) fn for_rows(store: &FsStore, rows: &[TicketRow]) -> Self {
        if rows.iter().all(|row| row.milestone.is_none()) {
            return Self(Vec::new());
        }
        Self(
            registry_for(store)
                .and_then(|registry| registry.list().ok())
                .unwrap_or_default(),
        )
    }

    pub(crate) fn title(&self, id: &str) -> String {
        self.0
            .iter()
            .find(|milestone| milestone.id == id && milestone.named)
            .map_or_else(
                || hotsheet_ticketing::milestones::milestone_placeholder_title(id),
                |milestone| milestone.title.clone(),
            )
    }
}

fn require_registry(store: &FsStore) -> Result<MilestoneRegistry, MilestoneError> {
    registry_for(store).ok_or(MilestoneError::RegistryRequired)
}

fn read_description(source: &str) -> Result<String> {
    if source == "-" {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
            .context("read the description from stdin")?;
        return Ok(text);
    }
    std::fs::read_to_string(source).with_context(|| format!("read {source}"))
}

/// One line for a listing: id, state, due date, title, and flags.
fn row(milestone: &Milestone) -> String {
    let state = match (milestone.state, milestone.closed_reason) {
        (MilestoneState::Open, _) => "open".to_string(),
        (MilestoneState::Closed, Some(CloseReason::NotPlanned)) => "not planned".to_string(),
        (MilestoneState::Closed, _) => "closed".to_string(),
        (MilestoneState::Conflicted, _) => "CONFLICTED".to_string(),
        (MilestoneState::Unknown, _) => "unknown".to_string(),
    };
    let due = milestone.due_on.as_deref().unwrap_or("-");
    let mut flags = Vec::new();
    if milestone.duplicate {
        flags.push("duplicate title; use the id".to_string());
    }
    if let Some(conflict) = &milestone.conflict {
        flags.push(format!(
            "{} {}; resolve it with Git",
            conflict.file, conflict.detail
        ));
    }
    let flags = if flags.is_empty() {
        String::new()
    } else {
        format!("  ({})", flags.join("; "))
    };
    format!(
        "{:<10}  {:<11}  {:<10}  {}{flags}",
        milestone.id, state, due, milestone.title
    )
}

/// Progress and health for a listing row: `3 of 8 finished (37%), at_risk, 2 risk, 1 clarity`.
fn rollup_line(summary: &MilestoneSummary) -> String {
    let rollup = &summary.rollup;
    let progress = &rollup.progress;
    let mut parts = vec![match progress.percent {
        Some(percent) => format!(
            "{} of {} finished ({percent}%)",
            progress.finished, progress.members
        ),
        None => "no tickets".to_string(),
    }];
    parts.push(
        serde_json::to_value(rollup.health)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default(),
    );
    if !rollup.risk.is_empty() {
        parts.push(format!("{} risk", rollup.risk.len()));
    }
    if !rollup.clarity.is_empty() {
        parts.push(format!("{} clarity", rollup.clarity.len()));
    }
    if !progress.unavailable.is_empty() {
        parts.push(format!("unreadable: {}", progress.unavailable.join(", ")));
    }
    parts.join(", ")
}

pub(crate) fn cmd_milestones(path: &Path, json: bool, all: bool) -> Result<()> {
    let store = FsStore::open(path)?;
    let registry = registry_for(&store);
    let stores = project_stores(&store);
    let members = ProjectMembers::scan(&stores.iter().collect::<Vec<_>>());
    let listing = project::listing(registry.as_ref(), all, &members, &clock_for(&store))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&listing)?);
        return Ok(());
    }
    if let Some(sync) = listing
        .registry_sync
        .as_ref()
        .filter(|sync| sync.state.needs_attention())
    {
        eprintln!("Milestone registry not published: {}", sync.detail);
    }
    if !listing.registry {
        eprintln!("{}", MilestoneError::RegistryRequired);
    }
    if listing.milestones.is_empty() {
        println!("(no milestones)");
    }
    for summary in &listing.milestones {
        println!("{}", row(&summary.milestone));
        println!("            {}", rollup_line(summary));
    }
    Ok(())
}

fn print_write(verb: &str, write: &MilestoneWrite) {
    let milestone = &write.milestone;
    if write.changed {
        println!("{verb} milestone '{}' ({}).", milestone.title, milestone.id);
    } else {
        println!(
            "Milestone '{}' ({}) is unchanged.",
            milestone.title, milestone.id
        );
    }
}

/// Name the tickets still open after a close, and how to move them.
fn print_open_members(result: &MilestoneWriteResult) {
    let open = &result.open_members;
    if open.is_empty() {
        return;
    }
    let names: Vec<&str> = open.iter().map(|member| member.slug.as_str()).collect();
    println!(
        "{} open ticket{} remain: {}. Move them with `milestone move-open {} <milestone>`.",
        open.len(),
        if open.len() == 1 { "" } else { "s" },
        names.join(", "),
        result.milestone.milestone.id
    );
}

fn print_members(verb: &str, outcome: &MemberOutcome) {
    let tickets = if outcome.updated == 1 {
        "ticket"
    } else {
        "tickets"
    };
    match &outcome.to {
        Some(to) => println!(
            "{verb} {} open {tickets} from milestone '{}' to '{}'.",
            outcome.updated, outcome.milestone.title, to.title
        ),
        None => println!(
            "{verb} milestone '{}' ({}); cleared it from {} {tickets}.",
            outcome.milestone.title, outcome.milestone.id, outcome.updated
        ),
    }
}

pub(crate) fn cmd_milestone(path: &Path, cmd: MilestoneCmd) -> Result<()> {
    let store = FsStore::open(path)?;
    let stores = project_stores(&store);
    let store_refs: Vec<&FsStore> = stores.iter().collect();
    let members = || ProjectMembers::scan(&store_refs);
    // `show` reads unknown ids without a registry; every other command needs one.
    if let MilestoneCmd::Show { milestone, json } = &cmd {
        let summary = project::show(
            registry_for(&store).as_ref(),
            &members(),
            milestone,
            &clock_for(&store),
        )?;
        if *json {
            println!("{}", serde_json::to_string_pretty(&summary)?);
            return Ok(());
        }
        let milestone = &summary.milestone;
        println!("{}", row(milestone));
        println!("{}", rollup_line(&summary));
        if let Some(closed_at) = &milestone.closed_at {
            println!("Closed at {closed_at}");
        }
        for signal in summary.rollup.risk.iter().chain(&summary.rollup.clarity) {
            println!(
                "  {:<16} {:<12} {}",
                signal.code, signal.ticket.slug, signal.detail
            );
        }
        if let Some(description) = &milestone.description {
            println!();
            println!("{description}");
        }
        return Ok(());
    }
    let registry = require_registry(&store)?;
    match cmd {
        MilestoneCmd::Create {
            title,
            due,
            description_file,
        } => {
            let description = description_file
                .as_deref()
                .map(read_description)
                .transpose()?;
            let milestone = registry.create(&title, description.as_deref(), due.as_deref())?;
            println!(
                "Created milestone '{}' ({}).",
                milestone.title, milestone.id
            );
        }
        MilestoneCmd::Show { .. } => unreachable!("answered above"),
        MilestoneCmd::Edit {
            milestone,
            title,
            due,
            clear_due,
            description_file,
        } => {
            let id = registry.resolve(&milestone)?.id;
            let description = description_file
                .as_deref()
                .map(read_description)
                .transpose()?
                .map(|text| Some(text).filter(|text| !text.trim().is_empty()));
            let due_on = if clear_due { Some(None) } else { due.map(Some) };
            let write = registry.edit(
                &id,
                &MilestoneEdit {
                    title,
                    description,
                    due_on,
                },
            )?;
            print_write("Updated", &write);
        }
        MilestoneCmd::Close { milestone, reason } => {
            let reason = CloseReason::parse(&reason)?;
            let id = registry.resolve(&milestone)?.id;
            let write = registry.close(&id, reason)?;
            print_write("Closed", &write);
            print_open_members(&MilestoneWriteResult::of(
                write,
                &members(),
                &clock_for(&store),
            ));
        }
        MilestoneCmd::Reopen { milestone } => {
            let id = registry.resolve(&milestone)?.id;
            print_write("Reopened", &registry.reopen(&id)?);
        }
        MilestoneCmd::Delete { milestone } => {
            let outcome =
                project::delete_milestone(Some(&registry), &store_refs, &members(), &milestone)
                    .map_err(op_error)?;
            print_members("Deleted", &outcome);
        }
        MilestoneCmd::MoveOpen { from, to } => {
            let outcome =
                project::move_open_members(Some(&registry), &store_refs, &members(), &from, &to)
                    .map_err(op_error)?;
            print_members("Moved", &outcome);
        }
    }
    Ok(())
}

fn op_error(error: project::MilestoneOpError) -> anyhow::Error {
    match error {
        project::MilestoneOpError::Milestone(error) => error.into(),
        project::MilestoneOpError::Store(error) => error.into(),
    }
}
