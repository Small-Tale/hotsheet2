//! External ticket-provider connections, provider-native tickets, keys, and accounts.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use hotsheet_model::{Confidence, NoteKind, ReviewRequest, Status, Ulid};
use hotsheet_ticketing::{
    FsStore, GitProvider, KeyRegistry, MutationContext, OsKeychain, ProviderConfigRegistry,
    ProviderDraft, ProviderPatch, ProviderRegistry, TicketProvider, TicketQuery, TicketRef,
    copy_between, git_connection_id, move_between, ops,
};
use time::OffsetDateTime;

use crate::{
    Cmd, confidence_change, feedback_synthesis, now_ts, parse_close_reason, parse_note_kind,
    parse_priority, parse_review_kind, parse_status_str, read_note_input, validate_note_modifiers,
};

use super::{Ctx, misrouted};

pub(super) fn run(cmd: Cmd, ctx: &Ctx) -> Result<()> {
    let path = &ctx.path;
    let actor = ctx.actor.as_ref();
    match cmd {
        Cmd::Providers { json } => cmd_providers(path, json),
        Cmd::GithubSignIn { web_base } => cmd_github_sign_in(&web_base),
        Cmd::GithubConnect(args) => cmd_github_connect(path, args),
        Cmd::ProviderConnect(args) => cmd_provider_connect(path, args),
        Cmd::ProviderAttach {
            connection,
            id,
            files,
        } => cmd_provider_attach(path, &connection, &id, &files),
        Cmd::ProviderLs { connection } => cmd_provider_ls(path, &connection),
        Cmd::ProviderGet { connection, id } => cmd_provider_get(path, &connection, &id),
        Cmd::FeedbackSynthesis { command } => match command {
            FeedbackSynthesisCmd::Prepare { connections } => {
                cmd_feedback_synthesis_prepare(path, &connections)
            }
            FeedbackSynthesisCmd::Accept { reviewed } => {
                accept_feedback_synthesis(path, actor, reviewed)
            }
        },
        Cmd::ProviderDisable { connection } => cmd_provider_set_disabled(path, &connection, true),
        Cmd::ProviderEnable { connection } => cmd_provider_set_disabled(path, &connection, false),
        Cmd::ProviderRemove { connection, json } => cmd_provider_remove(path, &connection, json),
        Cmd::ProviderNew {
            connection,
            title,
            category,
            priority,
            details,
            tags,
        } => cmd_provider_new(
            path,
            &connection,
            title,
            category,
            &priority,
            details.unwrap_or_default(),
            tags,
        ),
        Cmd::ProviderEdit {
            connection,
            id,
            title,
            details,
            status,
            expected_token,
            note,
            note_file,
            allow_literal_backslash_n,
            note_kind,
            note_summary,
            response_to,
            note_confidence,
            clear_note_confidence,
            edit_note,
        } => {
            let note = read_note_input(note, note_file, allow_literal_backslash_n)?;
            if response_to.is_some() && note.as_deref().is_none_or(str::is_empty) {
                anyhow::bail!("--response-to requires --note or --note-file");
            }
            let note_confidence = confidence_change(note_confidence, clear_note_confidence);
            validate_note_modifiers(
                &note,
                note_kind.as_ref(),
                note_summary.as_ref(),
                note_confidence,
                edit_note.as_ref(),
            )?;
            cmd_provider_edit(
                actor,
                path,
                &connection,
                &id,
                ProviderEditInput {
                    title,
                    details,
                    status,
                    expected_token,
                    note,
                    note_kind: parse_note_kind(note_kind.as_deref().unwrap_or("regular"))?,
                    note_summary,
                    response_to,
                    note_confidence,
                    edit_note,
                },
            )
        }
        Cmd::ProviderClose {
            connection,
            id,
            reason,
        } => cmd_provider_close(path, &connection, &id, &reason),
        Cmd::ProviderAssign {
            connection,
            id,
            to,
            clear,
            reviews,
        } => cmd_provider_assign(path, &connection, &id, to, clear, reviews),
        Cmd::ProviderRestore { connection, id } => cmd_provider_restore(path, &connection, &id),
        Cmd::ProviderReportNotWorking {
            connection,
            id,
            note,
            note_file,
            evidence,
            expected_token,
        } => cmd_provider_report_not_working(
            path,
            &connection,
            &id,
            note,
            note_file,
            &evidence,
            expected_token,
        ),
        Cmd::ProviderDeleteNote {
            connection,
            id,
            note_id,
        } => cmd_provider_delete_note(path, &connection, &id, &note_id),
        Cmd::ProviderCopy {
            id,
            to,
            operation_id,
        } => cmd_provider_transfer(path, &id, &to, &operation_id, false),
        Cmd::ProviderMove {
            id,
            to,
            operation_id,
            yes,
        } => {
            if !yes {
                bail!("provider-move requires --yes");
            }
            cmd_provider_transfer(path, &id, &to, &operation_id, true)
        }
        Cmd::Key { cmd } => cmd_key(cmd),
        Cmd::Account { cmd } => cmd_account(cmd, path),
        _ => misrouted("providers"),
    }
}

fn accept_feedback_synthesis(
    path: &Path,
    actor: Option<&hotsheet_ticketing::actor::MutationActor>,
    reviewed: bool,
) -> Result<()> {
    if !reviewed {
        bail!("review the draft, then pass --reviewed to accept its source cursor");
    }
    if actor.map(|actor| actor.role) != Some(hotsheet_model::AttachmentActorRole::Human) {
        bail!("accepting AI feedback synthesis requires --actor-role human");
    }
    let dir = feedback_synthesis::state_dir(FsStore::open(path)?.root())?;
    let cursor = feedback_synthesis::accept_review(&dir)?;
    println!("Marked feedback reviewed through {cursor}. Repository guidance was not published.");
    Ok(())
}

