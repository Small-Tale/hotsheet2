//! Git-backed ticket, note, attachment, transfer, and people commands.

use anyhow::Result;

use super::{Ctx, misrouted};
use crate::*;

pub(super) fn run(cmd: Cmd, ctx: &Ctx) -> Result<()> {
    let path = &ctx.path;
    let actor = ctx.actor.as_ref();
    match cmd {
        Cmd::New(args) => cmd_new(path, args),
        Cmd::Ls { filters } => cmd_ls(path, &filters),
        Cmd::Show { id } => cmd_show(path, &id),
        Cmd::Batch { ids, update } => cmd_batch(actor, path, &ids, update),
        Cmd::DeleteNote { id, note_id } => cmd_delete_note(path, &id, &note_id),
        Cmd::Attach(args) => cmd_attach(path, args),
        Cmd::AttachmentRename {
            id,
            attachment,
            filename,
        } => cmd_attachment_rename(path, &id, &attachment, &filename),
        Cmd::AttachmentDelete { id, attachment } => cmd_attachment_delete(path, &id, &attachment),
        Cmd::AttachmentMetadata {
            id,
            attachments,
            file,
        } => cmd_attachment_metadata(path, &id, &attachments, &file),
        Cmd::Annotate {
            id,
            attachment,
            file,
            clear,
            json,
        } => cmd_annotate(path, &id, &attachment, file.as_deref(), clear, json, actor),
        Cmd::AttachmentActor {
            id,
            attachment_ids,
            actor_role,
            actor_id,
            actor_name,
        } => cmd_attachment_actor(
            path,
            &id,
            &attachment_ids,
            &actor_role,
            actor_id,
            actor_name,
        ),
        Cmd::Edit { id, update } => cmd_edit(actor, path, &id, TicketEdit::prepare(update)?, false),
        Cmd::Close {
            id,
            reason,
            duplicate_of,
        } => cmd_close(actor, path, &id, &reason, duplicate_of),
        Cmd::Restore { id } => cmd_restore(path, &id),
        Cmd::Copy { id, to } => cmd_copy(path, &id, &to),
        Cmd::Move { id, to, yes } => cmd_move(path, &id, &to, yes),
        Cmd::Assign {
            id,
            to,
            clear,
            review,
        } => cmd_assign(path, &id, to, clear, review),
        Cmd::People { cmd } => cmd_people(path, cmd),
        Cmd::Read { id } => cmd_read(path, &id),
        Cmd::ConfidenceReport { json } => cmd_confidence_report(path, json),
        Cmd::Metrics {
            roll_up,
            prune_before,
            team,
        } => cmd_metrics(path, roll_up, prune_before, team),
        _ => misrouted("tickets"),
    }
}

pub(crate) fn cmd_new(path: &PathBuf, args: NewArgs) -> Result<()> {
    let NewArgs {
        title,
        title_flag,
        category,
        priority,
        details,
        up_next,
        tags,
        blocked_by,
    } = args;
    let priority = priority.as_str();
    let title = title
        .or(title_flag)
        .context("a title is required (positional or --title)")?;
    let store = FsStore::open(path)?;
    let prefix = store.metadata()?.ticket_prefix;
    let blocked_by = ops::resolve_blockers(&store, None, &blocked_by)?;
    let ticket = ops::create(
        &store,
        Ulid::new(),
        &prefix,
        now_ts(),
        NewTicket {
            title,
            category,
            priority: parse_priority(priority)?,
            status: Status::NotStarted,
            details: details.unwrap_or_default(),
            tags,
            up_next,
            blocked_by,
        },
    )?;
    println!(
        "Created {} ({})",
        ticket.slug,
        store.ticket_path(&ticket.id).display()
    );
    Ok(())
}

