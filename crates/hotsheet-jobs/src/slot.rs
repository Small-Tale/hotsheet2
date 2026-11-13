//! The machine's single Job monitor slot (HS2-5TC1GQ). One monitor process serves every
//! watch Job under a machine home. The slot is an advisory lock the monitor holds for its
//! whole life, so the OS frees it when the process dies, however it dies.
//!
//! Stopping is cooperative: a stop request is a small file addressed to one monitor's
//! owner id. No PID is ever signalled, so a recycled PID cannot be hit.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::lock::DirLock;

const LOCK_FILE: &str = ".monitor.lock";
const INFO_FILE: &str = ".monitor.json";
const STOP_FILE: &str = ".monitor.stop";
const MAX_INFO_BYTES: u64 = 4096;

/// What a running monitor publishes about itself. The PID is diagnostic only.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MonitorInfo {
    /// The lease owner id this monitor uses on every Job, unique per process.
    pub owner: String,
    pub pid: u32,
    pub started_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SlotState {
    /// No monitor is running.
    Free,
    /// A monitor holds the slot. `None` only in the instant before it published itself.
    Held(Option<MonitorInfo>),
}

/// The monitor slot of one machine home.
#[derive(Clone, Debug)]
pub struct MonitorSlot {
    dir: PathBuf,
}

/// Held by the running monitor. Dropping it frees the slot.
#[derive(Debug)]
pub struct MonitorClaim {
    slot: MonitorSlot,
    owner: String,
    _lock: DirLock,
}

impl MonitorSlot {
    pub fn at(machine_home: impl AsRef<Path>) -> Self {
        Self {
            dir: machine_home.as_ref().join("jobs"),
        }
    }

    /// The directory a monitor watches for new Jobs and requests.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Become the machine's monitor. `Ok(None)` means another monitor already is.
    pub fn try_claim(&self, info: &MonitorInfo) -> io::Result<Option<MonitorClaim>> {
        let Some(lock) = DirLock::try_acquire(self.lock_file()?)? else {
            return Ok(None);
        };
        // A request left for an earlier monitor must not stop this one.
        remove_if_present(&self.dir.join(STOP_FILE))?;
        let bytes = serde_json::to_vec(info)?;
        let mut temp = tempfile::NamedTempFile::new_in(&self.dir)?;
        temp.write_all(&bytes)?;
        temp.flush()?;
        temp.persist(self.dir.join(INFO_FILE))
            .map_err(|error| error.error)?;
        Ok(Some(MonitorClaim {
            slot: self.clone(),
            owner: info.owner.clone(),
            _lock: lock,
        }))
    }

    /// Whether a monitor is running. Never blocks and never disturbs a running monitor.
    pub fn state(&self) -> io::Result<SlotState> {
        if !self.dir.is_dir() {
            return Ok(SlotState::Free);
        }
        match DirLock::try_acquire(self.lock_file()?)? {
            Some(_free) => Ok(SlotState::Free),
            None => Ok(SlotState::Held(self.read_info())),
        }
    }

    /// Ask the monitor with this owner id to stop. Another monitor ignores the request.
    pub fn request_stop(&self, owner: &str) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let mut temp = tempfile::NamedTempFile::new_in(&self.dir)?;
        temp.write_all(owner.as_bytes())?;
        temp.flush()?;
        temp.persist(self.dir.join(STOP_FILE))
            .map_err(|error| error.error)?;
        Ok(())
    }

    fn stop_requested_for(&self, owner: &str) -> bool {
        fs::read_to_string(self.dir.join(STOP_FILE)).is_ok_and(|text| text.trim() == owner)
    }

    fn lock_file(&self) -> io::Result<fs::File> {
        if !self.dir.is_dir() {
            fs::create_dir_all(&self.dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))?;
            }
        }
        crate::lock::open_lock_file(&self.dir.join(LOCK_FILE))
    }

    fn read_info(&self) -> Option<MonitorInfo> {
        let file = fs::File::open(self.dir.join(INFO_FILE)).ok()?;
        if file.metadata().ok()?.len() > MAX_INFO_BYTES {
            return None;
        }
        serde_json::from_reader(file).ok()
    }
}