pub(crate) fn cmd_providers(path: &Path, json: bool) -> Result<()> {
    let store = FsStore::open(path)?;
    let connections = ProviderConfigRegistry::new(store.root().join("providers.json")).load()?;
    let external_default = connections.iter().any(|connection| connection.default);
    let disabled: Vec<String> = connections
        .iter()
        .filter(|connection| connection.disabled)
        .map(|connection| connection.id.clone())
        .collect();
    let descriptor = GitProvider::new(git_connection_id(&store), store.clone())
        .with_default(!external_default)
        .descriptor();
    let mut descriptors = vec![descriptor];
    for connection in connections {
        if connection.provider != "git" {
            descriptors.push(hotsheet_extsync::descriptor(&connection)?);
        }
    }
    descriptors.sort_by(|a, b| a.connection_id.cmp(&b.connection_id));
    if json {
        println!("{}", serde_json::to_string_pretty(&descriptors)?);
    } else {
        for descriptor in descriptors {
            println!(
                "{}  {}  {}  {}{}",
                descriptor.connection_id,
                descriptor.provider,
                if descriptor.default { "default" } else { "" },
                descriptor.locator,
                if disabled.contains(&descriptor.connection_id) {
                    "  (disabled)"
                } else {
                    ""
                }
            );
        }
    }
    Ok(())
}

pub(crate) fn cmd_github_sign_in(web_base: &str) -> Result<()> {
    const BUNDLED_ID: &str = include_str!("../../../hotsheet-server/github-app-client-id.txt");
    let client_id =
        hotsheet_extsync::github_app_config::client_id_for_web_base(BUNDLED_ID, web_base)
            .map_err(anyhow::Error::msg)?;
    let web_base = web_base.trim_end_matches('/');
    let client = hotsheet_extsync::GitHubDeviceClient::live(&client_id, web_base);
    let authorization = client.start()?;
    println!("Open: {}", authorization.verification_uri);
    println!("Enter code: {}", authorization.user_code);
    std::io::stdout().flush()?;
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(authorization.expires_in);
    let mut interval = authorization.interval.max(1);
    while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
        if remaining.is_zero() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(interval).min(remaining));
        match client.poll(&authorization.device_code)? {
            hotsheet_extsync::DevicePoll::Pending => {}
            hotsheet_extsync::DevicePoll::SlowDown => interval = interval.saturating_add(5),
            hotsheet_extsync::DevicePoll::Expired => {
                bail!("GitHub device code expired; run github-sign-in again")
            }
            hotsheet_extsync::DevicePoll::Denied => bail!("GitHub sign-in was denied"),
            hotsheet_extsync::DevicePoll::Authorized(bundle) => {
                let login = client.current_login(&bundle.access_token).ok();
                let reference = format!(
                    "github-app-{}",
                    Ulid::new().to_string().to_ascii_lowercase()
                );
                let keys = KeyRegistry::new(hotsheet_plugins::hotsheet_home(), OsKeychain);
                hotsheet_extsync::store_device_authorization(
                    &keys,
                    &reference,
                    &client_id,
                    web_base,
                    &bundle,
                    OffsetDateTime::now_utc().unix_timestamp(),
                )?;
                if let Some(login) = login {
                    let _ = keys.record_identity(&reference, &login);
                }
                println!("Credential reference: {reference}");
                return Ok(());
            }
        }
    }
    bail!("GitHub device code expired; run github-sign-in again")
}