pub(crate) fn cmd_ls(path: &PathBuf, f: &LsFilters) -> Result<()> {
    let store = FsStore::open(path)?;
    let index = hotsheet_index::Index::open_reconciled(&default_index_path(&store)?, &store)?;
    // `me` in a person filter → this store's git user.email; an unresolvable `me` errors
    // rather than silently matching everyone (HS2-TCDTCH, docs/10 §10.3).
    let resolve_person = |v: &Option<String>| -> Result<Option<String>> {
        match v {
            None => Ok(None),
            Some(raw) if raw.eq_ignore_ascii_case(hotsheet_ticketing::ME) => {
                hotsheet_ticketing::current_user_email(path)
                    .map(Some)
                    .ok_or_else(|| anyhow::anyhow!("cannot resolve 'me': no git user.email set"))
            }
            Some(raw) => Ok(Some(raw.clone())),
        }
    };
    let page_after = match &f.page_after {
        Some(s) => Some(
            index
                .resolve_id(s)?
                .ok_or_else(|| anyhow::anyhow!("page-after: no ticket '{s}'"))?,
        ),
        None => None,
    };
    let query = TicketQuery {
        // Validate enum filters up front so a typo errors instead of matching none.
        status: f.status.as_deref().map(parse_status_str).transpose()?,
        priority: f.priority.as_deref().map(parse_priority).transpose()?,
        category: f.category.clone(),
        tags: f.tags.clone(),
        text: f.text.clone(),
        up_next_only: f.up_next,
        open_only: f.open,
        close_reason: f
            .close_reason
            .as_deref()
            .map(parse_close_reason)
            .transpose()?,
        closed: f.closed.then_some(true),
        assignee: resolve_person(&f.assignee)?,
        review_requested: resolve_person(&f.review_requested)?,
        review_by: resolve_person(&f.review_by)?,
        claimed: f.claimed.then_some(true),
        blocked: f.blocked_filter(),
        min_confidence: f.min_confidence,
        max_confidence: f.max_confidence,
        sort: f.sort.parse().map_err(|e: String| anyhow::anyhow!(e))?,
        limit: f.limit,
        page_after,
        ..Default::default()
    };
    let tickets = index.query(&query)?;

    if tickets.is_empty() {
        println!("(no tickets)");
        return Ok(());
    }
    // Per-user unread state comes from the local (gitignored) overlay (HS2-21).
    let overlay = hotsheet_ticketing::LocalOverlay::new(path.clone());
    for t in &tickets {
        let up = if t.up_next { "*" } else { " " };
        let is_unread =
            Ulid::from_string(&t.id)
                .ok()
                .is_some_and(|id| match overlay.last_read(&id) {
                    Ok(None) => true,
                    Ok(Some(read_at)) => t
                        .updated_at
                        .as_deref()
                        .is_none_or(|updated_at| updated_at > read_at.as_str()),
                    Err(_) => false,
                });
        let unread = if is_unread { "●" } else { " " };
        let confidence = t
            .latest_confidence
            .map(|score| format!("  [{score}% confidence]"))
            .unwrap_or_default();
        println!(
            "{up}{unread} {:<12} {:<12} {}{confidence}",
            t.slug,
            t.status.as_deref().unwrap_or_default(),
            t.title
        );
    }
    Ok(())
}

pub(crate) fn cmd_copy(src_path: &Path, id: &str, to: &Path) -> Result<()> {
    let src = FsStore::open(src_path)?;
    let dest =
        FsStore::open(to).with_context(|| format!("opening destination store {}", to.display()))?;
    let ticket =
        ops::resolve(&src, id)?.ok_or_else(|| anyhow::anyhow!("no ticket matching '{id}'"))?;
    let copied = ops::copy_ticket(&src, &dest, &ticket.id, Ulid::new(), now_ts())?;
    println!(
        "Copied {} → {} in {} (copied_from {})",
        ticket.slug,
        copied.slug,
        to.display(),
        ticket.slug
    );
    Ok(())
}

