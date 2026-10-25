//! Machine-local child-process provenance (HS2-GDCMXE). Records contain identifiers and
//! lifecycle facts only. A launch is a unique ID, never just a reusable OS PID.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_RECORD_BYTES: u64 = 16 * 1024;
const MAX_ENDED: usize = 1024;
const MAX_UNRESOLVED: usize = 1024;
const RETAIN_MS: u64 = 30 * 24 * 60 * 60 * 1000;
const STALE_TEMP_SECS: u64 = 60 * 60;

/// A fixed vocabulary prevents commands, arguments, and customer content entering records.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    TerminalShell,
    TerminalAi,
    TerminalBroker,
    AiChannel,
    AiAppServer,
    AiAcp,
    AiSpawn,
    AiProbe,
    CodexQueue,
    CodexDaemon,
    CliExternal,
    CliSetup,
    CliMigrator,
    CodeReviewDifftool,
    ServerCommand,
    RepositoryHelper,
    BrokerSupervisor,
    GuidanceGit,
    Rulesync,
}

/// The observed end of one child; a crash leaves this absent rather than inventing an exit.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    Completed,
    Failed,
    Interrupted,
    Killed,
    Unknown,
}

/// Caller-supplied identifiers. `binary_revision` describes the Hot Sheet build supervising
/// the child; it is not the child command, path, arguments, or environment.
#[derive(Clone, Debug)]
pub struct LaunchMeta {
    pub pid: u32,
    pub supervisor_pid: u32,
    pub supervisor: String,
    pub purpose: Purpose,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub binary_revision: Option<String>,
}

/// Fixed provenance shared by commands whose PID is only known after spawning. It never
/// inspects the command, arguments, environment, or output (HS2-G9TF60).
#[derive(Clone, Debug)]
pub struct LaunchContext {
    pub journal: ProcessJournal,
    pub supervisor: &'static str,
    pub purpose: Purpose,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub binary_revision: Option<String>,
}

impl LaunchContext {
    pub fn spawn(&self, command: &mut Command) -> io::Result<TrackedChild> {
        let child = command.spawn()?;
        let record = self.journal.start(LaunchMeta {
            pid: child.id(),
            supervisor_pid: std::process::id(),
            supervisor: self.supervisor.to_owned(),
            purpose: self.purpose,
            project_id: self.project_id.clone(),
            session_id: self.session_id.clone(),
            binary_revision: self.binary_revision.clone(),
        });
        let launch_id = match record {
            Ok(record) => Some(record.launch_id),
            Err(error) => {
                eprintln!("Hot Sheet process journal unavailable: {}", error.kind());
                None
            }
        };
        Ok(TrackedChild {
            child,
            journal: self.journal.clone(),
            launch_id,
            killed: false,
        })
    }

    pub fn status(&self, command: &mut Command) -> io::Result<ExitStatus> {
        self.spawn(command)?.wait()
    }

    pub fn output(&self, command: &mut Command) -> io::Result<Output> {
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        self.spawn(command)?.wait_with_output()
    }

    /// The caller intentionally hands ownership to another process or the OS. The launch
    /// remains unresolved because this supervisor has not observed an exit.
    pub fn detached(&self, command: &mut Command) -> io::Result<()> {
        self.spawn(command).map(|_| ())
    }
}

/// A child whose observed exit updates its journal record. Dropping a still-running child
/// deliberately leaves an unresolved record for later diagnostics.
pub struct TrackedChild {
    child: Child,
    journal: ProcessJournal,
    launch_id: Option<String>,
    killed: bool,
}

impl TrackedChild {
    fn finish(&mut self, status: ExitStatus) {
        if let Some(id) = self.launch_id.take() {
            let reason = if self.killed {
                ExitReason::Killed
            } else if status.success() {
                ExitReason::Completed
            } else {
                ExitReason::Failed
            };
            let _ = self.journal.finish(&id, reason);
        }
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.child.wait()?;
        self.finish(status);
        Ok(status)
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if let Some(status) = status {
            self.finish(status);
        }
        Ok(status)
    }

