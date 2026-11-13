//! HS2-QR1K8R stage 2: the ask-the-AI state machine on real stalled stores with a fake
//! [`AiReach`], and `adopt_resolved_history`. Synthetic stores, files, and tools only; no
//! test launches an AI tool.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::*;
use crate::sync::tests::{clean, commit_file, head, ok, remote_with_two_clones};
use crate::sync::{AdoptError, SyncReport, adopt_resolved_history, git_ok, sync_once};
use crate::sync_health::health;

/// A scripted reach: what a running session, the default tool, and a launch do.
struct FakeReach {
    session: Option<&'static str>,
    push_error: Option<&'static str>,
    tool: Result<&'static str, &'static str>,
    start_error: Option<&'static str>,
    pushes: RefCell<Vec<String>>,
    starts: RefCell<Vec<(String, String)>>,
    done: RefCell<Vec<SessionDone>>,
}

impl Default for FakeReach {
    fn default() -> Self {
        Self {
            session: None,
            push_error: None,
            // The tool `.cargo/config.toml` allows a stall to start in tests.
            tool: Ok("repair-fixture"),
            start_error: None,
            pushes: RefCell::default(),
            starts: RefCell::default(),
            done: RefCell::default(),
        }
    }
}

impl AiReach for FakeReach {
    fn push_to_session(&self, prompt: &str) -> Result<Option<String>, String> {
        if let Some(error) = self.push_error {
            return Err(error.to_string());
        }
        if self.session.is_some() {
            self.pushes.borrow_mut().push(prompt.to_string());
        }
        Ok(self.session.map(str::to_string))
    }

    fn drivable_tool(&self) -> Result<String, String> {
        self.tool.map(str::to_string).map_err(str::to_string)
    }

    fn start_session(&self, tool: &str, prompt: &str, done: SessionDone) -> Result<String, String> {
        if let Some(error) = self.start_error {
            return Err(error.to_string());
        }
        self.starts
            .borrow_mut()
            .push((tool.to_string(), prompt.to_string()));
        // The test decides when the session ends, always after this call returns.
        self.done.borrow_mut().push(done);
        Ok(format!("sync-repair-{}", self.starts.borrow().len()))
    }
}

fn context(store: &Path, mode: AskAiMode) -> AskContext {
    AskContext {
        mode,
        label: "tickets.hs2".to_string(),
        store_reference: store_reference(store.parent().unwrap().join("code").as_path(), store),
    }
}

/// A Hot Sheet store with a remote and a peer clone, in sync.
fn store_with_peer() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let (dir, root, peer) = remote_with_two_clones();
    let store = FsStore::init(&root, &crate::StoreMetadata::new("HS"))
        .unwrap()
        .with_deferred_push();
    assert!(store.autocommit("init store").unwrap());
    assert!(matches!(sync_once(&root), SyncReport::Synced { .. }));
    ok(&peer, &["pull", "--quiet"]);
    (dir, root, peer)
}

/// Both sides change one plain file, so the merge driver cannot help: `conflict`.
fn conflict(root: &Path, peer: &Path, round: u32) {
    commit_file(root, "shared.txt", &format!("local body {round}\n"));
    std::fs::write(peer.join("shared.txt"), format!("peer body {round}\n")).unwrap();
    ok(peer, &["add", "-A"]);
    ok(peer, &["commit", "--quiet", "-m", "peer shared"]);
    ok(peer, &["push", "--quiet"]);
    assert_eq!(sync_once(root), SyncReport::Conflict);
}

/// Take the remote side of the conflicting file, keeping every other local commit, so the
/// next cycle publishes again.
fn settle(root: &Path) {
    ok(root, &["fetch", "--quiet"]);
    ok(
        root,
        &[
            "-c",
            "core.editor=true",
            "rebase",
            "--quiet",
            "-X",
            "ours",
            "@{u}",
        ],
    );
    assert!(matches!(
        sync_once(root),
        SyncReport::UpToDate | SyncReport::Synced { .. }
    ));
}

fn repair_tickets(root: &Path) -> Vec<hotsheet_model::Ticket> {
    let store = FsStore::open(root).unwrap();
    ops::query(
        &store,
        &TicketQuery {
            tags: vec![REPAIR_TAG.to_string()],
            ..TicketQuery::default()
        },
    )
    .unwrap()
}

fn asked(outcome: AskOutcome) -> AiHelpRecord {
    match outcome {
        AskOutcome::Asked(record) => *record,
        other => panic!("expected an ask, got {other:?}"),
    }
}