pub(crate) fn cmd_move(src_path: &Path, id: &str, to: &Path, yes: bool) -> Result<()> {
    let src = FsStore::open(src_path)?;
    let dest =
        FsStore::open(to).with_context(|| format!("opening destination store {}", to.display()))?;
    let ticket =
        ops::resolve(&src, id)?.ok_or_else(|| anyhow::anyhow!("no ticket matching '{id}'"))?;

    // Retention + exposure caveat (docs/02 §2.13) — git never forgets.
    eprintln!(
        "warning: moving {} does NOT remove it from {}'s git history or remote; and moving \
         INTO a shared store exposes it (the sync engine will push it). True purge needs a \
         manual history rewrite.",
        ticket.slug,
        src_path.display()
    );
    if !yes {
        bail!("re-run with --yes to confirm the move (see the retention/exposure warning above)");
    }

    // Until store ids exist (HS2-87), identify the destination by its canonical path.
    let dest_id = std::fs::canonicalize(to)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| to.display().to_string());
    let out = ops::move_ticket(&src, &dest, &ticket.id, &dest_id, now_ts())?;
    println!(
        "Moved {} → {} in {} (tombstone left in {})",
        ticket.slug,
        out.moved.slug,
        to.display(),
        src_path.display()
    );
    Ok(())
}

pub(crate) fn cmd_assign(
    path: &Path,
    id: &str,
    to: Vec<String>,
    clear: bool,
    review: Vec<String>,
) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket =
        ops::resolve(&store, id)?.ok_or_else(|| anyhow::anyhow!("no ticket matching '{id}'"))?;

    // `--clear` empties the set; otherwise `--to` (if any) replaces it; else leave unchanged.
    let set_assignees = if clear {
        Some(Vec::new())
    } else if !to.is_empty() {
        Some(to)
    } else {
        None
    };

    let now = now_ts();
    let reviews = review
        .iter()
        .map(|spec| {
            let (who, kind) = spec
                .split_once(':')
                .with_context(|| format!("--review expects email:kind, got '{spec}'"))?;
            Ok(ReviewRequest {
                who: who.to_string(),
                kind: parse_review_kind(kind)?,
                by: Ulid::new(),
                at: now.clone(),
                requested_by: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let t = ops::assign(&store, &ticket.id, now, set_assignees, reviews)?;
    let names = if t.assignees.is_empty() {
        "(none)".to_string()
    } else {
        t.assignees.join(", ")
    };
    println!(
        "Assigned {} → {names}{}",
        t.slug,
        if t.review_requests.is_empty() {
            String::new()
        } else {
            format!("  ({} review request(s))", t.review_requests.len())
        }
    );
    Ok(())
}

pub(crate) fn cmd_people(path: &Path, cmd: PeopleCmd) -> Result<()> {
    FsStore::open(path)?; // validate it's a store (people.json lives at its root)
    match cmd {
        PeopleCmd::List => {
            let roster = Roster::load(path)?;
            if roster.people.is_empty() {
                println!("(no people)");
            }
            for p in &roster.people {
                println!("{:<32} {}", p.email, p.name.as_deref().unwrap_or(""));
            }
        }
        PeopleCmd::Add {
            email,
            name,
            github,
        } => {
            let mut roster = Roster::load(path)?;
            roster.upsert(Person {
                email: email.clone(),
                name,
                github,
            });
            roster.save(path)?;
            println!("Added {email} to people.json (commit to share it with the team)");
        }
        PeopleCmd::SeedGithub { repo, api_base } => {
            if !repo
                .split_once('/')
                .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty() && !b.contains('/'))
            {
                bail!("--repo expects owner/name");
            }
            let token = hotsheet_ticketing::KeyRegistry::new(
                hotsheet_plugins::hotsheet_home(),
                hotsheet_ticketing::OsKeychain,
            )
            .get("github")?;
            let agent = ureq::AgentBuilder::new().user_agent("hotsheet2").build();
            let get = |url: &str| -> Result<serde_json::Value> {
                let response = agent
                    .get(url)
                    .set("Authorization", &format!("Bearer {token}"))
                    .set("Accept", "application/vnd.github+json")
                    .call()
                    .map_err(|e| anyhow::anyhow!("GitHub API: {e}"))?;
                Ok(serde_json::from_str(&response.into_string()?)?)
            };
            let collaborators = get(&format!(
                "{}/repos/{repo}/collaborators?per_page=100",
                api_base.trim_end_matches('/')
            ))?;
            let mut profiles = Vec::new();
            for login in collaborators
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.get("login").and_then(serde_json::Value::as_str))
            {
                let v = get(&format!("{}/users/{login}", api_base.trim_end_matches('/')))?;
                profiles.push(hotsheet_ticketing::roster::GitHubProfile {
                    login: login.to_owned(),
                    name: v
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    email: v
                        .get("email")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                });
            }
            let mut roster = Roster::load(path)?;
            let (changed, skipped) =
                hotsheet_ticketing::roster::seed_github_profiles(&mut roster, profiles);
            roster.save(path)?;
            println!(
                "Seeded {changed} people from GitHub; skipped {} without public email{}",
                skipped.len(),
                if skipped.is_empty() {
                    String::new()
                } else {
                    format!(": {}", skipped.join(", "))
                }
            );
        }
    }
    Ok(())
}

pub(crate) fn cmd_read(path: &PathBuf, id: &str) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket =
        ops::resolve(&store, id)?.ok_or_else(|| anyhow::anyhow!("no ticket matching '{id}'"))?;
    hotsheet_ticketing::LocalOverlay::new(path.clone()).mark_read(&ticket.id, &now_ts())?;
    println!("Marked {} read.", ticket.slug);
    Ok(())
}

