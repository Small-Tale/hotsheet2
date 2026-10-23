//! Store bootstrap, linking, format, import, and maintenance commands.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use hotsheet_cli::{git_init, run_import};
use hotsheet_model::{CloseReason, Ulid, parse_file, to_file_string};
use hotsheet_ticketing::{FsStore, ProviderConfigRegistry, StoreMetadata, ops};

use crate::{
    Cmd, binary_on_path, default_index_path, local_checkout_root, now_ts, settings_for_cli,
};

use super::{Ctx, misrouted};

pub(super) fn run(cmd: Cmd, ctx: &Ctx) -> Result<()> {
    let path = &ctx.path;
    match cmd {
        Cmd::Init {
            prefix,
            standalone,
            at,
            remote,
        } => cmd_init(path, &prefix, standalone, at.as_deref(), remote.as_deref()),
        Cmd::Bootstrap {
            project,
            store,
            prefix,
            remote,
            tools,
        } => cmd_bootstrap(
            &project,
            store.as_deref(),
            &prefix,
            remote.as_deref(),
            &tools,
        ),
        Cmd::Link { store } => cmd_link(&store),
        Cmd::ActivateFormat {
            acknowledge_pre_release_breakage,
        } => activate_format(path, acknowledge_pre_release_breakage),
        Cmd::Compatibility { json } => compatibility(path, json),
        Cmd::Import {
            file,
            prefix,
            diagnose_attachments,
            restore_attachment,
            confirm_omission,
        } => {
            if diagnose_attachments {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&hotsheet_cli::import_recovery::diagnose(
                        path, &file
                    )?)?
                );
                Ok(())
            } else if !restore_attachment.is_empty() || !confirm_omission.is_empty() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&hotsheet_cli::import_recovery::recover(
                        path,
                        &file,
                        &restore_attachment,
                        &confirm_omission
                    )?)?
                );
                Ok(())
            } else {
                cmd_import(path, &file, &prefix)
            }
        }
        Cmd::Checkout { cmd } => cmd_checkout(
            cmd,
            &hotsheet_cli::resolve_store_path(path.clone(), &ctx.cwd),
        ),
        Cmd::Sync => cmd_sync(path),
        Cmd::Doctor { project } => cmd_doctor(path, &project),
        Cmd::Reindex { index } => cmd_reindex(path, index),
        Cmd::Worklist => cmd_worklist(path, &ctx.cwd),
        Cmd::PurgeTrash {
            older_than_days,
            dry_run,
        } => cmd_purge_trash(path, &ctx.cwd, older_than_days, dry_run),
        Cmd::MergeDriver { base, ours, theirs } => cmd_merge_driver(&base, &ours, &theirs),
        _ => misrouted("store"),
    }
}

fn activate_format(path: &Path, acknowledged: bool) -> Result<()> {
    if !acknowledged {
        bail!(
            "format activation can break older pre-release HS2 processes; announce the change, stop them, then rerun with --acknowledge-pre-release-breakage"
        );
    }
    println!(
        "Activating a pre-release HS2 format boundary. Older HS2 processes may no longer open this store."
    );
    use std::io::Write as _;
    std::io::stdout().flush()?;
    FsStore::open(path)?.activate_current_format()?;
    println!(
        "Activated store format {}.",
        hotsheet_ticketing::STORE_SCHEMA_VERSION
    );
    Ok(())
}

fn compatibility(path: &Path, json: bool) -> Result<()> {
    let selected_store = path
        .join(hotsheet_ticketing::STORE_METADATA_FILE)
        .is_file()
        .then(|| std::fs::read(path.join(hotsheet_ticketing::STORE_METADATA_FILE)))
        .transpose()?
        .map(|bytes| serde_json::from_slice::<hotsheet_ticketing::StoreMetadata>(&bytes))
        .transpose()?
        .map(|metadata| metadata.schema_version);
    let value = serde_json::json!({
        "generation": "hs2",
        "application_version": env!("CARGO_PKG_VERSION"),
        "setup_assets_fingerprint": hotsheet_plugins::builtin_setup_assets_fingerprint(),
        "store_schema": {
            "min": 1,
            "max": hotsheet_ticketing::STORE_SCHEMA_VERSION,
            "creates": hotsheet_ticketing::STORE_SCHEMA_VERSION,
        },
        "selected_store_schema": selected_store,
    });
    if json {
        println!("{}", serde_json::to_string(&value)?);
    } else {
        println!(
            "Hot Sheet 2 CLI {} creates store schema {} and opens schemas 1–{}.",
            env!("CARGO_PKG_VERSION"),
            hotsheet_ticketing::STORE_SCHEMA_VERSION,
            hotsheet_ticketing::STORE_SCHEMA_VERSION
        );
        if let Some(schema) = selected_store {
            println!("Selected store uses schema {schema}.");
        }
    }
    Ok(())
}