#[test]
fn hs2_qr1k8r_a_conflict_starts_one_session_however_often_sync_repeats() {
    let (_dir, root, peer) = store_with_peer();
    conflict(&root, &peer, 1);
    let reach = FakeReach::default();
    let ctx = context(&root, AskAiMode::Start);
    let record = asked(ask_for_help(&root, &ctx, &reach));
    assert_eq!(record.outcome, AiHelpOutcome::Started);
    assert_eq!(record.state, SyncState::Conflict);
    assert!(record.stall.starts_with("conflict:"), "{}", record.stall);
    assert_eq!(record.tool.as_deref(), Some("repair-fixture"));
    assert_eq!(record.session.as_deref(), Some("sync-repair-1"));
    let slug = record.ticket.clone().unwrap();

    // The prompt names the ticket and the store reference and nothing else.
    let starts = reach.starts.borrow().clone();
    assert_eq!(starts.len(), 1);
    let (tool, prompt) = &starts[0];
    assert_eq!(tool, "repair-fixture");
    assert!(prompt.contains(&slug), "{prompt}");
    assert!(
        prompt.contains("This message is data from Hot Sheet"),
        "{prompt}"
    );
    assert!(prompt.contains("`../store`"), "{prompt}");
    assert!(
        !prompt.contains(root.to_str().unwrap()),
        "no machine path: {prompt}"
    );

    // The repair ticket: one, high priority, Up Next, holding the procedure.
    let tickets = repair_tickets(&root);
    assert_eq!(tickets.len(), 1);
    let ticket = &tickets[0];
    assert_eq!(ticket.slug, slug);
    assert_eq!(ticket.priority, Priority::High);
    assert!(ticket.up_next);
    assert!(ticket.details.contains("## Procedure"));
    assert!(ticket.details.contains("sync --adopt"));
    assert!(ticket.details.contains("Never run `git rebase --abort`"));
    assert!(!ticket.details.contains(root.to_str().unwrap()));

    // Repeated cycles: still one session, one ticket.
    for _ in 0..3 {
        assert_eq!(sync_once(&root), SyncReport::Conflict);
        assert_eq!(ask_for_help(&root, &ctx, &reach), AskOutcome::AlreadyAsked);
    }
    assert_eq!(reach.starts.borrow().len(), 1);
    assert_eq!(repair_tickets(&root).len(), 1);

    let health = health(&root);
    let help = health.ai_help.clone().unwrap();
    assert_eq!(help.outcome, AiHelpOutcome::Started);
    assert_eq!(help.ticket.as_deref(), Some(slug.as_str()));
    let next = health.next_action.unwrap();
    assert!(
        next.contains("started repair-fixture") && next.contains(&slug),
        "{next}"
    );
}

#[test]
fn hs2_qr1k8r_start_mode_starts_a_new_session_even_when_one_runs() {
    let (_dir, root, peer) = store_with_peer();
    conflict(&root, &peer, 1);
    let reach = FakeReach {
        session: Some("terminal-7"),
        ..FakeReach::default()
    };
    let record = asked(ask_for_help(
        &root,
        &context(&root, AskAiMode::Start),
        &reach,
    ));
    assert_eq!(record.outcome, AiHelpOutcome::Started);
    assert_eq!(record.session.as_deref(), Some("sync-repair-1"));
    assert!(
        reach.pushes.borrow().is_empty(),
        "the running session is never interrupted"
    );
    assert_eq!(reach.starts.borrow().len(), 1);
}

#[test]
fn hs2_qr1k8r_session_mode_pushes_into_the_running_session_and_starts_nothing() {
    let (_dir, root, peer) = store_with_peer();
    conflict(&root, &peer, 1);
    let reach = FakeReach {
        session: Some("terminal-7"),
        ..FakeReach::default()
    };
    let record = asked(ask_for_help(
        &root,
        &context(&root, AskAiMode::Session),
        &reach,
    ));
    assert_eq!(record.outcome, AiHelpOutcome::SessionAsked);
    assert_eq!(record.session.as_deref(), Some("terminal-7"));
    assert_eq!(reach.pushes.borrow().len(), 1);
    assert!(reach.starts.borrow().is_empty());
    // A push that fails leaves the ticket and says why; it never falls back to a start.
    let (_dir2, root2, peer2) = store_with_peer();
    conflict(&root2, &peer2, 1);
    let failing = FakeReach {
        push_error: Some("socket closed"),
        ..FakeReach::default()
    };
    let record = asked(ask_for_help(
        &root2,
        &context(&root2, AskAiMode::Session),
        &failing,
    ));
    assert_eq!(record.outcome, AiHelpOutcome::TicketOnly);
    assert_eq!(
        record.reason.as_deref(),
        Some("pushing to the running AI session failed: socket closed")
    );
    assert!(failing.starts.borrow().is_empty());
}