/// Show the usage/cost rollup for the store (`hotsheet-cli metrics`, docs/14).
pub(crate) fn cmd_metrics(
    path: &Path,
    roll_up: Option<String>,
    prune_before: Option<String>,
    team: bool,
) -> Result<()> {
    use hotsheet_ticketing::metrics;
    let store = FsStore::open(path)?;

    if let Some(day) = roll_up.as_deref() {
        metrics::roll_up_through(&store, day)?;
        eprintln!("✔ rolled up through {day}");
    }
    if let Some(day) = prune_before.as_deref() {
        let n = metrics::prune_raw_before(&store, day)?;
        eprintln!("✔ pruned {n} settled raw file(s) before {day}");
    }

    // DB-free read path: settled rollup + live tail (or the cross-contributor team sum).
    let r = if team {
        metrics::team_summary(&store)?
    } else {
        metrics::summary_settled(&store)?
    };
    println!(
        "Usage{}: {} events · {} tokens in / {} out · ${:.4} total",
        if team { " (team)" } else { "" },
        r.events,
        r.tokens_in,
        r.tokens_out,
        r.cost_usd
    );
    if !r.by_model.is_empty() {
        println!("By model:");
        for (m, c) in &r.by_model {
            println!("  {m}: ${c:.4}");
        }
    }
    if !r.by_day.is_empty() {
        println!("By day:");
        for (d, c) in &r.by_day {
            println!("  {d}: ${c:.4}");
        }
    }
    Ok(())
}

pub(crate) fn cmd_show(path: &PathBuf, needle: &str) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = if let Ok(id) = Ulid::from_string(needle) {
        store.read_ticket(&id)?
    } else {
        let index = hotsheet_index::Index::open_reconciled(&default_index_path(&store)?, &store)?;
        let id = index
            .resolve_id(needle)?
            .with_context(|| format!("no ticket matching '{needle}'"))?;
        store.read_ticket(&id)?
    };
    print!("{}", to_file_string(&ticket));
    Ok(())
}

