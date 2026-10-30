//! HS2-MFD44D process and journal transitions. Child tests use only synthetic homes.
#![cfg(any(unix, windows))]

use super::*;
use crate::config::{ChannelConfig, NotificationConfig};
use crate::dispatch::{Dispatcher, LOG_FILE};
use crate::notifier::{DeliveryError, LiveRunner, Notifier};
use crate::registry::Registry;
use crate::render::Rendered;
use crate::settings::Resolved;
use hotsheet_sync::LockExt;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

fn record(id: &str, channel: &str, held: bool) -> DeliveryRecord {
    DeliveryRecord {
        id: id.into(),
        at: "2026-01-01T12:00:00Z".into(),
        event: EventKind::TicketBlocked,
        channel: channel.into(),
        notifier: "synthetic".into(),
        rule: None,
        outcome: if held {
            Outcome::Suppressed
        } else {
            Outcome::Delivered
        },
        attempts: 0,
        error: held.then(|| "synthetic hold".into()),
        provider_id: None,
        project: None,
        ticket: None,
        truncated: false,
        omitted_attachments: 0,
        dedupe: None,
        held,
    }
}

fn reservation(id: &str, channel: &str) -> DeliveryRecord {
    DeliveryRecord {
        event: EventKind::HeldSummary,
        outcome: Outcome::Failed,
        ..record(id, channel, false)
    }
}

fn seed(path: &Path, records: &[DeliveryRecord]) {
    // Preexisting legacy-shaped history, before any dispatcher/child can own the journal.
    // Production append/compaction is exercised by the independent child processes.
    let _guard = lock(path, "journal").unwrap();
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .unwrap();
    for record in records {
        writeln!(file, "{}", serde_json::to_string(record).unwrap()).unwrap();
    }
    file.sync_all().unwrap();
}

fn config() -> NotificationConfig {
    NotificationConfig {
        channels: vec![ChannelConfig {
            id: "synthetic".into(),
            notifier: "synthetic".into(),
            label: None,
            enabled: true,
            settings: BTreeMap::new(),
            plugin_digest: None,
            quiet_hours: None,
            max_per_hour: None,
        }],
        ..Default::default()
    }
}

struct SyntheticNotifier {
    descriptor: crate::capability::NotifierDescriptor,
    home: PathBuf,
    mode: String,
}

impl Notifier for SyntheticNotifier {
    fn descriptor(&self) -> &crate::capability::NotifierDescriptor {
        &self.descriptor
    }

    fn deliver(
        &self,
        channel: &str,
        _rendered: &Rendered,
        _settings: &Resolved,
    ) -> Result<Option<String>, DeliveryError> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.home.join("synthetic-transports.jsonl"))
            .unwrap();
        writeln!(file, "{}", serde_json::json!({"channel": channel})).unwrap();
        file.sync_all().unwrap();
        if self.mode == "blocked" {
            ready("TRANSPORT_ENTERED");
            proceed();
        }
        if self.mode == "reentrant" {
            Process::start(&self.home, "busy-write").finish();
        }
        if self.mode == "failure" {
            Err(DeliveryError::permanent("synthetic permanent failure"))
        } else {
            Ok(None)
        }
    }
}

fn dispatcher(home: &Path, mode: &str) -> Dispatcher {
    let mut descriptor = crate::builtin::ntfy::descriptor();
    descriptor.id = "synthetic".into();
    descriptor.settings.clear();
    let registry = Registry::with_builtins(
        vec![Arc::new(SyntheticNotifier {
            descriptor,
            home: home.to_path_buf(),
            mode: mode.into(),
        })],
        None,
        Arc::new(LiveRunner),
    );
    Dispatcher::new(
        Arc::new(registry),
        Arc::new(BTreeMap::<String, String>::new()),
        Some(home),
    )
}

fn ready(event: &str) {
    println!("HS2_MFD44D:{event}");
    std::io::stdout().flush().unwrap();
}

fn proceed() {
    std::io::stdin().read_exact(&mut [0]).unwrap();
}

/// A separate OS process is essential: independent threads share the old static FLUSH.
struct Process {
    child: Child,
    events: mpsc::Receiver<String>,
    stderr: Arc<Mutex<String>>,
}