pub(crate) fn cmd_bootstrap(
    project_input: &Path,
    store_input: Option<&Path>,
    prefix: &str,
    remote: Option<&str>,
    tools: &[String],
) -> Result<()> {
    let project = project_input
        .canonicalize()
        .with_context(|| format!("project path does not exist: {}", project_input.display()))?;
    let store_candidate = store_input.map_or_else(
        || PathBuf::from(format!("{}.hs2", project.display())),
        |path| {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(path)
            }
        },
    );
    let created = !store_candidate.exists();
    if created {
        FsStore::init(&store_candidate, &StoreMetadata::new(prefix)).with_context(|| {
            format!(
                "initializing standalone store at {}",
                store_candidate.display()
            )
        })?;
        git_init(&store_candidate);
    } else {
        FsStore::open(&store_candidate).with_context(|| {
            format!(
                "existing bootstrap destination is not a Hot Sheet store: {}",
                store_candidate.display()
            )
        })?;
    }
    hotsheet_cli::register_merge_driver(&store_candidate);
    let store = hotsheet_cli::link_store(&store_candidate, &project)?;

    // A newly initialized store contains its schema and merge attributes but no ticket
    // mutation has occurred yet, so the normal autocommit path has not run. Remote setup
    // immediately pushes HEAD; make that contract true before either the CLI or a client
    // offers the remote step. This also repairs an older bootstrap-created empty repo.
    let store_handle = FsStore::open(&store)?;
    if store_handle.head_commit().is_none() {
        store_handle
            .autocommit("Initialize Hot Sheet ticket repository")
            .context("creating the initial ticket-store commit")?;
        if store_handle.head_commit().is_none() {
            bail!("ticket repository setup did not create an initial Git commit");
        }
    }

    let repository = hotsheet_ticketing::git::command()
        .args([
            "-C",
            project.to_str().unwrap_or("."),
            "config",
            "--get",
            "remote.origin.url",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(
        hotsheet_plugins::hotsheet_home().join("checkouts.json"),
    );
    let checkout = registry.open_git_store(&project, None, repository, &store)?;
    hotsheet_ticketing::worklist::regenerate_checkout(&checkout)?;

    let existing_remote = hotsheet_ticketing::git::command()
        .args([
            "-C",
            store.to_str().unwrap_or("."),
            "remote",
            "get-url",
            "origin",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if let Some(url) = remote {
        if url.is_empty() || url.starts_with('-') || url.contains(['\r', '\n']) {
            bail!("enter a valid existing Git remote URL");
        }
        match existing_remote.as_deref() {
            Some(current) if current != url => bail!(
                "origin is already configured as {current}; refusing to replace existing user configuration"
            ),
            Some(_) => {}
            None => {
                let status = hotsheet_ticketing::git::command()
                    .args([
                        "-C",
                        store.to_str().unwrap_or("."),
                        "remote",
                        "add",
                        "origin",
                        url,
                    ])
                    .status()
                    .context("running git remote add origin")?;
                if !status.success() {
                    bail!("git remote add origin exited with {status}");
                }
            }
        }
    }

    let mut setup_reports = Vec::new();
    if tools.is_empty() {
        match hotsheet_cli::run_setup(&store, &project, None, true) {
            Ok(reports) => setup_reports = reports,
            Err(error)
                if error
                    .downcast_ref::<hotsheet_plugins::SetupError>()
                    .is_some_and(|error| {
                        matches!(
                            error,
                            hotsheet_plugins::SetupError::NoneDetected
                                | hotsheet_plugins::SetupError::NoneEnabled
                        )
                    }) =>
            {
                if matches!(
                    error.downcast_ref::<hotsheet_plugins::SetupError>(),
                    Some(hotsheet_plugins::SetupError::NoneEnabled)
                ) {
                    println!(
                        "No detected AI tool is enabled by this project's `enabled_plugins` setting; project/store setup is complete."
                    );
                } else {
                    println!(
                        "No supported AI tools were detected; project/store setup is complete."
                    );
                }
                println!(
                    "Run `hotsheet-cli bootstrap --project {} --store {} --tool <tool>` after installing Claude, Codex, OpenCode, or another registered tool.",
                    shell_quote_path(&project),
                    shell_quote_path(&store)
                );
            }
            Err(error) => return Err(error),
        }
    } else {
        let mut seen = HashSet::new();
        for tool in tools.iter().filter(|tool| seen.insert((*tool).clone())) {
            setup_reports.extend(hotsheet_cli::run_setup(
                &store,
                &project,
                Some(tool),
                false,
            )?);
        }
    }
    for report in &setup_reports {
        println!("Set up {}:", report.tool);
        for path in &report.wrote {
            println!("  wrote {path}");
        }
    }

    println!(
        "{} standalone Hot Sheet store at {} and linked {} via {}.",
        if created { "Initialized" } else { "Reused" },
        store.display(),
        project.display(),
        hotsheet_cli::STORE_LINK
    );
    if remote.is_some() || existing_remote.is_some() {
        println!("Ticket-store remote: origin is configured.");
    } else {
        println!("Ticket-store remote: not configured.");
        println!("Connect an existing provider-neutral Git remote with:");
        println!(
            "  hotsheet-cli bootstrap --project {} --store {} --remote <url>",
            shell_quote_path(&project),
            shell_quote_path(&store)
        );
        println!("Then publish the first commit with:");
        println!("  git -C {} push -u origin HEAD", shell_quote_path(&store));
    }
    Ok(())
}

pub(crate) fn cmd_init(
    path: &PathBuf,
    prefix: &str,
    standalone: bool,
    at: Option<&Path>,
    remote: Option<&str>,
) -> Result<()> {
    if standalone {
        let project = std::env::current_dir()?;
        let name = project
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
            .context("current project directory has no usable name; pass --at <path>")?;
        let destination = at
            .map(PathBuf::from)
            .unwrap_or_else(|| hotsheet_plugins::hotsheet_home().join("stores").join(name));
        if destination.exists() {
            bail!(
                "standalone store destination already exists: {}; choose a new --at path or link the existing store",
                destination.display()
            );
        }
        FsStore::init(&destination, &StoreMetadata::new(prefix))
            .with_context(|| format!("initializing store at {}", destination.display()))?;
        git_init(&destination);
        hotsheet_cli::register_merge_driver(&destination);
        if let Some(url) = remote {
            let status = hotsheet_ticketing::git::command()
                .current_dir(&destination)
                .args(["remote", "add", "origin", url])
                .status()
                .context("running git remote add origin")?;
            if !status.success() {
                bail!("git remote add origin exited with {status}");
            }
        }
        let linked = hotsheet_cli::link_store(&destination, &project)?;
        println!(
            "Initialized standalone Hot Sheet store at {} and linked {} via {}",
            linked.display(),
            project.display(),
            hotsheet_cli::STORE_LINK
        );
        print_onboarding_report(&destination, &project);
        return Ok(());
    }

    FsStore::init(path, &StoreMetadata::new(prefix))
        .with_context(|| format!("initializing store at {}", path.display()))?;
    git_init(path);
    hotsheet_cli::register_merge_driver(path);
    println!("Initialized Hot Sheet store at {}", path.display());
    print_onboarding_report(path, path);
    Ok(())
}

/// Link the current directory to a standalone store (`hotsheet-cli link <store>`).
pub(crate) fn cmd_link(store: &Path) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let abs = hotsheet_cli::link_store(store, &cwd)?;
    let registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(
        hotsheet_plugins::hotsheet_home().join("checkouts.json"),
    );
    let checkout = registry.open_git_store(&cwd, None, None, &abs)?;
    hotsheet_ticketing::worklist::regenerate_checkout(&checkout)?;
    println!(
        "Linked {} → {} (via {})",
        cwd.display(),
        abs.display(),
        hotsheet_cli::STORE_LINK
    );
    Ok(())
}

pub(crate) fn cmd_sync(path: &Path) -> Result<()> {
    use hotsheet_ticketing::SyncReport;
    use hotsheet_ticketing::repository_size::{self, SizeLevel};
    FsStore::open(path)?; // validate it's a store before touching git
    // The level before the sync, so a sync that carries the store over a GitHub limit says
    // so once (HS2-P1GYWH). One `git count-objects`, no attachment walk.
    let before = repository_size::git_object_size(path)
        .map_or(SizeLevel::Ok, |size| SizeLevel::for_bytes(size.total_bytes));
    match hotsheet_ticketing::sync_once(path) {
        SyncReport::NoRemote => println!("Local-only store — no remote to sync."),
        SyncReport::UpToDate => println!("Already up to date."),
        SyncReport::Synced { pulled, pushed } => {
            let mut parts = Vec::new();
            if pulled {
                parts.push("pulled remote changes");
            }
            if pushed {
                parts.push("pushed local commits");
            }
            println!("Synced — {}.", parts.join(" + "));
            if let Some(line) = repository_size::git_object_size(path).and_then(|after| {
                repository_size::crossing_message("this ticket store", before, after)
            }) {
                eprintln!("warning: {line}. Run `hotsheet checkout repo-size` for details.");
            }
        }
        SyncReport::Offline => {
            println!("Remote unreachable — kept working locally; the next sync will retry.")
        }
        SyncReport::Conflict => bail!(
            "a ticket had a body change on both sides that couldn't auto-merge — resolve it \
             manually, then commit (the working tree was left clean; nothing was lost)"
        ),
    }
    Ok(())
}

pub(crate) fn cmd_doctor(path: &PathBuf, project: &Path) -> Result<()> {
    let store = FsStore::open(path)?;
    let meta = store.metadata()?;
    // Enumerate resiliently so a single unparseable file is reported as an issue rather
    // than aborting doctor on the first one — doctor is a diagnostic meant to surface
    // every problem, including corruption (HS2-9X9TZD / HS2-PRVPCQ).
    let listing = store.list_tickets_resilient()?;
    let tickets = listing.tickets;

    println!(
        "Store: {} (prefix {}, {} sharding)",
        path.display(),
        meta.ticket_prefix,
        meta.shard
    );
    println!("Tickets: {}", tickets.len());

    let mut issues = 0usize;

    if !listing.corrupt.is_empty() {
        println!("Corrupt: {}", listing.corrupt.len());
        for c in &listing.corrupt {
            let id =
                c.id.map(|i| i.to_string())
                    .unwrap_or_else(|| "unknown".into());
            let slug = c.slug.as_deref().unwrap_or("unknown");
            println!(
                "  ! corrupt ticket {} (id {id}, slug {slug}): {}",
                c.path.display(),
                c.error
            );
            issues += 1;
        }
    }

    let ids: HashSet<Ulid> = tickets.iter().map(|t| t.id).collect();

    let mut slug_counts = std::collections::HashMap::<&str, usize>::new();
    for t in &tickets {
        *slug_counts.entry(t.slug.as_str()).or_default() += 1;
    }
    for (slug, n) in &slug_counts {
        if *n > 1 {
            println!("  ! duplicate slug {slug} ({n} tickets)");
            issues += 1;
        }
    }

    for t in &tickets {
        for b in &t.blocked_by {
            if !ids.contains(b) {
                println!("  ! {} blocked_by unknown id {b}", t.slug);
                issues += 1;
            }
        }
        if let Some(d) = &t.duplicate_of {
            if Ulid::from_string(d).is_ok_and(|id| !ids.contains(&id)) {
                println!("  ! {} duplicate_of unknown id {d}", t.slug);
                issues += 1;
            }
        }
        if t.close_reason == Some(CloseReason::Duplicate) && t.duplicate_of.is_none() {
            println!("  ! {} close_reason=duplicate but no duplicate_of", t.slug);
            issues += 1;
        }
        if !t.created_at.is_valid() {
            println!(
                "  ! {} created_at not valid RFC3339: {}",
                t.slug, t.created_at
            );
            issues += 1;
        }
    }

    // The semantic merge driver must be registered, or git would text-merge ticket files
    // and could dump conflict markers over structured data (HS2-18, docs/02 §2.7).
    if !hotsheet_cli::merge_driver_registered(path) {
        println!(
            "  ! semantic merge driver not registered (missing .gitattributes \
             `merge=hotsheet-ticket` or its git config) — re-run `hotsheet-cli init` here to \
             register it, or git may text-merge ticket files"
        );
        issues += 1;
    }

    issues += print_repository_size(path, project);

    print_onboarding_report(path, project);
    if issues == 0 {
        println!("No issues found.");
        Ok(())
    } else {
        bail!("{issues} issue(s) found")
    }
}

/// The repository size section of `doctor` (HS2-P1GYWH): the registered checkout for
/// `project` when there is one (its code repository and every linked store), otherwise this
/// store alone. A repository over GitHub's 10 GB hard limit is an issue (GitHub rejects its
/// pushes); one over the 5 GB soft limit is a warning that does not fail doctor.
fn print_repository_size(store: &Path, project: &Path) -> usize {
    use hotsheet_ticketing::repository_size::{self, SizeLevel};
    let registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(
        hotsheet_plugins::hotsheet_home().join("checkouts.json"),
    );
    let lines: Vec<(SizeLevel, String)> = match registry.describe(&project.display().to_string()) {
        Ok(checkout) => {
            let report = repository_size::report(&checkout);
            println!(
                "Repository size: {} (checkout {}; `checkout repo-size` for details)",
                level_word(report.level),
                checkout.alias
            );
            repository_size::limit_messages(&report)
        }
        Err(_) => match repository_size::git_object_size(store) {
            Some(size) => {
                let level = SizeLevel::for_bytes(size.total_bytes);
                println!(
                    "Repository size: {} ({} of git objects in this store)",
                    level_word(level),
                    repository_size::format_bytes(size.total_bytes)
                );
                repository_size::limit_message("this ticket store", size.total_bytes, level)
                    .map(|line| vec![(level, line)])
                    .unwrap_or_default()
            }
            None => {
                println!("Repository size: not measured (the store is not in a git repository)");
                Vec::new()
            }
        },
    };
    let mut issues = 0;
    for (level, line) in lines {
        if level == SizeLevel::Critical {
            println!("  ! {line}");
            issues += 1;
        } else {
            println!("  warning: {line}");
        }
    }
    issues
}

fn level_word(level: hotsheet_ticketing::repository_size::SizeLevel) -> &'static str {
    match level {
        hotsheet_ticketing::repository_size::SizeLevel::Ok => "ok",
        hotsheet_ticketing::repository_size::SizeLevel::Warning => "WARNING",
        hotsheet_ticketing::repository_size::SizeLevel::Critical => "CRITICAL",
    }
}

/// Rebuild the disposable index from a full store walk (`hotsheet-cli reindex`).
pub(crate) fn cmd_reindex(path: &Path, index: Option<PathBuf>) -> Result<()> {
    let store = FsStore::open(path)?;
    let index_path = match index {
        Some(p) => p,
        None => default_index_path(&store)?,
    };
    // Same store_id derivation the server uses (the store root), so the rebuilt file is
    // the one the server reconciles against.
    let store_id = store
        .root()
        .canonicalize()
        .unwrap_or_else(|_| store.root().to_path_buf())
        .display()
        .to_string();
    let idx = hotsheet_index::Index::open(&index_path, store_id)?;
    let n = idx.rebuild_from_store(&store)?;
    println!("reindexed {n} ticket(s) → {}", index_path.display());
    // Index maintenance also drops files older schema generations left behind, unless a
    // running process (an older build's server) still holds them (HS2-Y0PAEM).
    let report =
        hotsheet_index::prune_stale_index_files(&hotsheet_plugins::hotsheet_home().join("index"))?;
    if let Some(summary) = report.summary() {
        println!("{summary}");
    }
    Ok(())
}

/// Regenerate the checkout-local worklist from all sources registered for this checkout.
pub(crate) fn cmd_worklist(path: &Path, cwd: &Path) -> Result<()> {
    let store = FsStore::open(path)?;
    let checkout_root = local_checkout_root(path, cwd);
    let output = checkout_root.join(hotsheet_ticketing::worklist::CHECKOUT_WORKLIST);
    let registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(
        hotsheet_plugins::hotsheet_home().join("checkouts.json"),
    );
    let registered = registry.list()?.into_iter().find(|checkout| {
        Path::new(&checkout.root).canonicalize().ok().as_ref()
            == checkout_root.canonicalize().ok().as_ref()
    });
    let n = if let Some(checkout) = registered {
        hotsheet_ticketing::worklist::regenerate_checkout(&checkout)?
    } else {
        hotsheet_ticketing::worklist::regenerate_for_project(&store, &checkout_root, &output)?
    };
    println!("wrote {} ({n} Up Next ticket(s))", output.display());
    Ok(())
}

/// The `merge=hotsheet-ticket` git driver: read git's base/ours/theirs, merge semantically,
/// write the result back to `ours` (git's `%A`). Exit 0 = clean, 1 = a body conflict git
/// should surface. Parse failure falls back to git's text merge so data is never lost.
pub(crate) fn cmd_merge_driver(base: &Path, ours: &Path, theirs: &Path) -> Result<()> {
    let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
    let (base_txt, ours_txt, theirs_txt) = (read(base), read(ours), read(theirs));

    let (b, o, t) = match (
        parse_file(&base_txt),
        parse_file(&ours_txt),
        parse_file(&theirs_txt),
    ) {
        (Ok(b), Ok(o), Ok(t)) => (b, o, t),
        _ => {
            // Unparseable input → git's plain text 3-way merge (never silent data loss).
            let (merged, clean) = git_merge_file(&ours_txt, &base_txt, &theirs_txt)?;
            std::fs::write(ours, merged)?;
            std::process::exit(if clean { 0 } else { 1 });
        }
    };

    let outcome = hotsheet_ticketing::merge_tickets(&b, &o, &t);
    let mut ticket = outcome.ticket;
    let clean = match &outcome.body {
        hotsheet_ticketing::BodyMerge::Resolved(_) => true,
        hotsheet_ticketing::BodyMerge::Conflict {
            base: bb,
            ours: oo,
            theirs: tt,
        } => {
            // Only the prose paragraph conflicts; frontmatter + notes stay cleanly merged.
            let (body, clean) = git_merge_file(oo, bb, tt)?;
            ticket.details = body;
            clean
        }
    };
    std::fs::write(ours, to_file_string(&ticket))?;
    std::process::exit(if clean { 0 } else { 1 });
}

pub(crate) fn cmd_purge_trash(
    path: &PathBuf,
    cwd: &Path,
    older_than_days: Option<u32>,
    dry_run: bool,
) -> Result<()> {
    let store = FsStore::open(path)?;
    let older_than_days = older_than_days
        .map(Ok)
        .unwrap_or_else(|| settings_for_cli(path, cwd).trash_cleanup_days())?;
    if dry_run {
        let due = ops::trash_due_for_purge(&store, &now_ts(), i64::from(older_than_days))?;
        println!(
            "Would purge {} Trash ticket(s) deleted {older_than_days}+ days ago",
            due.len()
        );
        for ticket in &due {
            println!("Would purge {} {}", ticket.slug, ticket.title);
        }
        return Ok(());
    }
    let purged = ops::purge_trash(&store, &now_ts(), i64::from(older_than_days))?;
    if purged.is_empty() {
        println!("Trash has no tickets deleted {older_than_days}+ days ago");
    }
    for ticket in &purged {
        println!("Purged {} {}", ticket.slug, ticket.title);
    }
    Ok(())
}

pub(crate) fn cmd_checkout(cmd: CheckoutCmd, store: &Path) -> Result<()> {
    use hotsheet_ticketing::checkouts::CheckoutRegistry;
    let registry = CheckoutRegistry::new(hotsheet_plugins::hotsheet_home().join("checkouts.json"));
    match cmd {
        CheckoutCmd::Register {
            root,
            alias,
            stores,
            repository,
        } => {
            let repository = repository.or_else(|| {
                hotsheet_ticketing::git::command()
                    .args(["-C", root.to_str()?, "config", "--get", "remote.origin.url"])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .map(|v| v.trim().to_owned())
                    .filter(|v| !v.is_empty())
            });
            let entry = match registry.describe(root.to_string_lossy().as_ref()) {
                Ok(_) => registry.open_sources(
                    &root,
                    alias.as_deref(),
                    repository,
                    stores
                        .into_iter()
                        .map(hotsheet_ticketing::checkouts::TicketSource::git)
                        .collect(),
                    None,
                    hotsheet_ticketing::checkouts::OpenSourceMode::Discovered,
                )?,
                Err(hotsheet_ticketing::checkouts::CheckoutError::NotFound(_)) => {
                    registry.register(&root, alias.as_deref(), repository, stores)?
                }
                Err(error) => return Err(error.into()),
            };
            super::setup::retry_pending_consolidation(store, Path::new(&entry.root));
            println!("{}", serde_json::to_string_pretty(&entry)?);
        }
        CheckoutCmd::List => println!("{}", serde_json::to_string_pretty(&registry.list()?)?),
        CheckoutCmd::Resolve { reference } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&registry.describe(&reference)?)?
            );
        }
        CheckoutCmd::Relocate {
            reference,
            root,
            alias,
        } => println!(
            "{}",
            serde_json::to_string_pretty(&registry.relocate(
                &reference,
                &root,
                alias.as_deref()
            )?)?
        ),
        CheckoutCmd::AddSource {
            reference,
            connection_id,
            provider,
            locator,
            default,
        } => {
            let source = if provider == "git" {
                let source = hotsheet_ticketing::checkouts::TicketSource::git(&locator);
                if source.connection_id != connection_id {
                    bail!("git source id must be {}", source.connection_id);
                }
                source
            } else {
                hotsheet_ticketing::checkouts::TicketSource {
                    connection_id,
                    provider,
                    locator,
                }
            };
            let store_root = FsStore::open(store)
                .map(|opened| opened.root().to_path_buf())
                .unwrap_or_else(|_| store.to_path_buf());
            let checkout = registry.add_registered_source(
                &ProviderConfigRegistry::new(store_root.join("providers.json")),
                &reference,
                source,
                default,
            )?;
            super::setup::retry_pending_consolidation(store, Path::new(&checkout.root));
            println!("{}", serde_json::to_string_pretty(&checkout)?);
        }
        CheckoutCmd::RemoveSource {
            reference,
            connection_id,
            json,
        } => {
            // The same workflow as Project Settings → Ticket sources → Remove (HS2-SM9PM8).
            let store_root = FsStore::open(store)
                .map(|opened| opened.root().to_path_buf())
                .unwrap_or_else(|_| store.to_path_buf());
            let report = hotsheet_ticketing::connection_removal::detach_source(
                &ProviderConfigRegistry::new(store_root.join("providers.json")),
                &registry,
                &reference,
                &connection_id,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                if report.unlinked {
                    println!(
                        "Removed ticket source '{connection_id}' from checkout {}.",
                        report.checkout_id
                    );
                }
                if report.removed_connection {
                    println!(
                        "No other project uses '{connection_id}'; its connection was deleted (the account stays signed in)."
                    );
                } else if !report.still_used_by.is_empty() {
                    println!("Still used by: {}.", report.still_used_by.join(", "));
                }
            }
        }
        CheckoutCmd::RenameSource {
            reference,
            connection_id,
            new_connection_id,
        } => println!(
            "{}",
            serde_json::to_string_pretty(&registry.rename_source(
                &reference,
                &connection_id,
                &new_connection_id,
            )?)?
        ),
        CheckoutCmd::SourceColor {
            reference,
            connection_id,
        } => print_checkout_source_color(&registry.describe(&reference)?, &connection_id)?,
        CheckoutCmd::SetSourceColor {
            reference,
            connection_id,
            color,
        } => print_checkout_source_color(
            &registry.set_source_color(&reference, &connection_id, &color)?,
            &connection_id,
        )?,
        CheckoutCmd::ReviewSource {
            reference,
            connection_id,
            store,
            yes,
        } => review_checkout_source(&registry, &reference, &connection_id, store, yes)?,
        CheckoutCmd::SetDefault {
            reference,
            connection_id,
            clear,
        } => {
            let selected = if clear {
                None
            } else {
                connection_id.as_deref()
            };
            let checkout = registry.set_default_source(&reference, selected)?;
            super::setup::retry_pending_consolidation(store, Path::new(&checkout.root));
            println!("{}", serde_json::to_string_pretty(&checkout)?);
        }
        CheckoutCmd::RepoSize { reference, json } => {
            let report =
                hotsheet_ticketing::repository_size::report(&registry.describe(&reference)?);
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!(
                    "{}",
                    hotsheet_ticketing::repository_size::render_text(&report)
                );
            }
        }
        CheckoutCmd::Compact {
            reference,
            connection_id,
            min_age_days,
            oversized_bytes,
            json,
            execute,
            backup,
            confirm,
        } => {
            let checkout = registry.describe(&reference)?;
            let source = checkout
                .source(&connection_id)
                .filter(|source| source.provider == "git")
                .ok_or_else(|| {
                    anyhow::anyhow!("{connection_id} is not a linked Git ticket source")
                })?;
            let root = PathBuf::from(&source.locator);
            if root.canonicalize()? == Path::new(&checkout.root).canonicalize()? {
                bail!("cannot rewrite a ticket store that is also the code repository");
            }
            let store = FsStore::open_without_maintenance(&root)?;
            let now = time::OffsetDateTime::now_utc();
            let plan = hotsheet_ticketing::repository_compaction::plan_store(
                &store,
                now,
                min_age_days,
                oversized_bytes,
            )?;
            if execute {
                let backup =
                    backup.ok_or_else(|| anyhow::anyhow!("--execute requires --backup"))?;
                let confirm =
                    confirm.ok_or_else(|| anyhow::anyhow!("--execute requires --confirm"))?;
                let result = hotsheet_ticketing::repository_compaction::execute_store(
                    &store, now, &plan, &backup, &confirm,
                )?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    println!(
                        "Rewrote {} attachment payloads on {}.",
                        result.candidates_removed, connection_id
                    );
                    println!("Backup: {}", result.backup);
                    println!(
                        "Git objects: {} before, {} after.",
                        result.git_bytes_before, result.git_bytes_after
                    );
                    for line in &result.recovery {
                        println!("- {line}");
                    }
                }
            } else if json {
                println!("{}", serde_json::to_string_pretty(&plan)?);
            } else {
                println!("Source: {connection_id} ({})", source.locator);
                println!("Branch/head: {} {}", plan.branch, plan.head);
                println!(
                    "Candidates: {} ({} current payload bytes)",
                    plan.candidates.len(),
                    plan.reclaimable_payload_bytes
                );
                for item in &plan.candidates {
                    println!(
                        "- {}: {} ({} bytes, {:?})",
                        item.ticket_slug, item.path, item.bytes, item.reason
                    );
                }
                println!(
                    "History rewrite changes commit IDs for collaborators. The original checkout remains stale after execution; every collaborator must re-clone or reset it after preserving local work."
                );
                println!(
                    "Use Git LFS for future large image and media attachments: install git-lfs, run `git lfs install`, and configure `git lfs track` in a dedicated ticket repository before adding new files."
                );
                println!(
                    "To execute after reviewing this plan, rerun with --execute --backup <new-path> --confirm 'rewrite {} {}'",
                    plan.branch, plan.fingerprint
                );
            }
        }
    }
    Ok(())
}