pub(crate) fn cmd_attach(path: &PathBuf, args: AttachArgs) -> Result<()> {
    let AttachArgs {
        id,
        files,
        json,
        batch_id,
        batch_label,
        actor_role,
        actor_id,
        actor_name,
        purpose,
    } = args;
    let (needle, files) = (id.as_str(), files.as_slice());
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, needle)?;
    let batch_id = Some(batch_id.unwrap_or_else(|| format!("batch-{}", Ulid::new())));
    let role = actor_role
        .or_else(|| std::env::var("HOTSHEET_ACTOR_ROLE").ok())
        .unwrap_or_else(|| "unknown".into());
    let role = parse_attachment_actor_role(&role)?;
    let purpose = purpose
        .as_deref()
        .map(|value| match value {
            "problem_evidence" => Ok(hotsheet_model::AttachmentPurpose::ProblemEvidence),
            "correctness_evidence" => Ok(hotsheet_model::AttachmentPurpose::CorrectnessEvidence),
            "reference" => Ok(hotsheet_model::AttachmentPurpose::Reference),
            "other" => Ok(hotsheet_model::AttachmentPurpose::Other),
            _ => anyhow::bail!(
                "--purpose must be problem_evidence, correctness_evidence, reference, or other"
            ),
        })
        .transpose()?;
    let actor = Some(hotsheet_model::AttachmentActor {
        identity: actor_id.or_else(|| std::env::var("HOTSHEET_ACTOR_ID").ok()),
        display_name: actor_name,
        role,
    });
    let mut attached = Vec::new();
    for file in files {
        let filename = file
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow::anyhow!("attachment path has no UTF-8 filename"))?;
        let bytes = std::fs::read(file)?;
        let attachment_id = Ulid::new();
        let (updated, written) = store.write_attachment_with_metadata(
            &ticket.id,
            attachment_id,
            now_ts(),
            filename,
            &bytes,
            hotsheet_model::AttachmentMetadata {
                batch_id: batch_id.clone(),
                batch_label: batch_label.clone(),
                actor: actor.clone(),
                purpose,
            },
        )?;
        let filename = updated
            .attachments
            .iter()
            .find(|attachment| attachment.id == attachment_id)
            .map(|attachment| attachment.filename.as_str())
            .unwrap_or(filename);
        if !json {
            println!("Attached {}", ops::attachment_reference(None, filename));
            println!(
                "Durable attachment id: {attachment_id} ({})",
                written.display()
            );
        }
        attached.push(serde_json::json!({"id": attachment_id.to_string(), "filename": filename}));
    }
    if json {
        println!("{}", serde_json::to_string(&attached)?);
    }
    Ok(())
}

pub(crate) fn cmd_attachment_rename(
    path: &Path,
    id: &str,
    attachment: &str,
    filename: &str,
) -> Result<()> {
    if filename.trim().is_empty() {
        bail!("filename is required");
    }
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let attachment_id = Ulid::from_string(attachment)
        .with_context(|| format!("invalid attachment ULID '{attachment}'"))?;
    let updated = store.rename_attachment(&ticket.id, &attachment_id, now_ts(), filename)?;
    println!("{}", serde_json::to_string_pretty(&updated)?);
    Ok(())
}

pub(crate) fn cmd_attachment_delete(path: &Path, id: &str, attachment: &str) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let attachment_id = Ulid::from_string(attachment)
        .with_context(|| format!("invalid attachment ULID '{attachment}'"))?;
    let updated = store.remove_attachment(&ticket.id, &attachment_id, now_ts())?;
    println!("{}", serde_json::to_string_pretty(&updated)?);
    Ok(())
}