impl Process {
    fn start(home: &Path, mode: &str) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "delivery_log::tests::hs2_mfd44d_dispatch_child",
                "--nocapture",
            ])
            .env("HS2_MFD44D_SYNTHETIC_HOME", home)
            .env("HS2_MFD44D_MODE", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, events) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let line = line.unwrap();
                if let Some(event) = line.split_once("HS2_MFD44D:").map(|(_, event)| event) {
                    let _ = tx.send(event.to_owned());
                }
            }
        });
        let stderr = Arc::new(Mutex::new(String::new()));
        let collected = stderr.clone();
        let pipe = child.stderr.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines() {
                let mut text = collected.lock_or_recover();
                text.push_str(&line.unwrap_or_else(|error| error.to_string()));
                text.push('\n');
            }
        });
        let process = Self {
            child,
            events,
            stderr,
        };
        process.expect("READY");
        process
    }

    fn expect(&self, event: &str) {
        let actual = self.events.recv_timeout(Duration::from_secs(30));
        assert!(
            actual.as_ref().is_ok_and(|actual| actual == event),
            "expected {event}, received {actual:?}; child stderr:\n{}",
            self.stderr.lock_or_recover()
        );
    }

    fn release(&mut self) {
        self.child.stdin.as_mut().unwrap().write_all(b"x").unwrap();
    }

    fn finish(mut self) {
        self.expect("FINISHED");
        assert!(
            self.child.wait().unwrap().success(),
            "child stderr:\n{}",
            self.stderr.lock_or_recover()
        );
    }

    fn kill(mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn hs2_mfd44d_dispatch_child() {
    let Some(home) = std::env::var_os("HS2_MFD44D_SYNTHETIC_HOME") else {
        return;
    };
    let home = PathBuf::from(home);
    let mode = std::env::var("HS2_MFD44D_MODE").unwrap();
    let path = home.join(LOG_FILE);
    ready("READY");
    match mode.as_str() {
        "reserved" => {
            let _flush = lock_flush(&path).unwrap().unwrap();
            let mut journal = Journal::open(&path).unwrap();
            let held = journal.held("synthetic");
            journal
                .reserve(&reservation("reserved-summary", "synthetic"), &held)
                .unwrap();
            drop(journal);
            ready("RESERVED");
            proceed();
        }
        "append-a" | "append-b" => {
            proceed();
            for index in 0..100 {
                append_record(
                    &path,
                    &record(&format!("{mode}-{index}"), "synthetic", false),
                )
                .unwrap();
            }
        }
        "append-during-send" => {
            // Exercise compaction while a reservation is pending and its original held
            // record falls outside normal retention. The new hold lands after that marker.
            for index in 0..200 {
                append_record(
                    &path,
                    &record(&format!("filler-{index}"), "synthetic", false),
                )
                .unwrap();
            }
            append_record(&path, &record("new-held", "synthetic", true)).unwrap();
        }
        "contending" => {
            assert!(
                FileLock::try_acquire(lock_file(&path, "flush").unwrap())
                    .unwrap()
                    .is_none()
            );
            ready("CONTENDING");
            dispatcher(&home, "success").flush_held(&config());
        }
        "busy-write" => {
            let mut quiet = config();
            quiet.channels[0].quiet_hours = Some(crate::config::QuietHours {
                start: "22:00".into(),
                end: "07:00".into(),
                utc_offset: "+00:00".into(),
                allow_errors: false,
            });
            quiet.rules = vec![crate::config::Rule {
                id: "synthetic-rule".into(),
                events: vec![EventKind::TicketBlocked],
                channels: vec!["synthetic".into()],
                enabled: true,
                filter: Default::default(),
                cooldown_seconds: 0,
            }];
            let now = time::OffsetDateTime::parse(
                "2026-01-01T23:00:00Z",
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap();
            let writer = dispatcher(&home, "success").with_clock(Arc::new(move || now));
            let records = writer.dispatch(
                &quiet,
                &crate::Message::new(EventKind::TicketBlocked, "synthetic write"),
            );
            assert_eq!(records.len(), 1);
            assert!(records[0].held);
        }
        "recover-removed" => {
            let results = dispatcher(&home, "success").flush_held(&NotificationConfig::default());
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].outcome, Outcome::Failed);
            assert!(
                results[0]
                    .error
                    .as_ref()
                    .unwrap()
                    .contains("outcome unknown")
            );
        }
        _ => {
            dispatcher(&home, &mode).flush_held(&config());
        }
    }
    ready("FINISHED");
}