/// The headless twin of Ticket sources' "review its location" recovery: the same registry
/// workflows the server relink route calls, gated on an explicit --yes.
fn review_checkout_source(
    registry: &hotsheet_ticketing::checkouts::CheckoutRegistry,
    reference: &str,
    connection_id: &str,
    store: Option<PathBuf>,
    yes: bool,
) -> Result<()> {
    let checkout = registry.describe(reference)?;
    let Some(source) = checkout
        .source(connection_id)
        .filter(|s| s.provider == "git")
    else {
        bail!(
            "checkout {} has no linked Git source '{connection_id}'",
            checkout.id
        );
    };
    let recorded = checkout.store_instance_ids.get(connection_id).cloned();
    let destination = store.unwrap_or_else(|| PathBuf::from(&source.locator));
    let moved = destination.canonicalize().ok() != Path::new(&source.locator).canonicalize().ok();
    if recorded.is_some() && !moved {
        println!(
            "Source '{connection_id}' of checkout {} already has a recorded store identity; nothing to do.",
            checkout.id
        );
        return Ok(());
    }
    let destination_identity = FsStore::open_without_maintenance(&destination)
        .with_context(|| format!("{} is not a Hot Sheet ticket store", destination.display()))?
        .metadata()?
        .instance_id;
    println!("Checkout:          {} ({})", checkout.id, checkout.root);
    println!("Source:            {connection_id}");
    println!("Linked location:   {}", source.locator);
    println!("Reviewed location: {}", destination.display());
    println!(
        "Recorded identity: {}",
        recorded
            .as_deref()
            .unwrap_or("none (linked before identities were recorded)")
    );
    println!(
        "Store identity:    {}",
        destination_identity
            .as_deref()
            .unwrap_or("none yet (assigned on confirmation)")
    );
    if !yes {
        println!(
            "\nConfirm this is the project's ticket store, then rerun with --yes to record its identity."
        );
        return Ok(());
    }
    let updated = if recorded.is_some() {
        registry.relink_git_source(reference, connection_id, &destination)?
    } else {
        registry.recover_unverified_git_source(reference, connection_id, &destination)?
    };
    println!("{}", serde_json::to_string_pretty(&updated)?);
    Ok(())
}