impl MonitorClaim {
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// Whether a stop request addressed to this monitor is waiting.
    pub fn stop_requested(&self) -> bool {
        self.slot.stop_requested_for(&self.owner)
    }
}

impl Drop for MonitorClaim {
    fn drop(&mut self) {
        // Still under the lock: nobody else can have published into the slot.
        let _ = remove_if_present(&self.slot.dir.join(INFO_FILE));
        let _ = remove_if_present(&self.slot.dir.join(STOP_FILE));
    }
}

/// Make sure the machine's monitor runs: report the running one, or call `spawn` (which
/// starts a detached `hotsheet-cli job monitor`) and wait until it has published itself.
/// The flag says whether this call started it. Safe to race: every caller ends up naming
/// the one monitor that holds the slot.
pub fn ensure_monitor_running(
    slot: &MonitorSlot,
    spawn: impl FnOnce() -> io::Result<()>,
    timeout: std::time::Duration,
) -> io::Result<(MonitorInfo, bool)> {
    if let SlotState::Held(Some(info)) = slot.state()? {
        return Ok((info, false));
    }
    let started = slot.state()? == SlotState::Free;
    if started {
        spawn()?;
    }
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let SlotState::Held(Some(info)) = slot.state()? {
            return Ok((info, started));
        }
        if std::time::Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the Job monitor did not start",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(owner: &str) -> MonitorInfo {
        MonitorInfo {
            owner: owner.into(),
            pid: 7,
            started_ms: 1,
        }
    }

    #[test]
    fn only_one_claim_is_held_and_dropping_it_frees_the_slot() {
        let home = tempfile::tempdir().unwrap();
        let slot = MonitorSlot::at(home.path());
        assert_eq!(slot.state().unwrap(), SlotState::Free);
        assert!(
            !home.path().join("jobs").exists(),
            "a status read creates nothing"
        );

        let first = slot.try_claim(&info("monitor-a")).unwrap().unwrap();
        assert!(slot.try_claim(&info("monitor-b")).unwrap().is_none());
        assert_eq!(
            slot.state().unwrap(),
            SlotState::Held(Some(info("monitor-a")))
        );
        assert_eq!(first.owner(), "monitor-a");

        drop(first);
        assert_eq!(slot.state().unwrap(), SlotState::Free);
        assert!(!slot.dir().join(INFO_FILE).exists());
        let second = slot.try_claim(&info("monitor-b")).unwrap().unwrap();
        assert_eq!(
            slot.state().unwrap(),
            SlotState::Held(Some(info("monitor-b")))
        );
        drop(second);
    }

    #[test]
    fn a_stop_request_reaches_only_the_monitor_it_names() {
        let home = tempfile::tempdir().unwrap();
        let slot = MonitorSlot::at(home.path());
        let claim = slot.try_claim(&info("monitor-a")).unwrap().unwrap();
        assert!(!claim.stop_requested());
        slot.request_stop("monitor-z").unwrap();
        assert!(!claim.stop_requested());
        slot.request_stop("monitor-a").unwrap();
        assert!(claim.stop_requested());
        // Repeating the request changes nothing.
        slot.request_stop("monitor-a").unwrap();
        assert!(claim.stop_requested());
        drop(claim);
        assert!(!slot.dir().join(STOP_FILE).exists());
    }

    #[test]
    fn a_stale_stop_request_does_not_stop_the_next_monitor() {
        let home = tempfile::tempdir().unwrap();
        let slot = MonitorSlot::at(home.path());
        // Left behind by a stop that raced a monitor which had already exited.
        slot.request_stop("monitor-b").unwrap();
        let claim = slot.try_claim(&info("monitor-b")).unwrap().unwrap();
        assert!(!claim.stop_requested());
    }

    #[test]
    fn an_unreadable_info_file_still_reports_the_slot_as_held() {
        let home = tempfile::tempdir().unwrap();
        let slot = MonitorSlot::at(home.path());
        let _claim = slot.try_claim(&info("monitor-a")).unwrap().unwrap();
        fs::write(slot.dir().join(INFO_FILE), b"{").unwrap();
        assert_eq!(slot.state().unwrap(), SlotState::Held(None));
    }
}