#[test]
fn hs2_qr1k8r_session_mode_without_a_session_leaves_the_ticket() {
    let (_dir, root, peer) = store_with_peer();
    conflict(&root, &peer, 1);
    let reach = FakeReach::default();
    let record = asked(ask_for_help(
        &root,
        &context(&root, AskAiMode::Session),
        &reach,
    ));
    assert_eq!(record.outcome, AiHelpOutcome::TicketOnly);
    assert_eq!(
        record.reason.as_deref(),
        Some("no AI session is running in this project")
    );
    assert!(reach.starts.borrow().is_empty());
    assert_eq!(repair_tickets(&root).len(), 1);
    let health = health(&root);
    assert!(
        health
            .reason
            .unwrap()
            .contains("No AI session was asked: no AI session is running")
    );
}

#[test]
fn hs2_qr1k8r_off_mode_asks_nothing_and_writes_nothing() {
    let (_dir, root, peer) = store_with_peer();
    conflict(&root, &peer, 1);
    let reach = FakeReach::default();
    for _ in 0..2 {
        assert_eq!(
            ask_for_help(&root, &context(&root, AskAiMode::Off), &reach),
            AskOutcome::Off
        );
    }
    assert!(read_ai_record(&root).is_none());
    assert!(repair_tickets(&root).is_empty());
    assert!(reach.pushes.borrow().is_empty() && reach.starts.borrow().is_empty());
    assert!(health(&root).ai_help.is_none());
}

#[test]
fn hs2_qr1k8r_a_missing_or_failing_tool_falls_back_to_the_ticket_and_says_why() {
    let (_dir, root, peer) = store_with_peer();
    conflict(&root, &peer, 1);
    let missing = FakeReach {
        tool: Err("no default AI tool is configured for this project"),
        ..FakeReach::default()
    };
    let record = asked(ask_for_help(
        &root,
        &context(&root, AskAiMode::Start),
        &missing,
    ));
    assert_eq!(record.outcome, AiHelpOutcome::TicketOnly);
    let health = health(&root);
    let reason = health.reason.unwrap();
    assert!(
        reason.contains("No AI session was asked: no default AI tool is configured"),
        "{reason}"
    );
    assert!(
        health
            .next_action
            .unwrap()
            .contains("Follow the repair procedure")
    );
    assert_eq!(repair_tickets(&root).len(), 1);

    let (_dir2, root2, peer2) = store_with_peer();
    conflict(&root2, &peer2, 1);
    let failing = FakeReach {
        start_error: Some("the tool is not set up"),
        ..FakeReach::default()
    };
    let record = asked(ask_for_help(
        &root2,
        &context(&root2, AskAiMode::Start),
        &failing,
    ));
    assert_eq!(record.outcome, AiHelpOutcome::TicketOnly);
    assert_eq!(
        record.reason.as_deref(),
        Some("repair-fixture could not be started: the tool is not set up")
    );
}

#[test]
fn hs2_qr1k8r_the_session_end_is_recorded_once_and_only_for_its_stall() {
    let (_dir, root, peer) = store_with_peer();
    conflict(&root, &peer, 1);
    let reach = FakeReach::default();
    let record = asked(ask_for_help(
        &root,
        &context(&root, AskAiMode::Start),
        &reach,
    ));
    // An end for another stall is ignored.
    record_session_end(&root, "conflict:other", Err("boom".into()));
    assert_eq!(
        read_ai_record(&root).unwrap().outcome,
        AiHelpOutcome::Started
    );
    let done = reach.done.borrow_mut().pop().unwrap();
    done(Err("the tool exited with status 1".into()));
    let ended = read_ai_record(&root).unwrap();
    assert_eq!(ended.outcome, AiHelpOutcome::Failed);
    assert!(ended.finished_at.is_some());
    assert!(
        health(&root)
            .reason
            .unwrap()
            .contains("ended with an error: the tool exited with status 1")
    );
    // A repeated end changes nothing.
    record_session_end(&root, &record.stall, Ok(()));
    assert_eq!(
        read_ai_record(&root).unwrap().outcome,
        AiHelpOutcome::Failed
    );
    // Still unresolved: no second start.
    assert_eq!(
        ask_for_help(&root, &context(&root, AskAiMode::Start), &reach),
        AskOutcome::AlreadyAsked
    );
    assert_eq!(reach.starts.borrow().len(), 1);

    let (_dir2, root2, peer2) = store_with_peer();
    conflict(&root2, &peer2, 1);
    let reach2 = FakeReach::default();
    asked(ask_for_help(
        &root2,
        &context(&root2, AskAiMode::Start),
        &reach2,
    ));
    let done = reach2.done.borrow_mut().pop().unwrap();
    done(Ok(()));
    assert_eq!(
        read_ai_record(&root2).unwrap().outcome,
        AiHelpOutcome::Finished
    );
    assert!(health(&root2).next_action.unwrap().contains("has ended"));
}