pub(crate) fn cmd_import(path: &Path, file: &Path, prefix: &str) -> Result<()> {
    let summary = run_import(path, file, prefix)?;
    println!(
        "Imported {} ticket(s) ({} attachment file(s)), skipped {} already present.",
        summary.written, summary.attachments, summary.skipped
    );
    Ok(())
}

#[derive(Subcommand)]
pub(crate) enum CheckoutCmd {
    /// Register or update a checkout. Repeating --store records all ticket stores it uses.
    Register {
        root: PathBuf,
        #[arg(long)]
        alias: Option<String>,
        #[arg(long = "store")]
        stores: Vec<PathBuf>,
        #[arg(long)]
        repository: Option<String>,
    },
    /// List registered checkouts as JSON.
    List,
    /// Resolve an id, id prefix, alias, or path and print its JSON record.
    Resolve { reference: String },
    /// Move a registered checkout while preserving its durable project id.
    Relocate {
        reference: String,
        root: PathBuf,
        #[arg(long)]
        alias: Option<String>,
    },
    /// Associate a provider-neutral ticket source with a checkout.
    AddSource {
        reference: String,
        connection_id: String,
        provider: String,
        locator: String,
        #[arg(long)]
        default: bool,
    },
    /// Remove a ticket source from this project; removing the default clears it. Sources
    /// belong to projects: a connection no other checkout links is deleted from the -C
    /// store's providers.json too (its account stays signed in).
    RemoveSource {
        reference: String,
        connection_id: String,
        /// Print the removal report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Rename a non-Git source while retaining its old id as a durable alias.
    RenameSource {
        reference: String,
        connection_id: String,
        new_connection_id: String,
    },
    /// Inspect a linked source's project-local color as JSON.
    SourceColor {
        reference: String,
        connection_id: String,
    },
    /// Set a linked source's project-local color; use Gray (#6b7280) to restore the default.
    SetSourceColor {
        reference: String,
        connection_id: String,
        color: String,
    },
    /// Review a linked Git store that has no recorded identity, or relink a moved one, and
    /// record its identity only with --yes. Without --yes this only prints the review; a
    /// repeated --yes is a no-op once the identity is recorded (HS2-47AFBK).
    ReviewSource {
        reference: String,
        connection_id: String,
        /// The store's current location when it moved; defaults to the linked location.
        #[arg(long)]
        store: Option<PathBuf>,
        /// Confirm the reviewed store and record its identity.
        #[arg(long)]
        yes: bool,
    },
    /// Select the source used by unqualified creates, or clear it with --clear.
    SetDefault {
        reference: String,
        #[arg(required_unless_present = "clear")]
        connection_id: Option<String>,
        #[arg(long, conflicts_with = "connection_id")]
        clear: bool,
    },
    /// Report git object size (what a push to GitHub carries) for the code repository and
    /// each linked ticket store, plus each store's attachment payload. Entries at or over
    /// GitHub's 5 GB soft limit are flagged WARNING, and over its 10 GB hard limit CRITICAL.
    RepoSize {
        /// Checkout id, id prefix, alias, or path (default: the current directory).
        #[arg(default_value = ".")]
        reference: String,
        /// Print the report as JSON (the same shape as the server endpoint).
        #[arg(long)]
        json: bool,
    },
    /// Preview duplicate and oversized old attachments in a dedicated Git ticket store.
    /// Execution makes a mirror backup, rewrites a disposable clone, and force-pushes the
    /// reviewed branch only after an exact confirmation and remote-head lease.
    Compact {
        /// Qualified linked Git ticket source to compact.
        connection_id: String,
        #[arg(default_value = ".")]
        reference: String,
        #[arg(long, default_value_t = 90)]
        min_age_days: u64,
        #[arg(long, default_value_t = 10_000_000)]
        oversized_bytes: u64,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        execute: bool,
        /// New path for a complete bare mirror backup.
        #[arg(long)]
        backup: Option<PathBuf>,
        /// Exact string printed by the preview (`rewrite <branch> <fingerprint>`).
        #[arg(long)]
        confirm: Option<String>,
    },
}

pub(crate) fn shell_quote_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Read-only first-run guidance. Detection never writes tool configuration and never
/// opens/migrates HS1's database; the printed commands are explicit trust gates.
pub(crate) fn print_onboarding_report(store: &Path, project: &Path) {
    let detected: Vec<_> = hotsheet_plugins::all_plugins(&hotsheet_plugins::default_dirs())
        .into_iter()
        .filter(|plugin| {
            plugin
                .manifest
                .detection
                .binaries
                .iter()
                .any(|binary| binary_on_path(binary))
        })
        .map(|plugin| {
            (
                plugin.manifest.product_name.clone(),
                plugin.id().to_string(),
            )
        })
        .collect();
    print!("{}", render_onboarding_report(store, project, &detected));
}

pub(crate) fn render_onboarding_report(
    store: &Path,
    project: &Path,
    detected: &[(String, String)],
) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("\nOnboarding (read-only):\n");
    if detected.is_empty() {
        writeln!(out, "  AI tools: none detected on PATH; no setup was run").unwrap();
    } else {
        writeln!(
            out,
            "  AI tools detected (setup is explicit and idempotent):"
        )
        .unwrap();
        for (product, id) in detected {
            writeln!(
                out,
                "    - {}: hotsheet-cli -C \"{}\" setup {} --project \"{}\"",
                product,
                store.display(),
                id,
                project.display()
            )
            .unwrap();
        }
        writeln!(out, "  No tool configuration was changed.").unwrap();
    }