pub(crate) fn cmd_attachment_metadata(
    path: &Path,
    id: &str,
    attachments: &[String],
    file: &Path,
) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let mut content = String::new();
    if file == Path::new("-") {
        std::io::stdin().read_to_string(&mut content)?;
    } else {
        content = std::fs::read_to_string(file)
            .with_context(|| format!("reading attachment metadata {}", file.display()))?;
    }
    let metadata: hotsheet_model::AttachmentMetadata = serde_json::from_str(&content)?;
    metadata.validate().map_err(anyhow::Error::msg)?;
    let mut seen = HashSet::new();
    let ids = attachments
        .iter()
        .map(|raw| {
            let id = Ulid::from_string(raw)
                .with_context(|| format!("invalid attachment ULID '{raw}'"))?;
            if !seen.insert(id) {
                bail!("attachment ids must be unique");
            }
            Ok(id)
        })
        .collect::<Result<Vec<_>>>()?;
    let updated = store.set_attachment_metadata(&ticket.id, &ids, metadata, now_ts())?;
    println!("{}", serde_json::to_string_pretty(&updated)?);
    Ok(())
}

pub(crate) fn cmd_annotate(
    path: &PathBuf,
    needle: &str,
    attachment: &str,
    file: Option<&Path>,
    clear: bool,
    json: bool,
    actor: Option<&hotsheet_ticketing::actor::MutationActor>,
) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, needle)?;
    let matches = ticket
        .attachments
        .iter()
        .filter(|item| item.id.to_string() == attachment || item.filename == attachment)
        .collect::<Vec<_>>();
    let [target] = matches.as_slice() else {
        bail!("attachment '{attachment}' must identify one attachment on {needle}");
    };
    let annotations: Vec<hotsheet_model::MediaAnnotation> = if clear {
        Vec::new()
    } else {
        let file = file.context("--file is required unless --clear is set")?;
        let input = if file == Path::new("-") {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            input
        } else {
            std::fs::read_to_string(file)?
        };
        let value: serde_json::Value = serde_json::from_str(&input)?;
        let value = value.get("annotations").cloned().unwrap_or(value);
        serde_json::from_value(value)?
    };
    let updated = store.set_attachment_annotations_with_activity(
        &ticket.id,
        &target.id,
        annotations,
        hotsheet_ticketing::actor::note_actor(actor),
        Ulid::new(),
        now_ts(),
    )?;
    let result = updated
        .attachments
        .iter()
        .find(|item| item.id == target.id)
        .context("updated attachment missing")?;
    if json {
        println!("{}", serde_json::to_string(result)?);
    } else {
        println!(
            "Updated annotations for {} ({})",
            result.filename, result.id
        );
    }
    Ok(())
}

pub(crate) fn cmd_attachment_actor(
    path: &PathBuf,
    needle: &str,
    attachment_ids: &[String],
    actor_role: &str,
    actor_id: Option<String>,
    actor_name: Option<String>,
) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, needle)?;
    let role = parse_attachment_actor_role(actor_role)?;
    let actor = Some(hotsheet_model::AttachmentActor {
        identity: actor_id.or_else(|| std::env::var("HOTSHEET_ACTOR_ID").ok()),
        display_name: actor_name,
        role,
    });
    let attachment_ids = attachment_ids
        .iter()
        .map(|id| Ulid::from_string(id).with_context(|| format!("invalid attachment id {id}")))
        .collect::<Result<Vec<_>>>()?;
    for id in &attachment_ids {
        if !ticket
            .attachments
            .iter()
            .any(|attachment| &attachment.id == id)
        {
            bail!("attachment {id} does not belong to {}", ticket.slug);
        }
    }
    // Preserve grouping and purpose while correcting provenance. The store method applies
    // one metadata value set, so update attachments individually when their metadata differs.
    for id in &attachment_ids {
        let attachment = ticket
            .attachments
            .iter()
            .find(|attachment| &attachment.id == id)
            .unwrap();
        store.set_attachment_metadata(
            &ticket.id,
            &[*id],
            hotsheet_model::AttachmentMetadata {
                batch_id: attachment.batch_id.clone(),
                batch_label: attachment.batch_label.clone(),
                actor: actor.clone(),
                purpose: attachment.purpose,
            },
            now_ts(),
        )?;
        println!("Updated attachment {id} actor to {actor_role}");
    }
    Ok(())
}