/// Resolved then recurring: a stall after a publishing cycle is a new stall; a failing
/// cycle in between (offline) does not resolve the first.
#[test]
fn hs2_qr1k8r_a_stall_that_resolves_and_recurs_is_asked_again() {
    let (dir, root, peer) = store_with_peer();
    let reach = FakeReach::default();
    let ctx = context(&root, AskAiMode::Start);
    conflict(&root, &peer, 1);
    let first = asked(ask_for_help(&root, &ctx, &reach));

    // Offline in between: not resolved, not asked again.
    let remote = dir.path().join("remote.git");
    ok(
        &root,
        &["remote", "set-url", "origin", "/no/such/remote.git"],
    );
    assert_eq!(sync_once(&root), SyncReport::Offline);
    assert_eq!(ask_for_help(&root, &ctx, &reach), AskOutcome::NoStall);
    assert!(read_ai_record(&root).unwrap().resolved_at.is_none());
    ok(
        &root,
        &["remote", "set-url", "origin", remote.to_str().unwrap()],
    );
    assert_eq!(sync_once(&root), SyncReport::Conflict);
    assert_eq!(ask_for_help(&root, &ctx, &reach), AskOutcome::AlreadyAsked);

    // Publishing again resolves it and notes the open ticket.
    settle(&root);
    assert_eq!(ask_for_help(&root, &ctx, &reach), AskOutcome::NoStall);
    let resolved = read_ai_record(&root).unwrap();
    assert!(resolved.resolved_at.is_some());
    assert!(health(&root).ai_help.is_none());
    let ticket = &repair_tickets(&root)[0];
    assert!(
        ticket
            .notes
            .iter()
            .any(|note| note.text.contains("publishes again")),
        "the open ticket is told"
    );
    // Repeated healthy passes change nothing.
    assert_eq!(ask_for_help(&root, &ctx, &reach), AskOutcome::NoStall);
    assert_eq!(read_ai_record(&root).unwrap(), resolved);

    // A later stall is a new stall: asked again, reusing the still-open ticket.
    ok(&peer, &["pull", "--quiet"]);
    conflict(&root, &peer, 2);
    let second = asked(ask_for_help(&root, &ctx, &reach));
    assert_ne!(second.stall, first.stall);
    assert_eq!(reach.starts.borrow().len(), 2);
    assert_eq!(second.ticket, first.ticket, "the open ticket is reused");
    assert_eq!(repair_tickets(&root).len(), 1);
}

/// A stalled rebase is a stall too; its repair ticket is saved without committing onto
/// the detached `HEAD`.
#[test]
fn hs2_qr1k8r_a_stalled_rebase_is_asked_and_its_ticket_waits_uncommitted() {
    let (_dir, root, peer) = store_with_peer();
    commit_file(&root, "shared.txt", "local body\n");
    std::fs::write(peer.join("shared.txt"), "peer body\n").unwrap();
    ok(&peer, &["add", "-A"]);
    ok(&peer, &["commit", "--quiet", "-m", "peer"]);
    ok(&peer, &["push", "--quiet"]);
    ok(&root, &["fetch", "--quiet"]);
    assert!(!git_ok(&root, &["rebase", "--quiet", "@{u}"]));
    assert_eq!(sync_once(&root), SyncReport::RebaseInProgress);
    let detached = head(&root);
    let stall = current_stall(&root).unwrap();
    assert!(stall.id.starts_with("rebase:"), "{}", stall.id);
    let reach = FakeReach::default();
    let record = asked(ask_for_help(
        &root,
        &context(&root, AskAiMode::Start),
        &reach,
    ));
    assert_eq!(record.state, SyncState::RebaseInProgress);
    assert_eq!(head(&root), detached, "nothing committed during the rebase");
    let tickets = repair_tickets(&root);
    assert_eq!(tickets.len(), 1);
    assert!(reach.starts.borrow()[0].1.contains("rebase_in_progress"));
    assert_eq!(
        health(&root).ai_help.unwrap().outcome,
        AiHelpOutcome::Started
    );
}