    let hs1 = project.join(".hotsheet/db/PG_VERSION");
    if hs1.is_file() {
        writeln!(
            out,
            "  ! Hot Sheet 1 PGlite data detected at {}",
            hs1.display()
        )
        .unwrap();
        writeln!(out, "    Close the Hot Sheet 1 project before migrating.").unwrap();
        writeln!(
            out,
            "    Review and run explicitly: hotsheet-migrate \"{}\" -C \"{}\"",
            project.join(".hotsheet").display(),
            store.display()
        )
        .unwrap();
        writeln!(
            out,
            "    Migration was not started; the HS1 source remains untouched."
        )
        .unwrap();
    } else {
        writeln!(
            out,
            "  Hot Sheet 1 data: not detected (looked for .hotsheet/db/PG_VERSION)"
        )
        .unwrap();
    }
    out
}

/// A plain text 3-way merge via `git merge-file` (returns the merged text — with conflict
/// markers when it can't resolve — and whether it was clean).
pub(crate) fn git_merge_file(ours: &str, base: &str, theirs: &str) -> Result<(String, bool)> {
    let dir = tempfile::tempdir()?;
    let write = |name: &str, s: &str| -> Result<PathBuf> {
        let p = dir.path().join(name);
        std::fs::write(&p, s)?;
        Ok(p)
    };
    let (op, bp, tp) = (
        write("ours", ours)?,
        write("base", base)?,
        write("theirs", theirs)?,
    );
    let out = hotsheet_ticketing::git::command()
        .args([
            "merge-file",
            "-p",
            "-L",
            "ours",
            "-L",
            "base",
            "-L",
            "theirs",
        ])
        .args([&op, &bp, &tp])
        .output()
        .context("running `git merge-file`")?;
    Ok((
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.success(),
    ))
}