    pub fn wait_with_output(self) -> io::Result<Output> {
        let Self {
            child,
            journal,
            launch_id,
            killed,
        } = self;
        let output = child.wait_with_output()?;
        if let Some(id) = launch_id {
            let reason = if killed {
                ExitReason::Killed
            } else if output.status.success() {
                ExitReason::Completed
            } else {
                ExitReason::Failed
            };
            let _ = journal.finish(&id, reason);
        }
        Ok(output)
    }

    pub fn kill(&mut self) -> io::Result<()> {
        self.child.kill()?;
        self.killed = true;
        Ok(())
    }
}

impl std::ops::Deref for TrackedChild {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        &self.child
    }
}

impl std::ops::DerefMut for TrackedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessRecord {
    pub launch_id: String,
    pub pid: u32,
    pub supervisor_pid: u32,
    pub supervisor: String,
    pub purpose: Purpose,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub binary_revision: Option<String>,
    pub started_ms: u64,
    pub ended_ms: Option<u64>,
    pub exit_reason: Option<ExitReason>,
}

/// A directory explicitly rooted in the caller's machine home, so tests never touch a user's
/// diagnostic store. Different supervisors can write distinct launch IDs concurrently.
#[derive(Clone, Debug)]
pub struct ProcessJournal {
    dir: PathBuf,
}

impl ProcessJournal {
    pub fn at(machine_home: impl AsRef<Path>) -> Self {
        Self {
            dir: machine_home.as_ref().join("processes"),
        }
    }

    pub fn start(&self, meta: LaunchMeta) -> io::Result<ProcessRecord> {
        self.prepare_dir()?;
        let record = ProcessRecord {
            launch_id: ulid::Ulid::new().to_string(),
            pid: meta.pid,
            supervisor_pid: meta.supervisor_pid,
            supervisor: bounded_id(&meta.supervisor)?,
            purpose: meta.purpose,
            project_id: meta.project_id.as_deref().map(bounded_id).transpose()?,
            session_id: meta.session_id.as_deref().map(bounded_id).transpose()?,
            binary_revision: meta
                .binary_revision
                .as_deref()
                .map(bounded_id)
                .transpose()?,
            started_ms: now_ms(),
            ended_ms: None,
            exit_reason: None,
        };
        let mut temp = tempfile::NamedTempFile::new_in(&self.dir)?;
        serde_json::to_writer(&mut temp, &record)?;
        temp.flush()?;
        temp.as_file().sync_all()?;
        temp.persist_noclobber(self.path(&record.launch_id))
            .map_err(|e| e.error)?;
        if self.prune().is_err() {
            eprintln!("Hot Sheet process journal retention needs repair");
        }
        Ok(record)
    }

