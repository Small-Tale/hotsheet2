//! Halt detection and per-ticket usage for Codex TUIs in Hot Sheet terminals (HS2-CYHAWB).
//!
//! Hot Sheet's Codex terminals host their own session (`--no-daemon`, HS2-4J7WCV), so the
//! server follows the session's own rollout file under the reported `CODEX_HOME`
//! ([`hotsheet_aitools::codex_rollout_watch`]). A turn that completed with an error records the
//! same terminal halt Claude's `StopFailure` hook reports; the next turn clears it. Each model
//! response's numeric usage is recorded once, attributed to the terminal session's unique live
//! claim at turn start.
//!
//! This replaces the read-only daemon thread rejoin of HS2-MN865E, which no Hot Sheet terminal
//! reached after HS2-4J7WCV and whose `thread/resume` could load a second copy of an
//! in-process thread into a user's own daemon. The server never connects to a Codex daemon.
//!
//! One watch runs per terminal. A newer session replaces it; `SessionEnd`, terminal kill, or
//! shell exit drops it, which stops its watcher.
//!
//! Codex reports a session only at `SessionStart`, and the broker keeps a terminal alive across
//! a server restart. Each watch therefore persists its session id, Codex home, report time, and
//! usage checkout (no credential, no rollout content) under the machine home, and a restarted
//! server resumes the watches of terminals the broker kept (HS2-H8BZGY, HS2-TWE913). A resumed
//! watch re-reads from the original report time; the per-response checkpoint
//! (`codex-work-<session>.json`) keeps responses recorded before the restart from counting
//! twice.

use super::signals::{
    clear_codex_question_on_input, clear_terminal_halt_on_resume, record_codex_question,
    record_terminal_halt,
};
use super::usage_store::{UsageTarget, terminal_usage_checkout, usage_target_for_checkout};
use crate::{AppState, now};
use hotsheet_aitools::codex_rollout_watch::{RolloutSignal, RolloutWatch, watch_rollout};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::{FsStore, metrics};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Machine-home directory of persisted watches, one `<terminal>.json` each.
const WATCH_DIR: &str = "codex-usage";

/// The live rollout watch for one terminal's Codex session.
pub struct CodexSessionWatch {
    session_id: String,
    codex_home: PathBuf,
    _watch: RolloutWatch,
}

/// The persisted part of a watch: identifiers and paths only, never a credential or content.
#[derive(Serialize, Deserialize)]
struct WatchRecord {
    terminal: String,
    session_id: String,
    codex_home: PathBuf,
    /// RFC 3339 time the session reported; only rollout records from then on count.
    reported_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usage_checkout: Option<String>,
}

/// Start (or keep) the watch matching terminal `id`'s reported connection: a Codex session
/// with a UUID session id is watched; anything else stops a previous watch. `reported_at` is
/// the connection's RFC 3339 report time; only rollout records from then on count.
pub(crate) fn sync_codex_session_watch(
    state: &AppState,
    id: &str,
    agent: Option<&str>,
    session_id: Option<&str>,
    codex_home: Option<&str>,
    reported_at: &str,
    usage_checkout: Option<&str>,
) {
    // Codex session ids are UUIDs; anything else cannot name a rollout.
    let Some(session_id) = session_id.filter(|id| agent == Some("codex") && is_uuid(id)) else {
        stop_codex_session_watch(state, id);
        return;
    };
    if watched_session(state, id).is_some_and(|(watched, _)| watched == session_id) {
        return;
    }
    stop_codex_session_watch(state, id);
    let codex_home = codex_home
        .map(PathBuf::from)
        .unwrap_or_else(|| state.default_codex_home());
    start_watch(
        state,
        WatchRecord {
            terminal: id.to_owned(),
            session_id: session_id.to_owned(),
            codex_home,
            reported_at: reported_at.to_owned(),
            usage_checkout: usage_checkout.map(str::to_owned),
        },
    );
}

