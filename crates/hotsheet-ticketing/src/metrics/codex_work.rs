//! Numeric usage from the exact local Codex session that holds a Hot Sheet worker id.
//! Unknown rollout fields, including prompts and tool content, are discarded while parsing.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

use hotsheet_model::{ClaimEventKind, Ticket};
use serde::Deserialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{UsageEvent, read_raw, record_priced};
use crate::store::FsStore;

#[derive(Deserialize)]
struct Kind {
    #[serde(rename = "type")]
    name: String,
}

#[derive(Deserialize)]
struct Record<T> {
    timestamp: String,
    payload: T,
}

#[derive(Deserialize)]
struct SessionMeta {
    id: String,
}

#[derive(Deserialize)]
struct TurnContext {
    root_turn_id: String,
    model: String,
}

#[derive(Deserialize)]
struct TokenRecord {
    response_id: String,
    root_turn_id: String,
    usage: TokenCounts,
}

#[derive(Deserialize)]
struct TokenCounts {
    input_tokens: u64,
    output_tokens: u64,
}

#[derive(Clone)]
struct ClaimWindow {
    ticket: String,
    start: OffsetDateTime,
    end: OffsetDateTime,
}

fn timestamp(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).ok()
}

fn windows(ticket: &Ticket, worker: &str) -> Vec<ClaimWindow> {
    let mut result = Vec::new();
    let mut active: Option<(OffsetDateTime, OffsetDateTime)> = None;
    for event in &ticket.claim_history {
        let Some(at) = timestamp(event.at.as_str()) else {
            continue;
        };
        if event.worker != worker {
            if event.kind == ClaimEventKind::Claim {
                if let Some((start, end)) = active.take() {
                    result.push(ClaimWindow {
                        ticket: ticket.id.to_string(),
                        start,
                        end: end.min(at),
                    });
                }
            }
            continue;
        }
        match event.kind {
            ClaimEventKind::Claim => {
                if let Some((start, end)) = active.take() {
                    result.push(ClaimWindow {
                        ticket: ticket.id.to_string(),
                        start,
                        end: end.min(at),
                    });
                }
                if let Some(end) = event
                    .lease_expires_at
                    .as_ref()
                    .and_then(|value| timestamp(value.as_str()))
                {
                    active = Some((at, end));
                }
            }
            ClaimEventKind::Renew => {
                if let Some((_, end)) = &mut active {
                    if at < *end {
                        if let Some(new_end) = event
                            .lease_expires_at
                            .as_ref()
                            .and_then(|value| timestamp(value.as_str()))
                        {
                            *end = new_end;
                        }
                    }
                }
            }
            ClaimEventKind::Release => {
                if let Some((start, end)) = active.take() {
                    result.push(ClaimWindow {
                        ticket: ticket.id.to_string(),
                        start,
                        end: end.min(at),
                    });
                }
            }
        }
    }
    if let Some((start, end)) = active {
        result.push(ClaimWindow {
            ticket: ticket.id.to_string(),
            start,
            end,
        });
    }
    result
}