    /// Only the launch's owner should finish it. A missing record is an error, so a stale
    /// supervisor cannot silently overwrite a pruned or replaced launch.
    pub fn finish(&self, launch_id: &str, reason: ExitReason) -> io::Result<ProcessRecord> {
        ulid::Ulid::from_string(launch_id)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid launch ID"))?;
        let path = self.path(launch_id);
        let mut record = read_record(&path)?;
        if record.launch_id != launch_id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "launch ID mismatch",
            ));
        }
        if record.ended_ms.is_some() {
            return Ok(record);
        }
        record.ended_ms = Some(now_ms());
        record.exit_reason = Some(reason);
        let mut temp = tempfile::NamedTempFile::new_in(&self.dir)?;
        serde_json::to_writer(&mut temp, &record)?;
        temp.flush()?;
        temp.as_file().sync_all()?;
        temp.persist(path).map_err(|e| e.error)?;
        Ok(record)
    }

    /// Read recent records in start order. An unended record after its supervisor disappears
    /// is evidence for investigation, not proof that the child exited.
    pub fn list(&self) -> io::Result<Vec<ProcessRecord>> {
        self.reject_non_directory()?;
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut records = Vec::new();
        for entry in entries {
            let entry = entry?;
            if entry.path().extension().is_some_and(|ext| ext == "json") {
                records.push(read_record(&entry.path())?);
            }
        }
        records.sort_by_key(|r| (r.started_ms, r.launch_id.clone()));
        Ok(records)
    }

    /// Retain the most recent 1024 ended and 1024 unresolved launches for up to 30 days.
    /// Every start performs this sweep; no background timer or network request is needed.
    pub fn prune(&self) -> io::Result<()> {
        self.reject_non_directory()?;
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let mut records = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                match read_record(&path) {
                    Ok(record) => records.push(record),
                    Err(_) => {
                        // A damaged record cannot establish any child identity. Remove it so
                        // future starts remain bounded; the read-only inventory reports the
                        // corruption until the next successful launch performs this sweep.
                        fs::remove_file(path)?;
                    }
                }
            } else if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".tmp"))
                && fs::symlink_metadata(&path)?.file_type().is_file()
                && fs::metadata(&path)?
                    .modified()?
                    .elapsed()
                    .is_ok_and(|age| age.as_secs() > STALE_TEMP_SECS)
            {
                // A crash can leave an atomic-write temporary file. Never remove a
                // concurrent writer's fresh file; sweep abandoned ones on later starts.
                fs::remove_file(path)?;
            }
        }
        records.sort_by_key(|r| (std::cmp::Reverse(r.started_ms), r.launch_id.clone()));
        let mut ended = 0;
        let mut unresolved = 0;
        let cutoff = now_ms().saturating_sub(RETAIN_MS);
        for record in records {
            let count = if record.ended_ms.is_some() {
                ended += 1;
                ended
            } else {
                unresolved += 1;
                unresolved
            };
            let cap = if record.ended_ms.is_some() {
                MAX_ENDED
            } else {
                MAX_UNRESOLVED
            };
            if count > cap || record.started_ms < cutoff {
                match fs::remove_file(self.path(&record.launch_id)) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(())
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    fn prepare_dir(&self) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        self.reject_non_directory()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    fn reject_non_directory(&self) -> io::Result<()> {
        match fs::symlink_metadata(&self.dir) {
            Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "process journal path is not a directory",
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

fn bounded_id(value: &str) -> io::Result<String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid diagnostic identifier",
        ));
    }
    Ok(value.to_owned())
}