fn transports(home: &Path) -> usize {
    std::fs::read_to_string(home.join("synthetic-transports.jsonl"))
        .unwrap_or_default()
        .lines()
        .count()
}

#[test]
fn hs2_mfd44d_two_processes_and_restarts_send_one_summary() {
    for mode in ["blocked", "failure"] {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(LOG_FILE);
        seed(&path, &[record("held-1", "synthetic", true)]);
        let mut first = Process::start(home.path(), mode);
        if mode == "blocked" {
            first.expect("TRANSPORT_ENTERED");
            let second = Process::start(home.path(), "contending");
            second.expect("CONTENDING");
            second.finish();
            first.release();
            first.finish();
        } else {
            first.finish();
        }
        Process::start(home.path(), "success").finish();
        assert_eq!(transports(home.path()), 1);
        let journal = Journal::open(&path).unwrap();
        assert!(journal.held("synthetic").is_empty());
        let summaries: Vec<_> = journal
            .records()
            .into_iter()
            .filter(|r| r.event == EventKind::HeldSummary)
            .collect();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].attempts, 1);
        assert_eq!(
            summaries[0].outcome,
            if mode == "failure" {
                Outcome::Failed
            } else {
                Outcome::Delivered
            }
        );
    }
}

#[test]
fn hs2_mfd44d_process_exit_recovers_unknown_without_resending() {
    for mode in ["reserved", "blocked"] {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(LOG_FILE);
        seed(&path, &[record("held-1", "synthetic", true)]);
        let child = Process::start(home.path(), mode);
        child.expect(if mode == "reserved" {
            "RESERVED"
        } else {
            "TRANSPORT_ENTERED"
        });
        child.kill();
        let before = transports(home.path());
        // Recovery does not depend on the old channel still existing or being enabled.
        Process::start(home.path(), "recover-removed").finish();
        Process::start(home.path(), "success").finish();
        assert_eq!(transports(home.path()), before);
        let recovered = read_records(&path).unwrap().pop().unwrap();
        assert_eq!(recovered.event, EventKind::HeldSummary);
        assert_eq!(recovered.outcome, Outcome::Failed);
        assert_eq!(recovered.attempts, 0);
        assert!(recovered.error.unwrap().contains("outcome unknown"));
        append_record(&path, &record("held-2", "synthetic", true)).unwrap();
        Process::start(home.path(), "success").finish();
        assert_eq!(
            transports(home.path()),
            before + 1,
            "a new interval must refill"
        );
    }
}

#[test]
fn hs2_mfd44d_new_holds_survive_send_and_pending_compaction() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(LOG_FILE);
    let mut history = vec![record("held-1", "synthetic", true)];
    history.extend((0..899).map(|index| record(&format!("old-{index}"), "synthetic", false)));
    seed(&path, &history);
    let mut first = Process::start(home.path(), "blocked");
    first.expect("TRANSPORT_ENTERED");
    Process::start(home.path(), "append-during-send").finish();
    {
        let journal = Journal::open(&path).unwrap();
        assert!(
            journal.records().iter().all(|record| record.id != "held-1"),
            "compaction must have removed the old held record"
        );
        assert!(
            journal
                .entries
                .iter()
                .any(|entry| matches!(entry, Entry::Reservation(_))),
            "the pending reservation must survive that compaction"
        );
    }
    first.release();
    first.finish();
    assert_eq!(
        Journal::open(&path).unwrap().held("synthetic")[0].id,
        "new-held"
    );
    Process::start(home.path(), "success").finish();
    Process::start(home.path(), "success").finish();
    assert_eq!(transports(home.path()), 2);
    assert!(Journal::open(&path).unwrap().held("synthetic").is_empty());
}