#[test]
fn hs2_qr1k8r_store_references_never_hold_machine_paths() {
    let dir = tempfile::tempdir().unwrap();
    let code = dir.path().join("code");
    let inside = code.join("tracking");
    let sibling = dir.path().join("code.hs2");
    let elsewhere = dir.path().join("a").join("b").join("far.hs2");
    for path in [&inside, &sibling, &elsewhere] {
        std::fs::create_dir_all(path).unwrap();
    }
    assert_eq!(store_reference(&code, &inside), "tracking");
    assert_eq!(store_reference(&code, &code), ".");
    assert_eq!(store_reference(&code, &sibling), "../code.hs2");
    assert_eq!(store_reference(&code, &elsewhere), "far.hs2");
}

#[test]
fn hs2_qr1k8r_the_ask_setting_parses_validates_and_defaults_to_start() {
    use serde_json::json;
    assert_eq!(AskAiMode::parse(&json!("off")), Some(AskAiMode::Off));
    assert_eq!(
        AskAiMode::parse(&json!("session")),
        Some(AskAiMode::Session)
    );
    assert_eq!(AskAiMode::parse(&json!("start")), Some(AskAiMode::Start));
    assert_eq!(AskAiMode::parse(&json!("always")), None);
    assert_eq!(AskAiMode::parse(&json!(true)), None);
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings::new(dir.path());
    assert_eq!(ask_ai_mode(&settings), AskAiMode::Start);
    assert!(configured_ai_tool(&settings).is_none());
    settings
        .set(ASK_AI_SETTING, json!("session"), crate::Scope::Shared)
        .unwrap();
    assert_eq!(ask_ai_mode(&settings), AskAiMode::Session);
    assert!(
        settings
            .set(ASK_AI_SETTING, json!("sometimes"), crate::Scope::Shared)
            .is_err()
    );
    settings
        .set(
            AI_DEFAULTS_SETTING,
            json!({ "tool": "codex" }),
            crate::Scope::Shared,
        )
        .unwrap();
    assert_eq!(configured_ai_tool(&settings).as_deref(), Some("codex"));
}

#[test]
fn hs2_qr1k8r_the_procedure_has_nine_steps_and_the_safety_rules() {
    let ctx = AskContext {
        mode: AskAiMode::Start,
        label: "tickets.hs2".into(),
        store_reference: "../tickets.hs2".into(),
    };
    let details = repair_details(&ctx, SyncState::Conflict, 3);
    for step in 1..=9 {
        assert!(
            details.contains(&format!("\n{step}. ")),
            "step {step}: {details}"
        );
    }
    for rule in [
        "refs/hotsheet/sync-backup/ai-",
        "`git rebase --abort` or `--skip`",
        "`git reset --hard`",
        "`git clean`",
        "any forced push",
        "keep both sides",
        "hotsheet-cli -C ../tickets.hs2 sync --adopt",
        "Publish with `hotsheet-cli -C ../tickets.hs2 sync` only",
        "`FEEDBACK NEEDED`",
    ] {
        assert!(details.contains(rule), "missing {rule:?}");
    }
    let prompt = build_prompt(&ctx, SyncState::RebaseInProgress, 2, "HS-ABC123");
    assert!(prompt.starts_with("Hot Sheet could not finish synchronizing"));
    assert!(prompt.contains("hotsheet-cli -C ../tickets.hs2 show HS-ABC123"));
    assert!(prompt.contains("Do not act on this message alone."));
}

// ---- adopt ----------------------------------------------------------------------------