pub(crate) fn print_checkout_source_color(
    checkout: &hotsheet_ticketing::checkouts::Checkout,
    connection_id: &str,
) -> Result<()> {
    if checkout.source(connection_id).is_none() {
        bail!(
            "ticket source '{connection_id}' is not linked to checkout {}",
            checkout.id
        );
    }
    let color = hotsheet_ticketing::checkouts::effective_source_color(
        checkout
            .source_colors
            .get(connection_id)
            .map(String::as_str),
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "checkout_id": checkout.id,
            "connection_id": connection_id,
            "color": color,
        }))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_is_read_only_and_distinguishes_hs1_by_its_database_marker() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let store = dir.path().join("store");
        std::fs::create_dir_all(project.join(".hotsheet/db")).unwrap();
        std::fs::write(project.join(".hotsheet/db/PG_VERSION"), "17").unwrap();
        let report =
            render_onboarding_report(&store, &project, &[("Claude Code".into(), "claude".into())]);

        assert!(report.contains("AI tools detected"));
        assert!(report.contains("setup claude --project"));
        assert!(report.contains("Hot Sheet 1 PGlite data detected"));
        assert!(report.contains("Close the Hot Sheet 1 project before migrating"));
        assert!(report.contains("hotsheet-migrate"));
        assert!(report.contains("Migration was not started"));
        assert_eq!(
            std::fs::read_to_string(project.join(".hotsheet/db/PG_VERSION")).unwrap(),
            "17"
        );
    }
}