pub(crate) fn cmd_edit(
    actor: Option<&hotsheet_ticketing::actor::MutationActor>,
    path: &PathBuf,
    id: &str,
    edit: TicketEdit,
    quiet: bool,
) -> Result<()> {
    let TicketEdit {
        title,
        details,
        category,
        priority,
        status,
        started_phase,
        clear_started_phase,
        tags,
        blocked_by,
        clear_blocked_by,
        blocked_reason,
        clear_blocked_reason,
        up_next,
        no_up_next,
        note,
        note_kind,
        note_summary,
        note_confidence,
        edit_note,
    } = edit;
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let edit_note = edit_note
        .map(|note_id| {
            Ulid::from_string(&note_id)
                .map_err(|_| anyhow::anyhow!("invalid note ULID '{note_id}'"))
        })
        .transpose()?;
    if let Some(note_id) = edit_note
        && !ticket.notes.iter().any(|note| note.id == note_id)
    {
        bail!("note '{note_id}' was not found on ticket '{}'", ticket.slug);
    }
    let up_next = if up_next {
        Some(true)
    } else if no_up_next {
        Some(false)
    } else {
        None
    };
    // Present (non-empty) --blocked-by replaces the set; --clear-blocked-by empties it;
    // neither leaves it unchanged.
    let blocked_by = if clear_blocked_by {
        Some(Vec::new())
    } else if blocked_by.is_empty() {
        None
    } else {
        Some(ops::resolve_blockers(
            &store,
            Some(&ticket.id),
            &blocked_by,
        )?)
    };
    let patch = TicketPatch {
        actor: hotsheet_ticketing::actor::note_actor(actor),
        title,
        details,
        category,
        priority: priority.as_deref().map(parse_priority).transpose()?,
        status: status.as_deref().map(parse_status_str).transpose()?,
        started_phase: if clear_started_phase {
            Some(None)
        } else {
            started_phase
                .as_deref()
                .map(parse_started_phase)
                .transpose()?
                .map(Some)
        },
        tags: (!tags.is_empty()).then_some(tags),
        up_next,
        blocked_by,
        blocked_reason: if clear_blocked_reason {
            Some(None)
        } else {
            blocked_reason.map(Some)
        },
    };
    // Role-specific rules run before any write (HS2-RD4M29).
    let note = note.filter(|t| !t.is_empty());
    let scores_now = match &edit_note {
        Some(_) => matches!(note_confidence, Some(Some(_))),
        None => note.is_some() && note_confidence.flatten().is_some(),
    };
    hotsheet_ticketing::actor::check_completion(
        actor,
        &ticket.slug,
        hotsheet_ticketing::actor::completes(ticket.status, patch.status),
        scores_now || hotsheet_ticketing::actor::scored_in_current_cycle(&ticket),
    )?;
    let mut updated = ops::update(&store, &ticket.id, now_ts(), patch)?;
    let warnings = note
        .as_deref()
        .map(|text| ops::attachment_reference_warnings(&store, &updated, text))
        .unwrap_or_default();
    if let Some(note_id) = edit_note {
        let edit = ops::NoteEditInput {
            text: note,
            confidence: note_confidence,
            actor: hotsheet_ticketing::actor::note_actor(actor),
        };
        if !edit.is_empty() {
            updated = ops::edit_note_with_metadata(&store, &ticket.id, &note_id, now_ts(), edit)?;
        }
    } else if let Some(text) = note {
        updated = ops::add_note_with_metadata(
            &store,
            &ticket.id,
            Ulid::new(),
            now_ts(),
            note_kind,
            ops::NoteMetadataInput {
                human_edited: false,
                summary: note_summary,
                confidence: note_confidence.flatten(),
                actor: hotsheet_ticketing::actor::note_actor(actor),
                ai_feedback: None,
            },
            text,
        )?;
    }
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
    if !quiet {
        println!("Updated {}", updated.slug);
    }
    Ok(())
}