/// The session id and Codex home terminal `id`'s watch follows, if any. After a restart this
/// is the only record of a session Codex will not report again (HS2-H8BZGY).
pub(crate) fn watched_session(state: &AppState, id: &str) -> Option<(String, PathBuf)> {
    state
        .codex_session_watches
        .lock_or_recover()
        .get(id)
        .map(|watch| (watch.session_id.clone(), watch.codex_home.clone()))
}

/// Start following `record`'s rollout and persist the record; `false` when its Codex home
/// cannot be watched.
fn start_watch(state: &AppState, record: WatchRecord) -> bool {
    let live_after = OffsetDateTime::parse(&record.reported_at, &Rfc3339)
        .unwrap_or_else(|_| OffsetDateTime::now_utc());
    let signal_state = state.clone();
    let (signal_id, signal_session) = (record.terminal.clone(), record.session_id.clone());
    let persisted_checkout = record.usage_checkout.clone();
    // The project store and claim snapshot of the current turn; a turn already running when
    // the session reported takes its snapshot at its first response (HS2-B4EQ87).
    let mut turn_target: Option<UsageTarget> = None;
    let watch = watch_rollout(
        &record.codex_home,
        &record.session_id,
        live_after,
        move |signals| {
            // A late batch from a replaced or ended session changes nothing.
            if !is_current(&signal_state, &signal_id, &signal_session) {
                return;
            }
            let target =
                || terminal_target(&signal_state, &signal_id, persisted_checkout.as_deref());
            // Responses grouped by the store their turn records into, in arrival order.
            let mut batches: Vec<(FsStore, Vec<(String, metrics::UsageEvent)>)> = Vec::new();
            // Signals apply in rollout order: the usage read before a halt or resume is
            // recorded before that terminal state changes, so whoever observes the halt
            // also sees the turn's usage (HS2-8G979N).
            let flush = |batches: &mut Vec<(FsStore, Vec<(String, metrics::UsageEvent)>)>| {
                for (store, responses) in batches.drain(..) {
                    if let Err(error) = metrics::codex_work::record_session_responses(
                        &store,
                        &signal_session,
                        responses,
                    ) {
                        tracing::warn!(%error, "recording Codex terminal usage failed");
                    }
                    // Tokens per minute moved in the project store (HS2-JTTQMY, HS2-B4EQ87).
                    let store_id = crate::multistore::store_url_id(&store);
                    crate::live_state::touch(&signal_state, &store_id);
                }
            };
            for signal in signals {
                match signal {
                    RolloutSignal::Halted {
                        error_type,
                        message,
                    } => {
                        flush(&mut batches);
                        record_terminal_halt(
                            &signal_state,
                            &signal_id,
                            Some(error_type),
                            Some(message),
                            Some("codex".to_owned()),
                        );
                    }
                    RolloutSignal::Resumed => {
                        flush(&mut batches);
                        clear_terminal_halt_on_resume(&signal_state, &signal_id);
                        turn_target = Some(target());
                    }
                    RolloutSignal::Question(asked) => {
                        record_codex_question(&signal_state, &signal_id, &signal_session, asked);
                    }
                    RolloutSignal::UserInput => {
                        clear_codex_question_on_input(&signal_state, &signal_id, &signal_session);
                    }
                    RolloutSignal::Usage(usage) => {
                        let target = turn_target.get_or_insert_with(target).clone();
                        let event = metrics::UsageEvent {
                            ts: now().as_str().to_owned(),
                            tool: "codex".to_owned(),
                            model: usage.model,
                            tokens_in: usage.tokens_in,
                            tokens_out: usage.tokens_out,
                            cost_usd: None,
                            ticket: target.ticket,
                            session: None,
                        };
                        let response = (usage.response_id, event);
                        match batches
                            .iter_mut()
                            .find(|(store, _)| store.root() == target.store.root())
                        {
                            Some((_, responses)) => responses.push(response),
                            None => batches.push((target.store, vec![response])),
                        }
                    }
                }
            }
            flush(&mut batches);
        },
    );
    // No Codex home, or it cannot be watched: nothing to follow.
    let Ok(watch) = watch else { return false };
    persist(state, &record);
    state.codex_session_watches.lock_or_recover().insert(
        record.terminal,
        CodexSessionWatch {
            session_id: record.session_id,
            codex_home: record.codex_home,
            _watch: watch,
        },
    );
    true
}