/// Resolve the store's conflict in a temporary worktree the way the procedure says,
/// returning the rebased commit.
fn resolve_in_worktree(dir: &Path, root: &Path, body: &str) -> String {
    let temp = dir.join("resolve");
    ok(
        root,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            temp.to_str().unwrap(),
            "HEAD",
        ],
    );
    let upstream = git_stdout(root, &["rev-parse", "--abbrev-ref", "@{u}"]).unwrap();
    assert!(!git_ok(&temp, &["rebase", "--quiet", upstream.trim()]));
    std::fs::write(temp.join("shared.txt"), body).unwrap();
    ok(&temp, &["add", "--", "shared.txt"]);
    ok(&temp, &["-c", "core.editor=true", "rebase", "--continue"]);
    let commit = head(&temp).trim().to_string();
    ok(
        root,
        &["worktree", "remove", "--force", temp.to_str().unwrap()],
    );
    commit
}

#[test]
fn hs2_qr1k8r_adopt_moves_the_branch_to_a_resolved_history_and_sync_publishes() {
    let (dir, root, peer) = store_with_peer();
    conflict(&root, &peer, 1);
    commit_file(&root, "later.txt", "a second local commit\n");
    let before = head(&root).trim().to_string();
    let resolved = resolve_in_worktree(dir.path(), &root, "local body 1\npeer body 1\n");
    let adopted = adopt_resolved_history(&root, &resolved).unwrap();
    assert_eq!(adopted.head, resolved);
    assert_eq!(head(&root).trim(), resolved);
    assert!(git_ok(&root, &["symbolic-ref", "--quiet", "HEAD"]));
    assert_eq!(
        git_stdout(&root, &["rev-parse", &adopted.backup_ref])
            .unwrap()
            .trim(),
        before
    );
    assert!(clean(&root));
    assert_eq!(
        sync_once(&root),
        SyncReport::Synced {
            pulled: false,
            pushed: true
        }
    );
    ok(&peer, &["pull", "--quiet"]);
    assert!(peer.join("later.txt").is_file());
}

#[test]
fn hs2_qr1k8r_adopt_refuses_a_history_that_loses_or_breaks_local_work() {
    let (dir, root, peer) = store_with_peer();
    conflict(&root, &peer, 1);
    commit_file(&root, "later.txt", "a second local commit\n");
    let before = head(&root);
    // The upstream alone does not contain the local commits.
    assert!(matches!(
        adopt_resolved_history(&root, "@{u}"),
        Err(AdoptError::MissingLocalCommits { missing: 2, .. })
    ));
    // The local branch itself does not contain the upstream.
    assert!(matches!(
        adopt_resolved_history(&root, "HEAD"),
        Err(AdoptError::NotOnUpstream(_))
    ));
    assert!(matches!(
        adopt_resolved_history(&root, "no-such-commit"),
        Err(AdoptError::NotACommit(_))
    ));
    // A resolution that kept conflict markers.
    let marked = resolve_in_worktree(
        dir.path(),
        &root,
        "<<<<<<< HEAD\npeer body 1\n=======\nlocal body 1\n>>>>>>> local\n",
    );
    match adopt_resolved_history(&root, &marked) {
        Err(AdoptError::ConflictMarkers { paths, .. }) => assert_eq!(paths, ["shared.txt"]),
        other => panic!("expected conflict markers, got {other:?}"),
    }
    assert_eq!(head(&root), before, "a refusal moves nothing");
    // Never during an unfinished rebase.
    ok(&root, &["fetch", "--quiet"]);
    assert!(!git_ok(&root, &["rebase", "--quiet", "@{u}"]));
    assert!(matches!(
        adopt_resolved_history(&root, "@{u}"),
        Err(AdoptError::InProgress)
    ));
}

#[test]
fn hs2_qr1k8r_only_allowed_tools_start_where_the_environment_restricts_them() {
    assert!(start_allowed("claude", None));
    assert!(start_allowed("repair-fixture", Some("repair-fixture")));
    assert!(start_allowed("codex", Some("repair-fixture, codex")));
    assert!(!start_allowed("claude", Some("repair-fixture")));
    assert!(!start_allowed("claude", Some("")));
    // In this test process the variable is forced, so an installed tool never starts.
    let (_dir, root, peer) = store_with_peer();
    conflict(&root, &peer, 1);
    let reach = FakeReach {
        tool: Ok("claude"),
        ..FakeReach::default()
    };
    let record = asked(ask_for_help(
        &root,
        &context(&root, AskAiMode::Start),
        &reach,
    ));
    assert_eq!(record.outcome, AiHelpOutcome::TicketOnly);
    assert!(reach.starts.borrow().is_empty());
    assert!(
        record
            .reason
            .unwrap()
            .contains("starting claude is disabled in this environment")
    );
}