pub(crate) fn cmd_batch(
    actor: Option<&hotsheet_ticketing::actor::MutationActor>,
    path: &PathBuf,
    ids: &[String],
    update: TicketEditArgs,
) -> Result<()> {
    let edit = TicketEdit::prepare(update)?;
    let mut updated = Vec::new();
    let mut errors = Vec::new();
    for id in ids {
        match cmd_edit(actor, path, id, edit.clone(), true) {
            Ok(()) => updated.push(resolve(&FsStore::open(path)?, id)?.slug),
            Err(error) => errors.push(serde_json::json!({
                "id": id,
                "message": error.to_string(),
            })),
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "updated": updated,
            "errors": errors,
        }))?
    );
    Ok(())
}

pub(crate) fn cmd_delete_note(path: &Path, id: &str, note_id: &str) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let note_id =
        Ulid::from_string(note_id).map_err(|_| anyhow::anyhow!("invalid note ULID '{note_id}'"))?;
    ops::delete_note(&store, &ticket.id, &note_id, now_ts())?;
    println!("Deleted note {note_id} from {}", ticket.slug);
    Ok(())
}

pub(crate) fn cmd_confidence_report(path: &Path, json: bool) -> Result<()> {
    let store = FsStore::open(path)?;
    let tickets = store.list_tickets_resilient()?.tickets;
    let report = hotsheet_ticketing::calibration::calibration(&tickets);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    println!(
        "Completion confidence calibration: {} completions, {} scored",
        report.completions, report.scored
    );
    println!(
        "{:<11} {:>6} {:>11} {:>8} {:>8} {:>7} {:>11} {:>9}",
        "band", "range", "completions", "verified", "reopened", "pending", "reopen-rate", "mean"
    );
    for band in &report.bands {
        let rate = band
            .reopen_rate
            .map(|rate| format!("{:.0}%", rate * 100.0))
            .unwrap_or_else(|| "-".into());
        let mean = band
            .mean_confidence
            .map(|mean| format!("{mean:.1}"))
            .unwrap_or_else(|| "-".into());
        println!(
            "{:<11} {:>6} {:>11} {:>8} {:>8} {:>7} {:>11} {:>9}",
            band.band,
            band.range,
            band.completions,
            band.verified,
            band.reopened,
            band.pending,
            rate,
            mean
        );
    }
    println!(
        "Reopen rate counts resolved completions only (verified or reopened). A well-calibrated \
         rubric reopens less as the band rises."
    );
    Ok(())
}

pub(crate) fn cmd_close(
    actor: Option<&hotsheet_ticketing::actor::MutationActor>,
    path: &PathBuf,
    id: &str,
    reason: &str,
    duplicate_of: Option<String>,
) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let reason_enum = parse_close_reason(reason)?;
    hotsheet_ticketing::actor::check_completion(
        actor,
        &ticket.slug,
        hotsheet_ticketing::actor::close_completes(ticket.status, reason_enum),
        hotsheet_ticketing::actor::scored_in_current_cycle(&ticket),
    )?;
    let dup = match duplicate_of {
        Some(d) => Some(resolve(&store, &d)?.id.to_string()),
        None => None,
    };
    let closed = ops::close_as(
        &store,
        &ticket.id,
        now_ts(),
        reason_enum,
        dup,
        hotsheet_ticketing::actor::note_actor(actor).as_ref(),
    )?;
    println!("Closed {} ({reason})", closed.slug);
    Ok(())
}

pub(crate) fn cmd_restore(path: &PathBuf, id: &str) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let restored = ops::restore(&store, &ticket.id, now_ts())?;
    println!(
        "Restored {} to {}",
        restored.slug,
        format!("{:?}", restored.status).to_lowercase()
    );
    Ok(())
}