/// Where terminal `terminal`'s watch persists, or `None` for an id unsafe as a file name.
fn record_path(state: &AppState, terminal: &str) -> Option<PathBuf> {
    (!terminal.is_empty()
        && terminal.len() <= 128
        && terminal
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'))
    .then(|| {
        state
            .machine_home()
            .join(WATCH_DIR)
            .join(format!("{terminal}.json"))
    })
}

/// Best effort: a watch that cannot persist still records while this server runs.
fn persist(state: &AppState, record: &WatchRecord) {
    let Some(path) = record_path(state, &record.terminal) else {
        return;
    };
    let write = || -> std::io::Result<()> {
        let dir = path.parent().expect("record paths have a directory");
        std::fs::create_dir_all(dir)?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec(record)?)?;
        std::fs::rename(temp, &path)
    };
    if let Err(error) = write() {
        tracing::warn!(%error, "persisting a Codex rollout watch failed");
    }
}

/// At startup, resume the persisted watches of terminals the broker kept alive (HS2-H8BZGY).
/// Records of terminals that did not survive, unreadable or unsafe records, and records whose
/// Codex home can no longer be watched are removed.
pub fn resume_codex_session_watches(state: &AppState, alive: &[String]) {
    let Ok(entries) = std::fs::read_dir(state.machine_home().join(WATCH_DIR)) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let record = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<WatchRecord>(&bytes).ok())
            .filter(|record| {
                record_path(state, &record.terminal).as_deref() == Some(path.as_path())
                    && is_uuid(&record.session_id)
                    && record.codex_home.is_absolute()
                    && alive.contains(&record.terminal)
            });
        let Some(record) = record else {
            let _ = std::fs::remove_file(&path);
            continue;
        };
        // A report since startup already started this terminal's watch.
        if watched_session(state, &record.terminal).is_some() {
            continue;
        }
        if !start_watch(state, record) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Drop terminal `id`'s watch, stopping its watcher, and forget its persisted record.
pub(crate) fn stop_codex_session_watch(state: &AppState, id: &str) {
    let removed = state.codex_session_watches.lock_or_recover().remove(id);
    drop(removed);
    if let Some(path) = record_path(state, id) {
        let _ = std::fs::remove_file(path);
    }
}

/// The project store recording the terminal's Codex turn, with the ticket its Codex or shell
/// session worker uniquely holds now, if any (HS2-B4EQ87). A watch resumed after a restart has
/// no reported connection yet and uses its persisted usage checkout (HS2-H8BZGY).
fn terminal_target(state: &AppState, id: &str, persisted_checkout: Option<&str>) -> UsageTarget {
    let ai_worker = hotsheet_aitools::session_worker_id("codex", id);
    let shell_worker = super::launch::terminal_session_worker_id(id);
    let checkout =
        terminal_usage_checkout(state, id).or_else(|| persisted_checkout.map(str::to_owned));
    usage_target_for_checkout(state, checkout.as_deref(), &[&ai_worker, &shell_worker])
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "looking up Codex terminal ticket failed");
            UsageTarget {
                store: state.store.clone(),
                ticket: None,
            }
        })
}