fn find_rollout(root: &Path, session: &str, depth: usize) -> io::Result<Option<PathBuf>> {
    if depth == 0 {
        return Ok(None);
    }
    let Ok(entries) = fs::read_dir(root) else {
        return Ok(None);
    };
    for entry in entries {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if let Some(path) = find_rollout(&entry.path(), session, depth - 1)? {
                return Ok(Some(path));
            }
        } else if kind.is_file()
            && entry
                .file_name()
                .to_string_lossy()
                .ends_with(&format!("-{session}.jsonl"))
        {
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

/// Capture only records from this process's exact session/worker/store binding. A project
/// launch supplies its linked Git stores separately from its project identity. A missing
/// binding is ordinary (for human CLI calls and server-managed terminals).
pub fn capture_current_session(store: &FsStore, worker: &str) -> io::Result<usize> {
    let Ok(session) = std::env::var("CODEX_SESSION_ID") else {
        return Ok(0);
    };
    let Ok(bound_worker) = std::env::var("HOTSHEET_WORKER_ID") else {
        return Ok(0);
    };
    let Ok(project) = std::env::var("HOTSHEET_PROJECT") else {
        return Ok(0);
    };
    let Some(store_root) = fs::canonicalize(store.root()).ok() else {
        return Ok(0);
    };
    let direct_store = fs::canonicalize(project).ok().as_deref() == Some(store_root.as_path());
    let linked_store = std::env::var("HOTSHEET_USAGE_STORES")
        .ok()
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .is_some_and(|paths| {
            paths
                .iter()
                .any(|path| fs::canonicalize(path).ok().as_deref() == Some(store_root.as_path()))
        });
    if worker != bound_worker || !valid_session_id(&session) || !(direct_store || linked_store) {
        return Ok(0);
    }
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|value| PathBuf::from(value).join(".codex")));
    let Some(home) = home else {
        return Ok(0);
    };
    capture_session(store, worker, &session, &home)
}

fn valid_session_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