#[test]
fn hs2_mfd44d_concurrent_append_compaction_keeps_both_processes_records() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(LOG_FILE);
    let old: Vec<_> = (0..900)
        .map(|i| record(&format!("old-{i}"), "synthetic", false))
        .collect();
    seed(&path, &old);
    let mut a = Process::start(home.path(), "append-a");
    let mut b = Process::start(home.path(), "append-b");
    a.release();
    b.release();
    a.finish();
    b.finish();
    let records = read_records(&path).unwrap();
    let ids: HashSet<_> = records.iter().map(|record| record.id.as_str()).collect();
    for mode in ["append-a", "append-b"] {
        for index in 0..100 {
            assert!(ids.contains(format!("{mode}-{index}").as_str()));
        }
    }
    assert!(records.len() <= COMPACT_AT);
}

#[test]
fn hs2_mfd44d_legacy_watermark_and_independent_channels_remain_compatible() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(LOG_FILE);
    seed(
        &path,
        &[
            record("old-held", "synthetic", true),
            reservation("legacy-summary", "synthetic"),
            record("new-held", "synthetic", true),
            record("other-held", "other", true),
        ],
    );
    let mut journal = Journal::open(&path).unwrap();
    let held = journal.held("synthetic");
    assert_eq!(
        held.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        ["new-held"]
    );
    journal
        .reserve(&reservation("modern-summary", "synthetic"), &held)
        .unwrap();
    assert!(journal.held("synthetic").is_empty());
    assert_eq!(journal.held("other")[0].id, "other-held");
    assert_eq!(journal.recover_interrupted().unwrap().len(), 1);
    assert!(journal.recover_interrupted().unwrap().is_empty());
}

#[test]
fn hs2_mfd44d_storage_and_reservation_errors_fail_before_transport() {
    for corrupt in [
        "{\"held_summary_reservation\":",
        "{\"held_summary_reservation\":{\"version\":2}}",
    ] {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(LOG_FILE);
        seed(&path, &[record("held-1", "synthetic", true)]);
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{corrupt}").unwrap();
        assert!(
            dispatcher(home.path(), "success")
                .flush_held(&config())
                .is_empty()
        );
        assert_eq!(transports(home.path()), 0);
    }
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(LOG_FILE);
    let mut journal = Journal::open(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(
        journal
            .reserve(
                &reservation("summary", "synthetic"),
                &[record("held", "synthetic", true)]
            )
            .is_err()
    );
    drop(journal);
    assert!(
        dispatcher(home.path(), "success")
            .flush_held(&config())
            .is_empty()
    );
    assert_eq!(transports(home.path()), 0);
}

#[test]
fn hs2_mfd44d_interrupted_completion_tail_recovers_once() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(LOG_FILE);
    let held = record("held", "synthetic", true);
    seed(&path, std::slice::from_ref(&held));
    Journal::open(&path)
        .unwrap()
        .reserve(&reservation("summary", "synthetic"), &[held])
        .unwrap();
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"{\"id\":\"summary\"").unwrap();
    drop(file);
    let mut journal = Journal::open(&path).unwrap();
    assert_eq!(journal.recover_interrupted().unwrap().len(), 1);
    drop(journal);
    let mut reopened = Journal::open(&path).unwrap();
    assert!(reopened.recover_interrupted().unwrap().is_empty());
    assert!(reopened.held("synthetic").is_empty());
    assert_eq!(
        reopened
            .records()
            .iter()
            .filter(|r| r.event == EventKind::HeldSummary)
            .count(),
        1
    );
}

#[test]
fn hs2_mfd44d_notifier_reentry_skips_busy_flush_but_keeps_the_write() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(LOG_FILE);
    seed(&path, &[record("held-1", "synthetic", true)]);
    Process::start(home.path(), "reentrant").finish();
    assert_eq!(transports(home.path()), 1);
    assert_eq!(Journal::open(&path).unwrap().held("synthetic").len(), 1);
    Process::start(home.path(), "success").finish();
    assert_eq!(transports(home.path()), 2);
    assert!(Journal::open(&path).unwrap().held("synthetic").is_empty());
}