fn read_record(path: &Path) -> io::Result<ProcessRecord> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "process record is not a file",
        ));
    }
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > MAX_RECORD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "oversized process record",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "oversized process record",
        ));
    }
    let record: ProcessRecord = serde_json::from_slice(&bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    ulid::Ulid::from_string(&record.launch_id)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid launch ID"))?;
    for value in [
        Some(record.supervisor.as_str()),
        record.project_id.as_deref(),
        record.session_id.as_deref(),
        record.binary_revision.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        bounded_id(value).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid diagnostic identifier")
        })?;
    }
    if record.ended_ms.is_some() != record.exit_reason.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "incomplete exit record",
        ));
    }
    if !path
        .file_stem()
        .is_some_and(|stem| stem == record.launch_id.as_str())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "launch file mismatch",
        ));
    }
    Ok(record)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn hs2_g9tf60_tracked_commands_record_observed_and_detached_lifecycle() {
        let home = tempfile::tempdir().unwrap();
        let context = LaunchContext {
            journal: ProcessJournal::at(home.path()),
            supervisor: "hotsheet-test",
            purpose: Purpose::RepositoryHelper,
            project_id: Some("project-1".into()),
            session_id: None,
            binary_revision: Some("test-revision".into()),
        };
        let mut success = Command::new("sh");
        success.args(["-c", "printf synthetic"]);
        assert_eq!(context.output(&mut success).unwrap().stdout, b"synthetic");
        let mut failure = Command::new("sh");
        failure.args(["-c", "exit 7"]);
        assert_eq!(context.status(&mut failure).unwrap().code(), Some(7));
        let mut detached = Command::new("sh");
        detached.args(["-c", "exit 0"]);
        context.detached(&mut detached).unwrap();
        let mut sleeping = Command::new("sleep");
        sleeping.arg("30");
        let mut killed = context.spawn(&mut sleeping).unwrap();
        killed.kill().unwrap();
        killed.wait().unwrap();

        let records = context.journal.list().unwrap();
        assert_eq!(records.len(), 4);
        for reason in [
            Some(ExitReason::Completed),
            Some(ExitReason::Failed),
            None,
            Some(ExitReason::Killed),
        ] {
            assert_eq!(
                records
                    .iter()
                    .filter(|record| record.exit_reason == reason)
                    .count(),
                1
            );
        }
        assert!(records.iter().all(|record| {
            record.project_id.as_deref() == Some("project-1")
                && record.binary_revision.as_deref() == Some("test-revision")
        }));
        let serialized = serde_json::to_string(&records).unwrap();
        assert!(!serialized.contains("synthetic"));
        assert!(!serialized.contains("exit 7"));
    }

    fn meta(pid: u32) -> LaunchMeta {
        LaunchMeta {
            pid,
            supervisor_pid: 7,
            supervisor: "test-host".into(),
            purpose: Purpose::AiChannel,
            project_id: Some("project-1".into()),
            session_id: Some("session-1".into()),
            binary_revision: Some("rev-1".into()),
        }
    }

    #[test]
    fn hs2_gdcmxe_launch_exit_restart_and_unresolved_investigation() {
        let home = tempfile::tempdir().unwrap();
        let journal = ProcessJournal::at(home.path());
        let first = journal.start(meta(42)).unwrap();
        assert_eq!(journal.list().unwrap(), vec![first.clone()]);
        let ended = journal
            .finish(&first.launch_id, ExitReason::Completed)
            .unwrap();
        assert!(ended.ended_ms.is_some());
        assert_eq!(ended.exit_reason, Some(ExitReason::Completed));
        assert_eq!(
            journal
                .finish(&first.launch_id, ExitReason::Killed)
                .unwrap(),
            ended
        );
        let restarted = journal.start(meta(42)).unwrap();
        assert_ne!(restarted.launch_id, first.launch_id);
        assert_eq!(restarted.pid, first.pid);
        let recovered = ProcessJournal::at(home.path()).list().unwrap();
        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[1].exit_reason, None);
        assert_eq!(recovered[1].session_id.as_deref(), Some("session-1"));
    }

    #[test]
    fn hs2_gdcmxe_rejects_content_and_corrupt_records() {
        let home = tempfile::tempdir().unwrap();
        let journal = ProcessJournal::at(home.path());
        let mut unsafe_meta = meta(1);
        unsafe_meta.session_id = Some("prompt text with spaces".into());
        assert_eq!(
            journal.start(unsafe_meta).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        let record = journal.start(meta(2)).unwrap();
        fs::write(journal.path(&record.launch_id), b"not-json").unwrap();
        assert_eq!(
            journal.list().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::write(home.path().join("processes/.tmp-interrupted"), b"partial").unwrap();
        let recovered = journal.start(meta(3)).unwrap();
        assert_eq!(journal.list().unwrap(), vec![recovered]);
    }

    #[test]
    fn hs2_gdcmxe_prunes_old_and_excess_records() {
        let home = tempfile::tempdir().unwrap();
        let journal = ProcessJournal::at(home.path());
        journal.prepare_dir().unwrap();
        for pid in 0..(MAX_ENDED as u32 + 3) {
            let record = ProcessRecord {
                launch_id: ulid::Ulid::new().to_string(),
                pid,
                supervisor_pid: 7,
                supervisor: "test-host".into(),
                purpose: Purpose::AiChannel,
                project_id: None,
                session_id: None,
                binary_revision: None,
                started_ms: now_ms() + u64::from(pid),
                ended_ms: Some(now_ms() + u64::from(pid)),
                exit_reason: Some(ExitReason::Completed),
            };
            fs::write(
                journal.path(&record.launch_id),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
        }
        journal.prune().unwrap();
        assert_eq!(journal.list().unwrap().len(), MAX_ENDED);
    }

    #[cfg(unix)]
    #[test]
    fn hs2_gdcmxe_rejects_a_redirected_journal_directory() {
        use std::os::unix::fs::symlink;

        let home = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        symlink(elsewhere.path(), home.path().join("processes")).unwrap();
        let journal = ProcessJournal::at(home.path());
        assert_eq!(
            journal.start(meta(4)).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            journal.list().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