/// Read an observed Codex rollout format. Only exact, unambiguous live claim windows
/// receive events; session/response ids deduplicate repeated capture calls.
pub fn capture_session(
    store: &FsStore,
    worker: &str,
    session: &str,
    codex_home: &Path,
) -> io::Result<usize> {
    if !valid_session_id(session) {
        return Ok(0);
    }
    let Some(path) = find_rollout(&codex_home.join("sessions"), session, 5)? else {
        return Ok(0);
    };
    let listing = store.list_tickets_resilient().map_err(io::Error::other)?;
    if !listing.corrupt.is_empty() {
        return Ok(0);
    }
    let claims: Vec<_> = listing
        .tickets
        .iter()
        .flat_map(|ticket| windows(ticket, worker))
        .collect();
    if claims.is_empty() {
        return Ok(0);
    }
    let mut models = HashMap::new();
    let mut events = Vec::new();
    let mut verified = false;
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        let Ok(kind) = serde_json::from_str::<Kind>(&line) else {
            continue;
        };
        match kind.name.as_str() {
            "session_meta" => {
                if let Ok(meta) = serde_json::from_str::<Record<SessionMeta>>(&line) {
                    verified = meta.payload.id == session;
                }
            }
            "turn_context" if verified => {
                if let Ok(context) = serde_json::from_str::<Record<TurnContext>>(&line) {
                    if context.payload.model.len() <= 128
                        && context
                            .payload
                            .model
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"-._/".contains(&byte))
                    {
                        models.insert(context.payload.root_turn_id, context.payload.model);
                    }
                }
            }
            "token_usage_record" if verified => {
                let Ok(record) = serde_json::from_str::<Record<TokenRecord>>(&line) else {
                    continue;
                };
                let Some(at) = timestamp(&record.timestamp) else {
                    continue;
                };
                let id = &record.payload.response_id;
                if id.is_empty()
                    || id.len() > 128
                    || !id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-._".contains(&byte))
                {
                    continue;
                }
                if record.payload.usage.input_tokens == 0 && record.payload.usage.output_tokens == 0
                {
                    continue;
                }
                let mut matches = claims
                    .iter()
                    .filter(|claim| claim.start <= at && at < claim.end);
                let Some(claim) = matches.next() else {
                    continue;
                };
                if matches.next().is_some() {
                    continue;
                }
                events.push((record.timestamp, record.payload, claim.ticket.clone()));
            }
            _ => {}
        }
    }
    if !verified {
        return Ok(0);
    }
    let local = store.root().join("metrics/local");
    fs::create_dir_all(&local)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&local, fs::Permissions::from_mode(0o700))?;
    }
    super::ensure_gitignored(store, "metrics/local/")?;
    let lock_file = File::options()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(local.join("codex-work.lock"))?;
    let _lock = crate::file_lock::FileLock::acquire(lock_file)?;
    let existing = read_raw(store)?;
    let checkpoint = local.join(format!("codex-work-{session}.json"));
    let mut seen: HashSet<String> = match fs::read(&checkpoint) {
        Ok(bytes) => serde_json::from_slice::<Vec<String>>(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
            .into_iter()
            .collect(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => HashSet::new(),
        Err(error) => return Err(error),
    };
    // Remember a server-managed terminal even after its raw metrics are pruned.
    let watched = format!("codex-work-managed:{session}");
    if existing
        .iter()
        .any(|event| event.session.as_deref() == Some(session))
    {
        seen.insert(watched.clone());
    }
    if seen.contains(&watched) {
        write_checkpoint(&checkpoint, seen)?;
        return Ok(0);
    }
    seen.extend(
        existing
            .iter()
            .filter_map(|event| event.session.as_ref())
            .filter(|value| value.starts_with(&format!("codex-work:{session}:")))
            .cloned(),
    );
    let mut added = 0;
    for (ts, payload, ticket) in events {
        let key = format!("codex-work:{session}:{}", payload.response_id);
        if !seen.insert(key.clone()) {
            continue;
        }
        let event = UsageEvent {
            ts,
            tool: "codex".to_owned(),
            model: models.get(&payload.root_turn_id).cloned(),
            tokens_in: payload.usage.input_tokens,
            tokens_out: payload.usage.output_tokens,
            cost_usd: None,
            ticket: Some(ticket),
            session: Some(key.clone()),
        };
        record_priced(store, event)?;
        added += 1;
    }
    write_checkpoint(&checkpoint, seen)?;
    Ok(added)
}

fn write_checkpoint(path: &Path, seen: HashSet<String>) -> io::Result<()> {
    if seen.is_empty() {
        return Ok(());
    }
    let mut keys: Vec<_> = seen.into_iter().collect();
    keys.sort();
    let temporary = path.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec(&keys).map_err(io::Error::other)?,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ops::{self, NewTicket},
        store::StoreMetadata,
    };
    use hotsheet_model::{Timestamp, Ulid};

    fn at(value: &str) -> Timestamp {
        Timestamp::new(value)
    }

    #[test]
    fn exact_claim_windows_deduplicate_records_and_discard_unrelated_content() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path().join("store"), &StoreMetadata::new("HS")).unwrap();
        let home = dir.path().join("codex");
        let session = "00000000-0000-7000-8000-000000000001";
        let rollout = home
            .join("sessions/2026/10/21")
            .join(format!("rollout-2026-10-21T00-00-00-{session}.jsonl"));
        fs::create_dir_all(rollout.parent().unwrap()).unwrap();
        let first = ops::create(
            &store,
            Ulid::new(),
            "HS",
            at("2026-10-21T00:00:00Z"),
            NewTicket {
                title: "first".into(),
                category: "task".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let second = ops::create(
            &store,
            Ulid::new(),
            "HS",
            at("2026-10-21T00:00:00Z"),
            NewTicket {
                title: "second".into(),
                category: "task".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let worker = "codex-exact";
        ops::claim(
            &store,
            &first.id,
            &at("2026-10-21T01:00:00Z"),
            at("2026-10-21T02:00:00Z"),
            worker,
            None,
        )
        .unwrap();
        ops::release(&store, &first.id, at("2026-10-21T01:30:00Z"), worker, false).unwrap();
        ops::claim(
            &store,
            &second.id,
            &at("2026-10-21T01:30:00Z"),
            at("2026-10-21T02:30:00Z"),
            worker,
            None,
        )
        .unwrap();
        let rows = [
            serde_json::json!({"type":"session_meta","timestamp":"2026-10-21T00:00:00Z","payload":{"id":session}}),
            serde_json::json!({"type":"turn_context","timestamp":"2026-10-21T01:00:00Z","payload":{"root_turn_id":"turn-1","model":"gpt-6-sol"}}),
            serde_json::json!({"type":"response_item","timestamp":"2026-10-21T01:05:00Z","payload":{"secret":"do-not-store-this"}}),
            serde_json::json!({"type":"token_usage_record","timestamp":"2026-10-21T00:59:59Z","payload":{"response_id":"before","root_turn_id":"turn-1","usage":{"input_tokens":1,"output_tokens":1}}}),
            serde_json::json!({"type":"token_usage_record","timestamp":"2026-10-21T01:10:00Z","payload":{"response_id":"response-one","root_turn_id":"turn-1","usage":{"input_tokens":100,"output_tokens":10}}}),
            serde_json::json!({"type":"token_usage_record","timestamp":"2026-10-21T01:40:00Z","payload":{"response_id":"response-two","root_turn_id":"turn-1","usage":{"input_tokens":200,"output_tokens":20}}}),
            serde_json::json!({"type":"token_usage_record","timestamp":"2026-10-21T02:40:00Z","payload":{"response_id":"after","root_turn_id":"turn-1","usage":{"input_tokens":1,"output_tokens":1}}}),
        ];
        fs::write(
            &rollout,
            rows.iter()
                .map(serde_json::Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        assert_eq!(capture_session(&store, worker, session, &home).unwrap(), 2);
        assert_eq!(capture_session(&store, worker, session, &home).unwrap(), 0);
        let first_usage = super::super::ticket_usage(&store, &first.id.to_string()).unwrap();
        let second_usage = super::super::ticket_usage(&store, &second.id.to_string()).unwrap();
        assert_eq!(first_usage.events.len(), 1);
        assert_eq!(second_usage.events.len(), 1);
        assert_eq!(first_usage.events[0].tokens_in, 100);
        assert_eq!(second_usage.events[0].tokens_out, 20);
        assert_eq!(first_usage.events[0].model.as_deref(), Some("gpt-6-sol"));
        assert!(
            !fs::read_to_string(store.root().join("metrics/raw/2026-10-21.jsonl"))
                .unwrap()
                .contains("do-not-store-this")
        );
        super::super::roll_up_through(&store, "2026-10-22").unwrap();
        super::super::prune_raw_before(&store, "2026-10-22").unwrap();
        assert_eq!(capture_session(&store, worker, session, &home).unwrap(), 0);
        assert!(super::super::read_raw(&store).unwrap().is_empty());
    }

    #[test]
    fn overlapping_claims_and_unverified_session_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path().join("store"), &StoreMetadata::new("HS")).unwrap();
        let home = dir.path().join("codex");
        let session = "00000000-0000-7000-8000-000000000002";
        let rollout = home
            .join("sessions/2026/10/21")
            .join(format!("rollout-2026-10-21T00-00-00-{session}.jsonl"));
        fs::create_dir_all(rollout.parent().unwrap()).unwrap();
        let first = ops::create(
            &store,
            Ulid::new(),
            "HS",
            at("2026-10-21T00:00:00Z"),
            NewTicket {
                title: "first".into(),
                category: "task".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let second = ops::create(
            &store,
            Ulid::new(),
            "HS",
            at("2026-10-21T00:00:00Z"),
            NewTicket {
                title: "second".into(),
                category: "task".into(),
                ..Default::default()
            },
        )
        .unwrap();
        for ticket in [&first, &second] {
            ops::claim(
                &store,
                &ticket.id,
                &at("2026-10-21T01:00:00Z"),
                at("2026-10-21T02:00:00Z"),
                "codex-exact",
                None,
            )
            .unwrap();
        }
        let usage = serde_json::json!({"type":"token_usage_record","timestamp":"2026-10-21T01:10:00Z","payload":{"response_id":"response","root_turn_id":"turn","usage":{"input_tokens":10,"output_tokens":1}}});
        fs::write(&rollout, format!("{}\n{usage}\n", serde_json::json!({"type":"session_meta","timestamp":"2026-10-21T00:00:00Z","payload":{"id":"wrong"}}))).unwrap();
        assert_eq!(
            capture_session(&store, "codex-exact", session, &home).unwrap(),
            0
        );
        fs::write(&rollout, format!("{}\n{usage}\n", serde_json::json!({"type":"session_meta","timestamp":"2026-10-21T00:00:00Z","payload":{"id":session}}))).unwrap();
        assert_eq!(
            capture_session(&store, "codex-exact", session, &home).unwrap(),
            0
        );
        ops::renew(
            &store,
            &first.id,
            at("2026-10-21T01:30:00Z"),
            at("2026-10-21T03:00:00Z"),
            "codex-exact",
        )
        .unwrap();
        let later = serde_json::json!({"type":"token_usage_record","timestamp":"2026-10-21T02:30:00Z","payload":{"response_id":"after-overlap","root_turn_id":"turn","usage":{"input_tokens":30,"output_tokens":3}}});
        use std::io::Write;
        writeln!(
            File::options().append(true).open(&rollout).unwrap(),
            "{later}"
        )
        .unwrap();
        assert_eq!(
            capture_session(&store, "codex-exact", session, &home).unwrap(),
            1
        );
        assert_eq!(
            capture_session(&store, "codex-exact", session, &home).unwrap(),
            0
        );
        assert_eq!(
            super::super::ticket_usage(&store, &first.id.to_string())
                .unwrap()
                .events
                .len(),
            1
        );
        assert!(
            super::super::ticket_usage(&store, &second.id.to_string())
                .unwrap()
                .events
                .is_empty()
        );
    }

    #[test]
    fn watched_terminal_session_stays_excluded_after_raw_prune() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path().join("store"), &StoreMetadata::new("HS")).unwrap();
        let home = dir.path().join("codex");
        let session = "00000000-0000-7000-8000-000000000003";
        let rollout = home
            .join("sessions/2026/10/21")
            .join(format!("rollout-2026-10-21T00-00-00-{session}.jsonl"));
        fs::create_dir_all(rollout.parent().unwrap()).unwrap();
        let ticket = ops::create(
            &store,
            Ulid::new(),
            "HS",
            at("2026-10-21T00:00:00Z"),
            NewTicket {
                title: "watched".into(),
                category: "task".into(),
                ..Default::default()
            },
        )
        .unwrap();
        ops::claim(
            &store,
            &ticket.id,
            &at("2026-10-21T01:00:00Z"),
            at("2026-10-21T02:00:00Z"),
            "codex-exact",
            None,
        )
        .unwrap();
        fs::write(
            &rollout,
            format!(
                "{}\n{}\n",
                serde_json::json!({"type":"session_meta","timestamp":"2026-10-21T00:00:00Z","payload":{"id":session}}),
                serde_json::json!({"type":"token_usage_record","timestamp":"2026-10-21T01:10:00Z","payload":{"response_id":"response","root_turn_id":"turn","usage":{"input_tokens":10,"output_tokens":1}}}),
            ),
        )
        .unwrap();
        record_priced(
            &store,
            UsageEvent {
                ts: "2026-10-21T01:10:00Z".into(),
                tool: "codex".into(),
                model: None,
                tokens_in: 10,
                tokens_out: 1,
                cost_usd: None,
                ticket: Some(ticket.id.to_string()),
                session: Some(session.into()),
            },
        )
        .unwrap();
        assert_eq!(
            capture_session(&store, "codex-exact", session, &home).unwrap(),
            0
        );
        super::super::roll_up_through(&store, "2026-10-22").unwrap();
        super::super::prune_raw_before(&store, "2026-10-22").unwrap();
        assert_eq!(
            capture_session(&store, "codex-exact", session, &home).unwrap(),
            0
        );
        assert!(super::super::read_raw(&store).unwrap().is_empty());
    }
}