fn is_uuid(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Whether terminal `id`'s watch still follows the Codex session `session_id`. The watch map,
/// not the reported connection, decides: every connection change re-syncs or stops the watch,
/// and a watch resumed after a restart runs before Codex reports anything again (HS2-H8BZGY).
fn is_current(state: &AppState, id: &str, session_id: &str) -> bool {
    watched_session(state, id).is_some_and(|(watched, _)| watched == session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_hba9z8_synthetic_session_does_not_name_a_rollout() {
        // The attribution fixture reports a hook without naming a real Codex session.
        // Keep its synthetic id outside the UUID boundary checked before home discovery.
        assert!(!super::is_uuid("synthetic-session"));
        assert!(super::is_uuid("00000000-0000-0000-0000-000000000001"));
    }

    fn state(root: &std::path::Path) -> AppState {
        AppState::new(
            FsStore::init(
                root.join("s.hs2"),
                &hotsheet_ticketing::StoreMetadata::new("HS"),
            )
            .unwrap(),
            "secret".into(),
        )
        .unwrap()
        .with_machine_home(root.join("home"))
    }

    fn write_record(state: &AppState, file: &str, record: serde_json::Value) -> PathBuf {
        let dir = state.machine_home().join(WATCH_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(file);
        std::fs::write(&path, record.to_string()).unwrap();
        path
    }

    /// HS2-H8BZGY: resume keeps only safe records of surviving terminals whose home can be
    /// watched, and a resumed watch is the terminal's current watch with no connection.
    #[test]
    fn hs2_h8bzgy_resume_filters_records_and_restores_a_current_watch() {
        let root = tempfile::tempdir().unwrap();
        let state = state(root.path());
        let home = root.path().join("codex");
        std::fs::create_dir_all(&home).unwrap();
        let session = "00000000-0000-0000-0000-0000000000a1";
        let record = |terminal: &str, session: &str, home: &std::path::Path| {
            serde_json::json!({
                "terminal": terminal,
                "session_id": session,
                "codex_home": home,
                "reported_at": "2026-11-06T00:00:00Z",
            })
        };
        let kept = write_record(&state, "t1.json", record("t1", session, &home));
        let dead = write_record(&state, "t2.json", record("t2", session, &home));
        let misnamed = write_record(&state, "t3.json", record("t1", session, &home));
        let bad_session = write_record(&state, "t4.json", record("t4", "not-a-uuid", &home));
        let no_home = write_record(
            &state,
            "t5.json",
            record("t5", session, &root.path().join("gone")),
        );
        let relative = write_record(
            &state,
            "t6.json",
            record("t6", session, std::path::Path::new("codex")),
        );
        let garbage = state.machine_home().join(WATCH_DIR).join("t7.json");
        std::fs::write(&garbage, "{").unwrap();
        let other = state.machine_home().join(WATCH_DIR).join("notes.txt");
        std::fs::write(&other, "x").unwrap();
        let alive: Vec<String> = ["t1", "t3", "t4", "t5", "t6", "t7"]
            .iter()
            .map(|id| (*id).to_owned())
            .collect();

        resume_codex_session_watches(&state, &alive);

        assert!(kept.exists());
        for removed in [
            &dead,
            &misnamed,
            &bad_session,
            &no_home,
            &relative,
            &garbage,
        ] {
            assert!(!removed.exists(), "{} should be removed", removed.display());
        }
        assert!(other.exists(), "non-record files are left alone");
        assert_eq!(
            watched_session(&state, "t1"),
            Some((session.to_owned(), home.clone()))
        );
        assert!(is_current(&state, "t1", session));
        assert!(!is_current(
            &state,
            "t1",
            "00000000-0000-0000-0000-0000000000b2"
        ));
        assert_eq!(watched_session(&state, "t5"), None);

        // A second resume (or one after a report already started the watch) changes nothing.
        resume_codex_session_watches(&state, &alive);
        assert!(kept.exists());
        assert!(is_current(&state, "t1", session));

        // The session ending stops the watch and forgets the record.
        stop_codex_session_watch(&state, "t1");
        assert!(!kept.exists());
        assert!(!is_current(&state, "t1", session));
    }

    /// HS2-D0QTGB: a question in the watched rollout becomes the terminal's in-app prompt, and
    /// later user input in that session clears it. Synthetic rollout records only.
    #[test]
    fn hs2_d0qtgb_a_watched_rollout_question_is_shown_then_cleared_by_user_input() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let state = state(root.path());
        let home = root.path().join("codex");
        let session = "00000000-0000-0000-0000-0000000000d1";
        let day = home.join("sessions/2026/11/12");
        std::fs::create_dir_all(&day).unwrap();
        let rollout = day.join(format!("rollout-2026-11-12T00-00-00-{session}.jsonl"));
        let line = |value: serde_json::Value| format!("{value}\n");
        std::fs::write(
            &rollout,
            line(serde_json::json!({"timestamp":"2026-11-12T00:00:00Z","type":"session_meta","payload":{"id":session}}))
                + &line(serde_json::json!({"timestamp":"2026-11-12T00:00:02Z","type":"event_msg","payload":{"type":"item_completed","item":{
                    "type":"AgentMessage","id":"item_q","delivery":"async",
                    "questions":[{"title":"Synthetic?","options":["Yes","No"]}]}}})),
        )
        .unwrap();
        write_record(
            &state,
            "tq.json",
            serde_json::json!({"terminal":"tq","session_id":session,"codex_home":home,"reported_at":"2026-11-12T00:00:01Z"}),
        );
        resume_codex_session_watches(&state, &["tq".to_owned()]);
        let wait = |want: bool| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            while state
                .terminal_questions
                .lock_or_recover()
                .contains_key("tq")
                != want
            {
                assert!(
                    std::time::Instant::now() < deadline,
                    "question presence never became {want}"
                );
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        };
        wait(true);
        assert_eq!(
            state.terminal_questions.lock_or_recover()["tq"].tool_use_id,
            "item_q"
        );
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&rollout)
            .unwrap();
        file.write_all(
            line(serde_json::json!({"timestamp":"2026-11-12T00:00:05Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"u1"}}}))
                .as_bytes(),
        )
        .unwrap();
        file.sync_all().unwrap();
        wait(false);
        stop_codex_session_watch(&state, "tq");
    }

    /// HS2-H8BZGY transitions: report persists, the same session keeps the watch, a new
    /// session replaces the record, and a non-Codex report stops it.
    #[test]
    fn hs2_h8bzgy_sync_persists_replaces_and_forgets_the_record() {
        let root = tempfile::tempdir().unwrap();
        let state = state(root.path());
        let home = root.path().join("codex");
        std::fs::create_dir_all(&home).unwrap();
        let home_arg = home.to_str();
        let first = "00000000-0000-0000-0000-0000000000c1";
        let second = "00000000-0000-0000-0000-0000000000c2";
        let path = record_path(&state, "t1").unwrap();
        let read =
            || serde_json::from_slice::<WatchRecord>(&std::fs::read(&path).unwrap()).unwrap();

        sync_codex_session_watch(
            &state,
            "t1",
            Some("codex"),
            Some(first),
            home_arg,
            "2026-11-06T01:00:00Z",
            Some("checkout-1"),
        );
        let record = read();
        assert_eq!(record.session_id, first);
        assert_eq!(record.codex_home, home);
        assert_eq!(record.reported_at, "2026-11-06T01:00:00Z");
        assert_eq!(record.usage_checkout.as_deref(), Some("checkout-1"));

        // A later report of the same session keeps the original watch and report time.
        sync_codex_session_watch(
            &state,
            "t1",
            Some("codex"),
            Some(first),
            home_arg,
            "2026-11-06T02:00:00Z",
            None,
        );
        assert_eq!(read().reported_at, "2026-11-06T01:00:00Z");

        sync_codex_session_watch(
            &state,
            "t1",
            Some("codex"),
            Some(second),
            home_arg,
            "2026-11-06T03:00:00Z",
            None,
        );
        assert_eq!(read().session_id, second);
        assert!(is_current(&state, "t1", second));
        assert!(!is_current(&state, "t1", first));

        sync_codex_session_watch(
            &state,
            "t1",
            Some("claude"),
            Some(second),
            home_arg,
            "2026-11-06T04:00:00Z",
            None,
        );
        assert!(!path.exists());
        assert_eq!(watched_session(&state, "t1"), None);

        // A home that cannot be watched records nothing.
        let gone = root.path().join("gone");
        sync_codex_session_watch(
            &state,
            "t1",
            Some("codex"),
            Some(first),
            gone.to_str(),
            "2026-11-06T05:00:00Z",
            None,
        );
        assert!(!path.exists());
        // An id unsafe as a file name still watches in memory but never persists.
        sync_codex_session_watch(
            &state,
            "../x",
            Some("codex"),
            Some(first),
            home_arg,
            "2026-11-06T06:00:00Z",
            None,
        );
        assert!(is_current(&state, "../x", first));
        assert!(record_path(&state, "../x").is_none());
    }
}