pub(crate) fn cmd_github_connect(path: &Path, args: GithubConnectArgs) -> Result<()> {
    let GithubConnectArgs {
        locator,
        credential,
        name,
        id,
        default: make_default,
        checkout,
        attachment_repo,
        attachment_folder,
        attachment_branch,
        no_attachments,
    } = args;
    let (locator, credential) = (locator.as_str(), credential.as_str());
    let attachments = GitHubAttachmentChange::from_flags(
        attachment_repo,
        attachment_folder,
        attachment_branch,
        no_attachments,
    );
    let store = FsStore::open(path)?;
    let home = hotsheet_plugins::hotsheet_home();
    let keys = KeyRegistry::new(&home, OsKeychain);
    let raw = keys
        .get(credential)
        .with_context(|| format!("credential reference '{credential}' was not found"))?;
    let checkout_registry =
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json"));
    if let Some(reference) = &checkout {
        checkout_registry.resolve(reference)?;
    }
    let registry = ProviderConfigRegistry::new(store.root().join("providers.json"));
    let connections = registry.load()?;
    let existing = match id.as_deref() {
        Some(id) => connections.iter().find(|item| item.id == id),
        None => connections
            .iter()
            .find(|item| item.provider == "github" && item.locator.eq_ignore_ascii_case(locator)),
    };
    if existing.is_some_and(|item| item.provider != "github") {
        bail!("connection id is already used by another provider");
    }
    if existing.is_some_and(|item| !item.locator.eq_ignore_ascii_case(locator)) {
        bail!(
            "an existing connection id cannot be retargeted to another repository; create a new connection"
        );
    }
    let connection_id = id.unwrap_or_else(|| {
        existing.map(|item| item.id.clone()).unwrap_or_else(|| {
            hotsheet_ticketing::generate_connection_id(&connections, "github", locator)
        })
    });
    let web_base = serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .filter(|value| value.get("kind").and_then(serde_json::Value::as_str) == Some("github_app"))
        .and_then(|value| {
            value
                .get("web_base")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
    let api_base = web_base
        .as_deref()
        .filter(|base| *base != "https://github.com")
        .map(|base| format!("{base}/api/v3"))
        .or_else(|| {
            existing.and_then(|item| {
                item.settings
                    .get("api_base")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
        });
    let mut settings = serde_json::json!({"credential":{"secret":credential}});
    if let Some(api_base) = api_base {
        settings["api_base"] = api_base.into();
    }
    // A reconnect keeps the existing assets repository unless the flags change it.
    let attachment_repository = match attachments {
        GitHubAttachmentChange::Set {
            repository,
            folder,
            branch,
        } => Some(
            hotsheet_extsync::GitHubAttachmentRepository::new(
                &repository,
                folder.as_deref(),
                branch.as_deref(),
            )
            .map_err(anyhow::Error::msg)?,
        ),
        GitHubAttachmentChange::Remove => None,
        GitHubAttachmentChange::Keep => existing
            .map(|item| hotsheet_extsync::GitHubAttachmentRepository::from_settings(&item.settings))
            .transpose()
            .map_err(anyhow::Error::msg)?
            .flatten(),
    };
    if let Some(repository) = &attachment_repository {
        repository.write_settings(&mut settings);
    }
    let connection = hotsheet_ticketing::ProviderConnection {
        id: connection_id.clone(),
        provider: "github".into(),
        locator: locator.into(),
        name: Some(name.unwrap_or_else(|| {
            existing
                .and_then(|item| item.name.clone())
                .unwrap_or_else(|| "GitHub Issues".into())
        })),
        default: make_default || existing.is_some_and(|item| item.default),
        settings,
        disabled: existing.is_some_and(|item| item.disabled),
    };
    let replacing = existing.map(|item| item.id.clone());
    let updated = hotsheet_extsync::updated_connections(
        connections,
        connection.clone(),
        replacing.as_deref(),
    )?;
    registry.save(&updated)?;
    if let Some(reference) = &checkout {
        checkout_registry.add_registered_source(
            &registry,
            reference,
            hotsheet_ticketing::checkouts::TicketSource {
                connection_id: connection_id.clone(),
                provider: "github".into(),
                locator: locator.into(),
            },
            make_default,
        )?;
    }
    println!("Connected GitHub Issues as '{connection_id}'.");
    match attachment_repository {
        Some(repository) => println!(
            "Attachments: {} ({}/ on {})",
            repository.repository,
            if repository.folder.is_empty() {
                "."
            } else {
                repository.folder.as_str()
            },
            repository.branch
        ),
        None => println!("Attachments: off (configure with --attachment-repo OWNER/REPO)"),
    }
    Ok(())
}

pub(crate) fn cmd_provider_attach(
    path: &Path,
    connection: &str,
    id: &str,
    files: &[PathBuf],
) -> Result<()> {
    let provider = configured_provider(path, connection)?;
    if !provider.descriptor().capabilities.attachments {
        bail!(
            "provider connection '{connection}' does not support attachments{}",
            if provider.descriptor().provider == "github" {
                "; configure an assets repository with `github-connect --attachment-repo OWNER/REPO`"
            } else {
                ""
            }
        );
    }
    let batch_id = hotsheet_model::Ulid::new().to_string();
    let mut ticket = None;
    for file in files {
        let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
        let filename = file
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow::anyhow!("{} has no UTF-8 file name", file.display()))?
            .to_string();
        ticket = Some(
            provider.add_attachment(
                id,
                hotsheet_ticketing::wire::ApiAttachment {
                    id: hotsheet_model::Ulid::new().to_string(),
                    filename,
                    created_at: OffsetDateTime::now_utc()
                        .format(&time::format_description::well_known::Rfc3339)?,
                    batch_id: (files.len() > 1).then(|| batch_id.clone()),
                    batch_label: None,
                    actor: None,
                    purpose: None,
                    annotations: vec![],
                    crop: None,
                    revision: None,
                },
                bytes,
            )?,
        );
    }
    println!("{}", serde_json::to_string_pretty(&ticket)?);
    Ok(())
}

pub(crate) fn cmd_provider_ls(path: &Path, connection: &str) -> Result<()> {
    let tickets = configured_provider(path, connection)?.query(&TicketQuery::default())?;
    println!("{}", serde_json::to_string_pretty(&tickets)?);
    Ok(())
}

pub(crate) fn cmd_provider_get(path: &Path, connection: &str, id: &str) -> Result<()> {
    let ticket = configured_provider(path, connection)?.get(id)?;
    println!("{}", serde_json::to_string_pretty(&ticket)?);
    Ok(())
}

pub(crate) fn cmd_feedback_synthesis_prepare(path: &Path, connections: &[String]) -> Result<()> {
    let store = FsStore::open(path)?;
    let git_id = git_connection_id(&store);
    let configs = ProviderConfigRegistry::new(store.root().join("providers.json")).load()?;
    let mut selected = connections.to_vec();
    selected.sort();
    selected.dedup();
    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for connection in &selected {
        let result = (|| -> Result<Vec<hotsheet_ticketing::AiFeedbackRecord>> {
            if connection != &git_id {
                let config = configs
                    .iter()
                    .find(|item| item.id == *connection)
                    .ok_or_else(|| {
                        anyhow::anyhow!("provider connection '{connection}' was not found")
                    })?;
                if config.disabled {
                    bail!("provider connection '{connection}' is disabled");
                }
                if !hotsheet_extsync::descriptor(config)?
                    .capabilities
                    .ai_feedback
                {
                    bail!(
                        "provider connection '{connection}' does not support AI feedback retrieval"
                    );
                }
            }
            Ok(configured_provider(store.root(), connection)?.ai_feedback_records()?)
        })();
        match result {
            Ok(mut records) => rows.append(&mut records),
            Err(error) => errors.push((connection.clone(), error.to_string())),
        }
    }
    if errors.len() == selected.len() {
        for (connection, error) in &errors {
            eprintln!("{connection}: {error}");
        }
        bail!("no selected provider could supply AI feedback");
    }
    let dir = feedback_synthesis::state_dir(store.root())?;
    match feedback_synthesis::prepare(&dir, &selected, rows, &errors)? {
        Some(draft) => println!("Review draft: {}", draft.display()),
        None if errors.is_empty() => println!("No new AI feedback for the selected providers."),
        None => println!(
            "No new readable AI feedback; {} provider(s) unavailable.",
            errors.len()
        ),
    }
    for (connection, error) in errors {
        eprintln!("{connection}: {error}");
    }
    Ok(())
}

pub(crate) fn cmd_provider_set_disabled(
    path: &Path,
    connection: &str,
    disabled: bool,
) -> Result<()> {
    let store = FsStore::open(path)?;
    ProviderConfigRegistry::new(store.root().join("providers.json"))
        .set_disabled(connection, disabled)?
        .ok_or_else(|| anyhow::anyhow!("provider connection '{connection}' was not found"))?;
    println!(
        "Provider connection '{connection}' is {}.",
        if disabled { "disabled" } else { "enabled" }
    );
    Ok(())
}

pub(crate) fn cmd_provider_remove(path: &Path, connection: &str, json: bool) -> Result<()> {
    let store = FsStore::open(path)?;
    if connection == git_connection_id(&store) {
        anyhow::bail!(
            "'{connection}' is this store's git ticket source, not an external provider connection"
        );
    }
    let home = hotsheet_plugins::hotsheet_home();
    let report = hotsheet_ticketing::connection_removal::remove_provider_connection(
        &ProviderConfigRegistry::new(store.root().join("providers.json")),
        &hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json")),
        connection,
    )?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    if report.removed_connection {
        println!("Removed provider connection '{connection}'.");
    } else {
        println!("Provider connection '{connection}' was already removed.");
    }
    for checkout in &report.unlinked_checkouts {
        println!("Unlinked it from checkout {checkout}.");
    }
    if let Some(credential) = &report.kept_credential {
        println!(
            "Account '{credential}' stays signed in; sign out with `hotsheet account sign-out {credential}`."
        );
    }
    Ok(())
}

pub(crate) fn cmd_provider_new(
    path: &Path,
    connection: &str,
    title: String,
    category: String,
    priority: &str,
    details: String,
    tags: Vec<String>,
) -> Result<()> {
    let priority = parse_priority(priority)?;
    let ticket = configured_provider(path, connection)?.create(
        MutationContext {
            now: now_ts(),
            generated_id: Ulid::new(),
        },
        ProviderDraft {
            title,
            category,
            priority,
            status: Status::NotStarted,
            details,
            tags,
            up_next: false,
            blocked_by: vec![],
            transfer: None,
        },
    )?;
    println!("{}", serde_json::to_string_pretty(&ticket)?);
    Ok(())
}

pub(crate) fn cmd_provider_edit(
    actor: Option<&hotsheet_ticketing::actor::MutationActor>,
    path: &Path,
    connection: &str,
    id: &str,
    input: ProviderEditInput,
) -> Result<()> {
    let provider = configured_provider(path, connection)?;
    if input.edit_note.is_some() && !provider.supports_note_edit() {
        bail!("provider connection '{connection}' does not support note editing");
    }
    let status = input.status.as_deref().map(parse_status_str).transpose()?;
    if actor.is_some_and(hotsheet_ticketing::actor::MutationActor::is_ai)
        && status == Some(hotsheet_model::Status::Completed)
    {
        let current = provider.get(id)?;
        let scores_now = match &input.edit_note {
            Some(_) => matches!(input.note_confidence, Some(Some(_))),
            None => input.note.is_some() && input.note_confidence.flatten().is_some(),
        };
        hotsheet_ticketing::actor::check_completion(
            actor,
            &current.slug,
            hotsheet_ticketing::actor::completes(current.status, status),
            scores_now || hotsheet_ticketing::actor::api_scored_in_current_cycle(&current),
        )?;
    }
    let now = now_ts();
    let mut ticket = provider.update(
        id,
        now.clone(),
        ProviderPatch {
            expected_token: input.expected_token,
            title: input.title,
            details: input.details,
            status,
            actor: hotsheet_ticketing::actor::note_actor(actor),
            ..Default::default()
        },
    )?;
    if let Some(note_id) = input.edit_note {
        let edit = ops::NoteEditInput {
            text: input.note,
            confidence: input.note_confidence,
            actor: hotsheet_ticketing::actor::note_actor(actor),
            allow_feedback_prompt_edit: false,
        };
        if !edit.is_empty() {
            ticket = provider.edit_note_with_metadata(id, &note_id, now, edit)?;
        }
    } else if let Some(note) = input.note {
        ticket = provider.add_note_with_metadata(
            id,
            MutationContext {
                now,
                generated_id: Ulid::new(),
            },
            input.note_kind,
            ops::NoteMetadataInput {
                human_edited: false,
                summary: input.note_summary,
                confidence: input.note_confidence.flatten(),
                actor: hotsheet_ticketing::actor::note_actor(actor),
                ai_feedback: None,
                response_to: input.response_to,
            },
            note,
        )?;
    }
    println!("{}", serde_json::to_string_pretty(&ticket)?);
    Ok(())
}

pub(crate) fn cmd_provider_close(
    path: &Path,
    connection: &str,
    id: &str,
    reason: &str,
) -> Result<()> {
    let reason = parse_close_reason(reason)?;
    let ticket = configured_provider(path, connection)?.close(id, now_ts(), reason, None)?;
    println!("{}", serde_json::to_string_pretty(&ticket)?);
    Ok(())
}

pub(crate) fn cmd_provider_assign(
    path: &Path,
    connection: &str,
    id: &str,
    to: Vec<String>,
    clear: bool,
    review: Vec<String>,
) -> Result<()> {
    let provider = configured_provider(path, connection)?;
    let capabilities = provider.descriptor().capabilities;
    if !capabilities.assignment {
        bail!("provider connection '{connection}' does not support assignment");
    }
    if !review.is_empty() && !capabilities.review_requests {
        bail!("provider connection '{connection}' does not support review requests");
    }
    if !clear && to.is_empty() && review.is_empty() {
        bail!("provide --to, --clear, or --review");
    }
    let assignees = if clear {
        Some(Vec::new())
    } else if to.is_empty() {
        None
    } else {
        Some(to)
    };
    let at = now_ts();
    let reviews = review
        .iter()
        .map(|spec| {
            let (who, kind) = spec
                .split_once(':')
                .with_context(|| format!("--review expects email:kind, got '{spec}'"))?;
            Ok(ReviewRequest {
                who: who.to_owned(),
                kind: parse_review_kind(kind)?,
                by: Ulid::new(),
                at: at.clone(),
                requested_by: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let ticket = provider.assign(id, at, assignees, reviews)?;
    println!("{}", serde_json::to_string_pretty(&ticket)?);
    Ok(())
}

pub(crate) fn cmd_provider_restore(path: &Path, connection: &str, id: &str) -> Result<()> {
    let store = FsStore::open(path)?;
    let git_id = git_connection_id(&store);
    if connection != git_id {
        let _ = configured_provider(path, connection)?;
        bail!(
            "provider connection '{connection}' does not support git-backed Hot Sheet Trash restore"
        );
    }
    let ticket =
        ops::resolve(&store, id)?.ok_or_else(|| anyhow::anyhow!("no ticket matching '{id}'"))?;
    ops::restore(&store, &ticket.id, now_ts())?;
    let restored = GitProvider::new(git_id, store).get(id)?;
    println!("{}", serde_json::to_string_pretty(&restored)?);
    Ok(())
}

pub(crate) fn cmd_provider_report_not_working(
    path: &Path,
    connection: &str,
    id: &str,
    note: Option<String>,
    note_file: Option<PathBuf>,
    files: &[PathBuf],
    expected_token: Option<String>,
) -> Result<()> {
    let provider = configured_provider(path, connection)?;
    if !provider.descriptor().capabilities.not_working_report {
        bail!("provider connection '{connection}' does not support an atomic Not Working report");
    }
    let note = read_note_input(note, note_file, false)?
        .and_then(|text| (!text.trim().is_empty()).then(|| (Ulid::new(), text)));
    if note.is_none() && files.is_empty() {
        bail!("a Not Working report requires a note or at least one evidence attachment");
    }
    let at = now_ts();
    let evidence = files
        .iter()
        .map(|file| {
            let filename = file
                .file_name()
                .and_then(|name| name.to_str())
                .with_context(|| format!("invalid evidence filename {}", file.display()))?;
            Ok(hotsheet_ticketing::ProviderEvidence {
                id: Ulid::new(),
                filename: filename.to_owned(),
                created_at: at.clone(),
                bytes: std::fs::read(file)
                    .with_context(|| format!("reading evidence {}", file.display()))?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let ticket = provider.report_not_working(
        id,
        at,
        hotsheet_ticketing::NotWorkingReport {
            expected_token,
            note,
            evidence,
        },
    )?;
    println!("{}", serde_json::to_string_pretty(&ticket)?);
    Ok(())
}

pub(crate) fn cmd_provider_transfer(
    src_path: &Path,
    id: &str,
    to: &Path,
    operation_id: &str,
    move_source: bool,
) -> Result<()> {
    let source_store = FsStore::open(src_path)?;
    let destination_store =
        FsStore::open(to).with_context(|| format!("opening destination store {}", to.display()))?;
    let source_id = git_connection_id(&source_store);
    let destination_id = git_connection_id(&destination_store);
    let source_ticket = ops::resolve(&source_store, id)?
        .ok_or_else(|| anyhow::anyhow!("no ticket matching '{id}'"))?;
    let registry = ProviderRegistry::default();
    registry.register(Arc::new(GitProvider::new(source_id.clone(), source_store)))?;
    registry.register(Arc::new(GitProvider::new(
        destination_id.clone(),
        destination_store,
    )))?;
    let source = TicketRef {
        connection_id: source_id,
        native_id: source_ticket.id.to_string(),
    };
    let outcome = if move_source {
        move_between(&registry, source, &destination_id, operation_id, now_ts())?
    } else {
        copy_between(&registry, source, &destination_id, operation_id, now_ts())?
    };
    println!(
        "{} {} -> {} (operation {})",
        if move_source { "Moved" } else { "Copied" },
        outcome.source.qualified(),
        outcome.destination.qualified(),
        outcome.operation_id
    );
    Ok(())
}

pub(crate) fn cmd_provider_delete_note(
    path: &Path,
    connection: &str,
    id: &str,
    note_id: &str,
) -> Result<()> {
    let provider = configured_provider(path, connection)?;
    if !provider.supports_note_delete() {
        bail!("provider connection '{connection}' does not support note deletion");
    }
    let ticket = provider.delete_note(id, note_id, now_ts())?;
    println!("{}", serde_json::to_string_pretty(&ticket)?);
    Ok(())
}

/// `hotsheet account …` (HS2-SM9PM8): the same account listing and sign-out as App
/// Settings → Accounts, over the -C store's providers.json and the machine checkout registry.
pub(crate) fn cmd_account(cmd: AccountCmd, path: &Path) -> Result<()> {
    use hotsheet_ticketing::accounts;
    let home = hotsheet_plugins::hotsheet_home();
    let keys = KeyRegistry::new(&home, OsKeychain);
    let connections =
        ProviderConfigRegistry::new(FsStore::open(path)?.root().join("providers.json")).load()?;
    let checkouts =
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json")).list()?;
    match cmd {
        AccountCmd::List { json } => {
            // Records the site of sign-ins stored before keys.json kept one (HS2-16MYXN).
            let credentials = hotsheet_extsync::credentials_with_sites(&keys)?;
            let list = accounts::list_accounts(&connections, &checkouts, &credentials);
            if json {
                println!("{}", serde_json::to_string_pretty(&list)?);
                return Ok(());
            }
            if list.is_empty() {
                println!("(no accounts)");
            }
            for account in list {
                let host = if account.host.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", account.host)
                };
                println!("{} ({}{host})", account.id, account.provider);
                // What a new source reusing this account needs besides its own locator.
                if let Some(identity) = &account.identity {
                    println!("  account: {identity}");
                }
                if let Some(base_url) = &account.base_url {
                    println!("  endpoint: {base_url}");
                }
                if account.sources.is_empty() {
                    println!("  not used by any ticket source");
                }
                for source in account.sources {
                    let projects = source
                        .projects
                        .iter()
                        .map(|project| project.alias.as_str())
                        .collect::<Vec<_>>();
                    println!(
                        "  {} [{}] {} — {}",
                        source.name,
                        source.connection_id,
                        source.locator,
                        if projects.is_empty() {
                            "used by no project".to_owned()
                        } else {
                            format!("used by {}", projects.join(", "))
                        }
                    );
                }
            }
        }
        AccountCmd::BackfillLogins { limit, after } => {
            if !(1..=10).contains(&limit) {
                bail!("--limit must be between 1 and 10");
            }
            let credentials = hotsheet_extsync::credentials_with_sites(&keys)?;
            let missing = accounts::list_accounts(&connections, &checkouts, &credentials)
                .into_iter()
                .filter(|account| {
                    account.provider == "github" && account.managed && account.identity.is_none()
                })
                .filter(|account| {
                    after
                        .as_ref()
                        .is_none_or(|after| account.id.as_str() > after.as_str())
                })
                .take(limit)
                .collect::<Vec<_>>();
            if missing.is_empty() {
                println!("No managed GitHub logins need backfilling.");
            }
            let mut failed = 0;
            for account in &missing {
                let result = (|| -> Result<String> {
                    let raw = keys.get(&account.id)?;
                    let stored: serde_json::Value = serde_json::from_str(&raw)
                        .context("managed GitHub credential is not a token bundle")?;
                    if stored.get("kind").and_then(serde_json::Value::as_str) != Some("github_app")
                    {
                        bail!("credential is not a managed GitHub App sign-in");
                    }
                    let client_id = stored
                        .get("client_id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    let web_base = stored
                        .get("web_base")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("https://github.com");
                    let token = hotsheet_extsync::access_token_from_raw(
                        &raw,
                        &keys,
                        &account.id,
                        OffsetDateTime::now_utc().unix_timestamp(),
                    )?;
                    let login = hotsheet_extsync::GitHubDeviceClient::live(client_id, web_base)
                        .current_login(&token)?;
                    keys.record_identity(&account.id, &login)?;
                    Ok(login)
                })();
                match result {
                    Ok(login) => println!("{}: {login}", account.id),
                    Err(error) => {
                        failed += 1;
                        eprintln!("{}: {error}", account.id);
                    }
                }
            }
            if failed > 0 {
                bail!("{failed} of {} login backfills failed", missing.len());
            }
        }
        AccountCmd::SignOut { account } => {
            accounts::sign_out(&connections, &checkouts, &keys, &account)?;
            println!("Signed out of {account}.");
        }
    }
    Ok(())
}

pub(crate) fn cmd_key(cmd: KeyCmd) -> Result<()> {
    use std::io::IsTerminal;

    use hotsheet_ticketing::{KeyRegistry, OsKeychain};

    let registry = KeyRegistry::new(hotsheet_plugins::hotsheet_home(), OsKeychain);
    match cmd {
        KeyCmd::Set { provider } => {
            let stdin = std::io::stdin();
            let secret = read_key_secret(&provider, stdin.is_terminal(), stdin.lock(), |prompt| {
                rpassword::prompt_password(prompt).map_err(Into::into)
            })?;
            registry.set(&provider, &secret)?;
            println!("Stored encrypted key for {provider} in the machine secret store");
        }
        KeyCmd::Get { provider } => println!("{}", registry.get(&provider)?),
        KeyCmd::List => {
            let keys = registry.list()?;
            if keys.is_empty() {
                println!("(no registered keys)");
            }
            for key in keys {
                println!("{} (fallback: {})", key.provider, key.env);
            }
        }
        KeyCmd::Delete { provider } => {
            if registry.delete(&provider)? {
                println!("Deleted key for {provider}");
            } else {
                println!("No registered key for {provider}");
            }
        }
        KeyCmd::MasterExport { output } => {
            use std::io::Write;
            let encoded = OsKeychain.export_machine_master()?;
            let parent = output
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."));
            let mut staged = tempfile::NamedTempFile::new_in(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                staged
                    .as_file()
                    .set_permissions(std::fs::Permissions::from_mode(0o600))?;
            }
            writeln!(staged, "{encoded}")?;
            staged.as_file().sync_all()?;
            staged
                .persist_noclobber(&output)
                .map_err(|error| error.error)?;
            println!(
                "Wrote an owner-only machine master backup. Keep this file secure; it opens every local encrypted secret."
            );
        }
        KeyCmd::MasterRestore { input } => {
            let encoded = std::fs::read_to_string(input)?;
            OsKeychain.restore_machine_master(&encoded)?;
            println!("Restored the machine master after verifying an encrypted secret.");
        }
        KeyCmd::MasterRotate => {
            OsKeychain.rotate_machine_master()?;
            println!("Rotated and verified the machine master.");
        }
        KeyCmd::MasterResume => {
            if OsKeychain.resume_machine_master_rotation()? {
                println!("Completed the interrupted machine master rotation.");
            } else {
                println!("No machine master rotation is pending.");
            }
        }
        KeyCmd::MasterCleanupLegacy { backup } => {
            let encoded = std::fs::read_to_string(backup)?;
            let count = OsKeychain.cleanup_legacy_machine_items(&encoded)?;
            println!("Removed {count} migrated legacy credential-store items.");
        }
    }
    Ok(())
}

#[derive(Args)]
pub(crate) struct GithubConnectArgs {
    /// Owner/repository reachable by the Hot Sheet GitHub App.
    locator: String,
    /// Credential reference printed by `github-sign-in` or created with `key set`.
    #[arg(long)]
    credential: String,
    /// Display name (defaults to GitHub Issues).
    #[arg(long)]
    name: Option<String>,
    /// Explicit connection id; otherwise use the existing locator or generate an id.
    #[arg(long)]
    id: Option<String>,
    /// Make this connection the default for its store and, with --checkout, that checkout.
    #[arg(long)]
    default: bool,
    /// Also link the connection to this registered checkout (id, alias, or path).
    #[arg(long)]
    checkout: Option<String>,
    /// Enable attachments by committing files to this `owner/repo` assets repository
    /// and linking them from issue comments (HS2-HSA64D). Kept on a later reconnect.
    #[arg(long, value_name = "OWNER/REPO", conflicts_with = "no_attachments")]
    attachment_repo: Option<String>,
    /// Folder inside the assets repository (default `hotsheet-attachments`).
    #[arg(long, requires = "attachment_repo")]
    attachment_folder: Option<String>,
    /// Branch uploads are committed to (default `main`).
    #[arg(long, requires = "attachment_repo")]
    attachment_branch: Option<String>,
    /// Remove the assets repository; the connection then reports no attachment support.
    #[arg(long)]
    no_attachments: bool,
}

/// A provider `provider-connect` creates (GitHub has `github-connect`, which also owns its
/// sign-in and assets repository).
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ConnectProvider {
    Gitlab,
    Jira,
}

impl ConnectProvider {
    fn id(self) -> &'static str {
        match self {
            Self::Gitlab => "gitlab",
            Self::Jira => "jira",
        }
    }
}

#[derive(Args)]
pub(crate) struct ProviderConnectArgs {
    /// The ticket provider.
    #[arg(value_enum)]
    provider: ConnectProvider,
    /// GitLab `namespace/project` path, or the Jira project key.
    locator: String,
    /// Credential reference holding the GitLab token or Jira API token (see `key set`).
    #[arg(long, required_unless_present = "account")]
    credential: Option<String>,
    /// Reuse an account from `account list`: fills the credential, the Jira email, and the
    /// Jira site or GitLab API base. Explicit flags still win.
    #[arg(long)]
    account: Option<String>,
    /// Self-managed GitLab API base (default `https://gitlab.com/api/v4`).
    #[arg(long, value_name = "URL")]
    api_base: Option<String>,
    /// Jira site URL, such as `https://acme.atlassian.net`.
    #[arg(long, value_name = "URL")]
    site: Option<String>,
    /// Jira account email the API token belongs to.
    #[arg(long)]
    email: Option<String>,
    /// Display name (defaults to GitLab Issues or Jira Cloud).
    #[arg(long)]
    name: Option<String>,
    /// Explicit connection id; otherwise reuse the matching source or generate an id.
    #[arg(long)]
    id: Option<String>,
    /// Make this the default source: of the checkout with --checkout, otherwise of the store.
    #[arg(long)]
    default: bool,
    /// Also link the source to this registered checkout (id, alias, or path).
    #[arg(long)]
    checkout: Option<String>,
}

/// `provider-connect`: create or refresh a GitLab or Jira ticket source headlessly through
/// the same validated workflow as `POST /checkouts/{ref}/provider-connections` (HS2-65KYVP).
pub(crate) fn cmd_provider_connect(path: &Path, args: ProviderConnectArgs) -> Result<()> {
    let ProviderConnectArgs {
        provider,
        locator,
        credential,
        account,
        api_base,
        site,
        email,
        name,
        id,
        default: make_default,
        checkout,
    } = args;
    let store = FsStore::open(path)?;
    let home = hotsheet_plugins::hotsheet_home();
    let keys = KeyRegistry::new(&home, OsKeychain);
    let registry = ProviderConfigRegistry::new(store.root().join("providers.json"));
    let checkout_registry =
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json"));
    let reused = match &account {
        Some(account) => Some(reused_account(
            &registry,
            &checkout_registry,
            &keys,
            account,
            provider,
        )?),
        None => None,
    };
    let credential = credential
        .or_else(|| reused.as_ref().map(|account| account.id.clone()))
        .context("--credential or --account is required")?;
    keys.get(&credential)
        .with_context(|| format!("credential reference '{credential}' was not found"))?;
    let reused_base = reused.as_ref().and_then(|account| account.base_url.clone());
    let mut settings = serde_json::json!({"credential":{"secret":credential}});
    match provider {
        ConnectProvider::Gitlab => {
            if site.is_some() || email.is_some() {
                bail!("--site and --email apply to Jira; GitLab uses --api-base");
            }
            if let Some(api_base) = api_base.or(reused_base) {
                settings["api_base"] = api_base.trim_end_matches('/').into();
            }
        }
        ConnectProvider::Jira => {
            if api_base.is_some() {
                bail!("--api-base applies to GitLab; Jira uses --site");
            }
            let site = site
                .or(reused_base)
                .context("Jira needs --site (or --account with a known site)")?;
            let email = email
                .or_else(|| reused.as_ref().and_then(|account| account.identity.clone()))
                .context("Jira needs --email (or --account with a known email)")?;
            settings["base_url"] = site.trim_end_matches('/').into();
            settings["email"] = email.into();
        }
    }
    if let Some(reference) = &checkout {
        checkout_registry.resolve(reference)?;
    }
    let connection = hotsheet_ticketing::ProviderConnection {
        id: id.unwrap_or_default(),
        provider: provider.id().into(),
        locator: locator.clone(),
        name,
        default: make_default,
        settings,
        disabled: false,
    };
    let connection = hotsheet_extsync::connect_source(
        &registry,
        connection,
        checkout
            .as_deref()
            .map(|reference| hotsheet_extsync::CheckoutLink {
                registry: &checkout_registry,
                reference,
                make_default,
            }),
        hotsheet_extsync::ConnectMode::Reconnect,
    )?;
    if let Some(reference) = &checkout {
        let linked = checkout_registry.resolve(reference)?;
        super::setup::retry_pending_consolidation(path, Path::new(&linked.root));
    }
    println!(
        "Connected {} '{}' as '{}'.",
        connection.name.as_deref().unwrap_or(provider.id()),
        connection.locator,
        connection.id
    );
    Ok(())
}

/// The account `provider-connect --account` reuses, which must belong to `provider`.
fn reused_account(
    registry: &ProviderConfigRegistry,
    checkouts: &hotsheet_ticketing::checkouts::CheckoutRegistry,
    keys: &KeyRegistry<OsKeychain>,
    account: &str,
    provider: ConnectProvider,
) -> Result<hotsheet_ticketing::accounts::Account> {
    // keys.json metadata only, like the client's account picker: never reads the keychain.
    let accounts = hotsheet_ticketing::accounts::list_accounts(
        &registry.load()?,
        &checkouts.list()?,
        &keys.list()?,
    );
    let found = accounts
        .into_iter()
        .find(|item| item.id == account)
        .with_context(|| format!("no account named '{account}' (see `account list`)"))?;
    if found.provider != provider.id() {
        bail!(
            "account '{account}' signs in to {}, not {}",
            found.provider,
            provider.id()
        );
    }
    Ok(found)
}

#[derive(Subcommand)]
pub(crate) enum FeedbackSynthesisCmd {
    /// Read only the explicitly selected connections and prepare a private local draft.
    Prepare {
        #[arg(long = "connection", required = true)]
        connections: Vec<String>,
    },
    /// Advance the review ledger after a human has edited or rejected the draft.
    Accept {
        #[arg(long)]
        reviewed: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum AccountCmd {
    /// List provider sign-ins with the ticket sources and projects using each.
    List {
        /// Emit the accounts as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Resolve missing GitHub usernames for a bounded batch of older managed sign-ins.
    /// This may read the OS keychain and contact GitHub; plain `account list` never does so.
    BackfillLogins {
        /// Maximum accounts to try in this run (1-10; default 5).
        #[arg(long, default_value_t = 5)]
        limit: usize,
        /// Start after this credential reference to reach later accounts when earlier ones fail.
        #[arg(long)]
        after: Option<String>,
    },
    /// Sign out: delete the account's credential. Refused while a ticket source uses it.
    SignOut { account: String },
}

#[derive(Subcommand)]
pub(crate) enum KeyCmd {
    /// Store/replace a provider key. Prompts securely on a terminal or reads piped stdin.
    Set { provider: String },
    /// Print a provider key to stdout (explicit secret-reveal operation).
    Get { provider: String },
    /// List registered provider names and their read-only environment fallbacks.
    List,
    /// Delete a provider key from encrypted machine storage and the registry.
    Delete { provider: String },
    /// Save the machine master to a new owner-only backup file.
    MasterExport {
        #[arg(long)]
        output: PathBuf,
    },
    /// Restore a backed-up master after checking it against existing encrypted data.
    MasterRestore {
        #[arg(long)]
        input: PathBuf,
    },
    /// Rotate the native master and re-encrypt all local secret envelopes.
    MasterRotate,
    /// Finish an interrupted master rotation without starting another.
    MasterResume,
    /// Remove migrated legacy OS items after verifying a backup of the current master.
    MasterCleanupLegacy {
        #[arg(long)]
        backup: PathBuf,
    },
}

/// How `github-connect` changes a connection's attachment repository (HS2-HSA64D).
pub(crate) enum GitHubAttachmentChange {
    Keep,
    Set {
        repository: String,
        folder: Option<String>,
        branch: Option<String>,
    },
    Remove,
}

impl GitHubAttachmentChange {
    fn from_flags(
        repository: Option<String>,
        folder: Option<String>,
        branch: Option<String>,
        remove: bool,
    ) -> Self {
        match (repository, remove) {
            (Some(repository), _) => Self::Set {
                repository,
                folder,
                branch,
            },
            (None, true) => Self::Remove,
            (None, false) => Self::Keep,
        }
    }
}

pub(crate) fn configured_provider(
    path: &Path,
    connection_id: &str,
) -> Result<Arc<dyn TicketProvider>> {
    let store = FsStore::open(path)?;
    let git_id = git_connection_id(&store);
    if connection_id == git_id {
        return Ok(Arc::new(GitProvider::new(git_id, store)));
    }
    let connection = ProviderConfigRegistry::new(store.root().join("providers.json"))
        .load()?
        .into_iter()
        .find(|connection| connection.id == connection_id)
        .ok_or_else(|| anyhow::anyhow!("provider connection '{connection_id}' was not found"))?;
    if connection.disabled {
        return Err(hotsheet_ticketing::ProviderError::Disabled {
            connection_id: connection_id.into(),
        }
        .into());
    }
    let token = hotsheet_extsync::connection_access_token(
        &connection,
        &KeyRegistry::new(hotsheet_plugins::hotsheet_home(), OsKeychain),
        time::OffsetDateTime::now_utc().unix_timestamp(),
    )?;
    Ok(hotsheet_extsync::live_provider(&connection, token)?)
}

pub(crate) struct ProviderEditInput {
    title: Option<String>,
    details: Option<String>,
    status: Option<String>,
    expected_token: Option<String>,
    note: Option<String>,
    note_kind: NoteKind,
    note_summary: Option<String>,
    response_to: Option<String>,
    note_confidence: Option<Option<Confidence>>,
    edit_note: Option<String>,
}

pub(crate) fn read_key_secret(
    provider: &str,
    terminal: bool,
    mut reader: impl std::io::Read,
    prompt: impl FnOnce(&str) -> Result<String>,
) -> Result<String> {
    let value = if terminal {
        prompt(&format!("Key for {provider}: "))?
    } else {
        let mut value = String::new();
        reader.read_to_string(&mut value)?;
        value
    };
    let value = value.trim_end_matches(['\r', '\n']).to_owned();
    if value.is_empty() {
        bail!("refusing to store an empty key")
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_input_prompts_on_a_terminal_and_reads_pipes_without_prompting() {
        let prompted = read_key_secret("github-live", true, "ignored".as_bytes(), |prompt| {
            assert_eq!(prompt, "Key for github-live: ");
            Ok("terminal-secret".into())
        })
        .unwrap();
        assert_eq!(prompted, "terminal-secret");

        let piped = read_key_secret("github-live", false, "pipe-secret\n".as_bytes(), |_| {
            panic!("non-terminal input must not prompt")
        })
        .unwrap();
        assert_eq!(piped, "pipe-secret");
        assert!(
            read_key_secret("github-live", false, "\n".as_bytes(), |_| unreachable!())
                .unwrap_err()
                .to_string()
                .contains("empty key")
        );
    }
}
