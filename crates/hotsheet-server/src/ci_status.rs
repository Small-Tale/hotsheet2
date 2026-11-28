//! Main CI status per checkout (HS2-M8HHEA, `docs/32-notifier-plugins.md` §32.8.5).
//!
//! The server has no CI source of its own and never polls one. The tool that already watches
//! CI (`scripts/ci-gate.mjs status`, an integrator, a user's hook) reports each run it learns
//! about to the local `POST /checkouts/{reference}/ci-status` route, or headlessly with
//! `hotsheet-cli ci report`. The server keeps the latest run per checkout id, branch, and
//! workflow (HS2-D3JJ49), persists them machine-locally in `<machine home>/ci-status.json`,
//! and pushes them to glance surfaces as the `ci` and `ci_runs` members of every linked
//! store's live state ([`crate::live_state`]). No inbound exposure is needed, and any CI that
//! can run a command can report. A reported GitHub Actions run still in flight is followed to
//! its end by the Job monitor (HS2-CC7TCB, [`crate::ci_follow`]).
//!
//! HS2-D3JJ49: every stored run carries a [`Verification`]. A run that nothing can verify
//! any more is never shown; a provider that is only unreachable for now keeps the run shown
//! with its last known status ([`Verification::Transient`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::{Path as RoutePath, State};
use axum::http::StatusCode;
use hotsheet_sync::LockExt;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::AppState;
use crate::api_error::ApiError;

/// The machine-local file holding the latest runs per checkout.
pub const CI_STATUS_FILE: &str = "ci-status.json";

/// The persisted format. Version 1 (one run per checkout) is migrated on load; a file of
/// any other version is ignored rather than misread.
pub const CI_STATUS_VERSION: u32 = 2;
/// HS2-D3JJ49: the version 1 format, one run per checkout.
const LEGACY_VERSION: u32 = 1;

/// The most runs kept per checkout, one per branch and workflow; the oldest is dropped.
pub const MAX_RUNS_PER_CHECKOUT: usize = 8;
/// The most runs a store's live state lists in `ci_runs`.
pub const MAX_LIVE_RUNS: usize = 8;

/// Longest accepted `run_url`.
const MAX_URL: usize = 2048;
/// Longest accepted `branch` or `workflow`.
const MAX_LABEL: usize = 255;

/// A CI run's state. `unavailable` appears only in the live state, when no linked checkout
/// has reported a run that can be shown; a report never carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CiStatus {
    Unavailable,
    Queued,
    InProgress,
    Success,
    Failure,
    Cancelled,
}

impl CiStatus {
    /// Whether the run finished.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            CiStatus::Success | CiStatus::Failure | CiStatus::Cancelled
        )
    }

    fn parse_reported(text: &str) -> Result<CiStatus, String> {
        match text {
            "queued" => Ok(CiStatus::Queued),
            "in_progress" => Ok(CiStatus::InProgress),
            "success" => Ok(CiStatus::Success),
            "failure" => Ok(CiStatus::Failure),
            "cancelled" => Ok(CiStatus::Cancelled),
            other => Err(format!(
                "status must be queued, in_progress, success, failure, or cancelled, not '{other}'"
            )),
        }
    }
}

/// HS2-D3JJ49: whether a stored run's state is still confirmed by its provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    /// The watch observed the run at its provider, or the run is finished.
    Verified,
    /// The last check failed for a network error, a server error, or a rate limit. The last
    /// known state stays shown while the Job monitor retries with its bounded backoff.
    Transient,
    /// No watch has confirmed the active run, or it can no longer be confirmed. Never shown.
    #[default]
    Unverifiable,
}

/// HS2-D3JJ49: what happened to a stored run's verification. The one transition function
/// is [`CiRun::verify`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationEvent {
    /// A watch observed the run at the provider.
    Confirmed,
    /// Hot Sheet has no watcher for this run, such as a disabled monitor or missing token.
    Unfollowed,
    /// The watch's last provider read failed for now, or its monitor was lost.
    Unreachable,
    /// A restart re-checks an older run: hidden until the watch confirms it.
    Recheck,
    /// The watch ended without naming a finished run, the follow window closed, or the
    /// re-check found nothing.
    Unsettled,
}

/// One reported run, stored per checkout, branch, and workflow. Times are Unix milliseconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CiRun {
    pub status: CiStatus,
    pub branch: Option<String>,
    pub sha: Option<String>,
    pub run_url: Option<String>,
    pub workflow: Option<String>,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    /// When the server accepted the report.
    pub reported_at_ms: i64,
    /// HS2-D3JJ49: whether the provider still confirms the state.
    #[serde(default)]
    pub verification: Verification,
    /// Legacy v2 marker: an active row with a report-only deadline has no watch evidence.
    #[serde(default)]
    pub verify_by_ms: Option<i64>,
}

impl CiRun {
    /// Whether this run is newer than `stored`, so it replaces it. A run that started before
    /// the stored one is stale, and so is a non-terminal report of the same run after a
    /// terminal one (a late `in_progress` after `success`). Without both start times, the
    /// later report wins.
    pub fn supersedes(&self, stored: &CiRun) -> bool {
        match (self.started_at_ms, stored.started_at_ms) {
            (Some(new), Some(old)) if new < old => false,
            (Some(new), Some(old)) if new == old => {
                let same_run = self.sha == stored.sha && self.workflow == stored.workflow;
                !(same_run && stored.status.is_terminal() && !self.status.is_terminal())
            }
            _ => true,
        }
    }

    /// The ordering key among runs: the start time when known, then the report time.
    fn recency(&self) -> (i64, i64) {
        (
            self.started_at_ms.unwrap_or(self.reported_at_ms),
            self.reported_at_ms,
        )
    }

    /// HS2-D3JJ49: the run's branch and workflow. One run is kept per pair and checkout.
    pub fn key(&self) -> (Option<&str>, Option<&str>) {
        (self.branch.as_deref(), self.workflow.as_deref())
    }

    /// An old v2 report-only row has no watch evidence, even before its old deadline.
    pub fn verification(&self) -> Verification {
        if self.status.is_terminal() {
            Verification::Verified
        } else if self.verify_by_ms.is_some() {
            Verification::Unverifiable
        } else {
            self.verification
        }
    }

    /// Whether glance surfaces show the run.
    pub fn visible(&self) -> bool {
        self.verification() != Verification::Unverifiable
    }

    /// HS2-D3JJ49: apply `event` and return whether the stored verification changed. A
    /// finished run is verified for good and ignores every event.
    pub fn verify(&mut self, event: VerificationEvent) -> bool {
        if self.status.is_terminal() {
            return false;
        }
        let next = match event {
            VerificationEvent::Confirmed => (Verification::Verified, None),
            // A first read or re-check waiting on its first answer stays hidden.
            VerificationEvent::Unreachable if !self.visible() => {
                return false;
            }
            VerificationEvent::Unreachable => (Verification::Transient, None),
            VerificationEvent::Unfollowed
            | VerificationEvent::Recheck
            | VerificationEvent::Unsettled => (Verification::Unverifiable, None),
        };
        let changed = (self.verification, self.verify_by_ms) != next;
        (self.verification, self.verify_by_ms) = next;
        changed
    }

    /// Whether `self` is the stored form of the followed run `followed`: the same branch,
    /// workflow, commit, and reported run URL, still in flight.
    fn same_flight(&self, followed: &CiRun) -> bool {
        !self.status.is_terminal()
            && self.key() == followed.key()
            && self.sha == followed.sha
            && self.run_url == followed.run_url
    }
}

/// `LiveStoreState.ci`: the newest shown run reported by any checkout linked to the store,
/// or `status: unavailable` with every other member `null` when none has reported or none
/// can be verified. Each entry of `LiveStoreState.ci_runs` has the same shape (HS2-D3JJ49).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveCi {
    pub status: CiStatus,
    /// The checkout id that reported the run.
    pub checkout: Option<String>,
    pub branch: Option<String>,
    pub sha: Option<String>,
    pub run_url: Option<String>,
    pub workflow: Option<String>,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    pub reported_at_ms: Option<i64>,
    /// HS2-D3JJ49: `verified` or `transient` for a shown run; null while unavailable.
    #[serde(default)]
    pub verification: Option<Verification>,
    /// Legacy report-only deadline from older servers. The current server emits null.
    #[serde(default)]
    pub verify_by_ms: Option<i64>,
}

impl LiveCi {
    pub fn unavailable() -> LiveCi {
        LiveCi {
            status: CiStatus::Unavailable,
            checkout: None,
            branch: None,
            sha: None,
            run_url: None,
            workflow: None,
            started_at_ms: None,
            finished_at_ms: None,
            reported_at_ms: None,
            verification: None,
            verify_by_ms: None,
        }
    }

    fn of(checkout: &str, run: &CiRun) -> LiveCi {
        LiveCi {
            status: run.status,
            checkout: Some(checkout.to_owned()),
            branch: run.branch.clone(),
            sha: run.sha.clone(),
            run_url: run.run_url.clone(),
            workflow: run.workflow.clone(),
            started_at_ms: run.started_at_ms,
            finished_at_ms: run.finished_at_ms,
            reported_at_ms: Some(run.reported_at_ms),
            verification: Some(run.verification()),
            verify_by_ms: None,
        }
    }
}

/// The body of `POST /checkouts/{reference}/ci-status`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CiReport {
    pub status: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub sha: Option<String>,
    #[serde(default)]
    pub run_url: Option<String>,
    #[serde(default)]
    pub workflow: Option<String>,
    #[serde(default)]
    pub started_at_ms: Option<i64>,
    #[serde(default)]
    pub finished_at_ms: Option<i64>,
}

fn label(name: &str, value: Option<String>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim().to_owned();
    if value.is_empty() || value.chars().count() > MAX_LABEL {
        return Err(format!("{name} must be 1 to {MAX_LABEL} characters"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{name} must not contain control characters"));
    }
    Ok(Some(value))
}

impl CiReport {
    /// Validate and normalize the report into a stored run.
    pub fn into_run(self, reported_at_ms: i64) -> Result<CiRun, String> {
        let status = CiStatus::parse_reported(self.status.trim())?;
        let sha = match self.sha {
            None => None,
            Some(sha) => {
                let sha = sha.trim().to_ascii_lowercase();
                if !(7..=64).contains(&sha.len()) || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err("sha must be 7 to 64 hexadecimal characters".into());
                }
                Some(sha)
            }
        };
        let run_url = match self.run_url {
            None => None,
            Some(url) => {
                let url = url.trim().to_owned();
                let scheme = url.starts_with("https://") || url.starts_with("http://");
                if !scheme
                    || url.len() > MAX_URL
                    || url.chars().any(|c| c.is_whitespace() || c.is_control())
                {
                    return Err(format!(
                        "run_url must be an http or https URL of at most {MAX_URL} characters"
                    ));
                }
                Some(url)
            }
        };
        let branch = label("branch", self.branch)?;
        let workflow = label("workflow", self.workflow)?;
        for (name, at) in [
            ("started_at_ms", self.started_at_ms),
            ("finished_at_ms", self.finished_at_ms),
        ] {
            if at.is_some_and(|at| at < 0) {
                return Err(format!("{name} must not be negative"));
            }
        }
        if self.finished_at_ms.is_some() && !status.is_terminal() {
            return Err("finished_at_ms is only valid for success, failure, or cancelled".into());
        }
        if let (Some(started), Some(finished)) = (self.started_at_ms, self.finished_at_ms)
            && finished < started
        {
            return Err("finished_at_ms must not be before started_at_ms".into());
        }
        let run = CiRun {
            status,
            branch,
            sha,
            run_url,
            workflow,
            started_at_ms: self.started_at_ms,
            finished_at_ms: self.finished_at_ms,
            reported_at_ms,
            verification: if status.is_terminal() {
                Verification::Verified
            } else {
                Verification::Unverifiable
            },
            verify_by_ms: None,
        };
        Ok(run)
    }
}

/// HS2-D3JJ49: record `run` among one checkout's runs. It replaces the stored run of the
/// same branch and workflow when it supersedes it, and is stale otherwise; a new pair is
/// added, dropping the oldest run past [`MAX_RUNS_PER_CHECKOUT`]. Returns whether it applied
/// and the run now stored for the pair.
pub fn record_run(runs: &mut Vec<CiRun>, mut run: CiRun) -> (bool, CiRun) {
    if let Some(stored) = runs.iter_mut().find(|stored| stored.key() == run.key()) {
        if !run.supersedes(stored) {
            return (false, stored.clone());
        }
        // A fresh report of the same flight must not discard the watch's last observation.
        if run.same_flight(stored) && !stored.status.is_terminal() && stored.run_url.is_some() {
            run.verification = stored.verification();
        }
        *stored = run.clone();
        return (true, run);
    }
    runs.push(run.clone());
    if runs.len() > MAX_RUNS_PER_CHECKOUT {
        let oldest = runs
            .iter()
            .enumerate()
            .min_by_key(|(_, stored)| stored.recency())
            .map(|(index, _)| index);
        if let Some(index) = oldest {
            runs.remove(index);
        }
    }
    (true, run)
}

/// HS2-D3JJ49: which stored runs `DELETE /checkouts/{reference}/ci-status` clears. Every
/// given member must match; an empty filter matches every run of the checkout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiClearQuery {
    /// Clear only a run reported at this time (the run the caller saw), so a newer report of
    /// the same branch and workflow is kept.
    #[serde(default)]
    pub reported_at_ms: Option<i64>,
    /// Clear only runs of this branch.
    #[serde(default)]
    pub branch: Option<String>,
    /// Clear only runs of this workflow.
    #[serde(default)]
    pub workflow: Option<String>,
}

impl CiClearQuery {
    fn matches(&self, run: &CiRun) -> bool {
        self.reported_at_ms
            .is_none_or(|at| run.reported_at_ms == at)
            && self
                .branch
                .as_deref()
                .is_none_or(|branch| run.branch.as_deref() == Some(branch))
            && self
                .workflow
                .as_deref()
                .is_none_or(|workflow| run.workflow.as_deref() == Some(workflow))
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CiStatusFile {
    version: u32,
    #[serde(default)]
    checkouts: BTreeMap<String, Vec<CiRun>>,
}

/// HS2-D3JJ49: the version 1 file, one run per checkout.
#[derive(Debug, Deserialize)]
struct LegacyCiStatusFile {
    #[serde(default)]
    checkouts: BTreeMap<String, CiRun>,
}

/// The stored runs by checkout id.
pub type CiRunsByCheckout = BTreeMap<String, Vec<CiRun>>;

/// The latest runs per checkout id, loaded lazily from the machine home.
#[derive(Clone, Default)]
pub(crate) struct CiReports {
    runs: Arc<Mutex<Option<CiRunsByCheckout>>>,
}

pub fn ci_status_path(machine_home: &Path) -> PathBuf {
    machine_home.join(CI_STATUS_FILE)
}

/// Version 2 keeps watch evidence, and [`CiRun::verification`] hides its legacy report-only
/// rows. Version 1 predates watches' saved evidence, so its active runs load hidden. Any other
/// version, or a file that is not JSON, holds none.
pub fn parse_runs(bytes: &[u8]) -> Result<CiRunsByCheckout, String> {
    #[derive(Deserialize)]
    struct Version {
        version: u32,
    }
    let version = serde_json::from_slice::<Version>(bytes)
        .map_err(|error| error.to_string())?
        .version;
    match version {
        CI_STATUS_VERSION => serde_json::from_slice::<CiStatusFile>(bytes)
            .map(|file| file.checkouts)
            .map_err(|error| error.to_string()),
        LEGACY_VERSION => {
            let legacy = serde_json::from_slice::<LegacyCiStatusFile>(bytes)
                .map_err(|error| error.to_string())?;
            Ok(legacy
                .checkouts
                .into_iter()
                .map(|(checkout, mut run)| {
                    run.verify(VerificationEvent::Unfollowed);
                    (checkout, vec![run])
                })
                .collect())
        }
        _ => Ok(BTreeMap::new()),
    }
}

/// The persisted runs, or none when the file is missing, unreadable, or another version.
pub fn load_runs(path: &Path) -> CiRunsByCheckout {
    let Ok(bytes) = std::fs::read(path) else {
        return BTreeMap::new();
    };
    parse_runs(&bytes).unwrap_or_else(|error| {
        tracing::warn!(path = %path.display(), %error, "ignoring an unreadable CI status file");
        BTreeMap::new()
    })
}

/// Write the runs atomically. A failure is logged: the runs stay in memory and are pushed.
fn save_runs(path: &Path, runs: &CiRunsByCheckout) {
    let write = || -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = CiStatusFile {
            version: CI_STATUS_VERSION,
            checkouts: runs.clone(),
        };
        let temp = path.with_extension("json.tmp");
        std::fs::write(
            &temp,
            serde_json::to_vec_pretty(&file).map_err(std::io::Error::other)?,
        )?;
        hotsheet_sync::files::replace(&temp, path)
    };
    if let Err(error) = write() {
        tracing::warn!(path = %path.display(), %error, "could not save the CI status file");
    }
}

fn now_ms() -> i64 {
    (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

impl CiReports {
    pub(crate) fn with_runs<T>(
        &self,
        machine_home: &Path,
        f: impl FnOnce(&mut CiRunsByCheckout) -> T,
    ) -> T {
        let mut runs = self.runs.lock_or_recover();
        let runs = runs.get_or_insert_with(|| load_runs(&ci_status_path(machine_home)));
        f(runs)
    }

    /// Store a report unless a newer report already superseded it.
    pub(crate) fn record(&self, machine_home: &Path, checkout: &str, run: CiRun) -> (bool, CiRun) {
        self.record_matching(machine_home, checkout, run, None)
    }

    /// Run a short action under the status lock only while the exact flight is still current.
    pub(crate) fn with_current_flight<T>(
        &self,
        machine_home: &Path,
        checkout: &str,
        followed: &CiRun,
        action: impl FnOnce() -> T,
    ) -> Option<T> {
        self.with_runs(machine_home, |runs| {
            Self::contains_flight(runs, checkout, followed).then(action)
        })
    }

    fn contains_flight(runs: &CiRunsByCheckout, checkout: &str, followed: &CiRun) -> bool {
        runs.get(checkout)
            .is_some_and(|stored| stored.iter().any(|run| run.same_flight(followed)))
    }

    /// A watch verdict applies only while the exact followed flight is still stored.
    fn record_matching(
        &self,
        machine_home: &Path,
        checkout: &str,
        run: CiRun,
        followed: Option<&CiRun>,
    ) -> (bool, CiRun) {
        self.with_runs(machine_home, |runs| {
            if let Some(followed) = followed
                && !Self::contains_flight(runs, checkout, followed)
            {
                return (false, run);
            }
            let (applied, stored) = record_run(runs.entry(checkout.to_owned()).or_default(), run);
            if applied {
                save_runs(&ci_status_path(machine_home), runs);
            }
            (applied, stored)
        })
    }

    /// HS2-D3JJ49: apply `event` to the stored run of `checkout` that `followed` stands for,
    /// while it is still that run in flight. Returns whether its verification changed.
    pub(crate) fn verify(
        &self,
        machine_home: &Path,
        checkout: &str,
        followed: &CiRun,
        event: VerificationEvent,
    ) -> bool {
        self.with_runs(machine_home, |runs| {
            let changed = runs
                .get_mut(checkout)
                .and_then(|runs| runs.iter_mut().find(|run| run.same_flight(followed)))
                .is_some_and(|run| run.verify(event));
            if changed {
                save_runs(&ci_status_path(machine_home), runs);
            }
            changed
        })
    }

    /// HS2-7PSBHQ, HS2-D3JJ49: remove the stored runs of `checkout` that `query` matches, so
    /// a run reported after the caller looked is never cleared unseen. Returns whether any
    /// run was removed and the runs left stored.
    pub(crate) fn clear(
        &self,
        machine_home: &Path,
        checkout: &str,
        query: &CiClearQuery,
    ) -> (bool, Vec<CiRun>) {
        self.with_runs(machine_home, |runs| {
            let Some(stored) = runs.get_mut(checkout) else {
                return (false, Vec::new());
            };
            let before = stored.len();
            stored.retain(|run| !query.matches(run));
            let cleared = stored.len() != before;
            let left = stored.clone();
            if left.is_empty() {
                runs.remove(checkout);
            }
            if cleared {
                save_runs(&ci_status_path(machine_home), runs);
            }
            (cleared, left)
        })
    }

    /// The live state's `ci` and `ci_runs` among `checkouts`.
    pub(crate) fn live(&self, machine_home: &Path, checkouts: &[String]) -> (LiveCi, Vec<LiveCi>) {
        self.with_runs(machine_home, |runs| {
            let shown = live_runs(runs, checkouts);
            (
                shown.first().cloned().unwrap_or_else(LiveCi::unavailable),
                shown,
            )
        })
    }
}

/// HS2-D3JJ49: the runs a store's live state shows among its linked `checkouts`:
/// only visible ones, the newest per branch and workflow across checkouts, newest first, at
/// most [`MAX_LIVE_RUNS`]. Checkouts are visited in id order so ties are stable.
pub fn live_runs(runs: &CiRunsByCheckout, checkouts: &[String]) -> Vec<LiveCi> {
    let mut ids: Vec<&String> = checkouts.iter().collect();
    ids.sort();
    ids.dedup();
    let mut newest: Vec<(&String, &CiRun)> = Vec::new();
    for (id, run) in ids.into_iter().flat_map(|id| {
        runs.get(id)
            .into_iter()
            .flatten()
            .filter(|run| run.visible())
            .map(move |run| (id, run))
    }) {
        match newest.iter_mut().find(|(_, kept)| kept.key() == run.key()) {
            Some(kept) if kept.1.recency() >= run.recency() => {}
            Some(kept) => *kept = (id, run),
            None => newest.push((id, run)),
        }
    }
    newest.sort_by(|a, b| b.1.recency().cmp(&a.1.recency()).then(a.0.cmp(b.0)));
    newest
        .into_iter()
        .take(MAX_LIVE_RUNS)
        .map(|(id, run)| LiveCi::of(id, run))
        .collect()
}

/// The newest shown run among `checkouts`, or `unavailable`.
pub fn newest(runs: &CiRunsByCheckout, checkouts: &[String]) -> LiveCi {
    live_runs(runs, checkouts)
        .into_iter()
        .next()
        .unwrap_or_else(LiveCi::unavailable)
}

// ---- route -----------------------------------------------------------------------

/// The response of `POST /checkouts/{reference}/ci-status`.
#[derive(Debug, Serialize)]
pub struct CiReportResponse {
    /// The resolved checkout id.
    pub checkout: String,
    /// False when the report was older than the stored run and left it unchanged.
    pub applied: bool,
    /// The run now stored for this checkout, branch, and workflow.
    pub ci: LiveCi,
    /// Hosted stores linked to this checkout, whose live state shows the run.
    pub stores: Vec<String>,
    /// HS2-CC7TCB: whether a GitHub Actions watch follows the run to its end.
    pub follow: crate::ci_follow::FollowState,
}

/// Record a followed run's verdict and rebuild linked stores only if that run is still current.
pub(crate) fn apply_verdict(state: &AppState, checkout: &str, followed: &CiRun, run: CiRun) {
    if state
        .ci_reports
        .record_matching(state.machine_home(), checkout, run, Some(followed))
        .0
    {
        for store in linked_stores(state, checkout) {
            crate::live_state::touch(state, &store);
        }
    }
}

/// HS2-D3JJ49: apply `event` to the stored run `followed` of `checkout` and, when its
/// verification changed, push the linked stores' live state again. Returns whether it
/// changed.
pub(crate) fn apply_verification(
    state: &AppState,
    checkout: &str,
    followed: &CiRun,
    event: VerificationEvent,
) -> bool {
    let changed = state
        .ci_reports
        .verify(state.machine_home(), checkout, followed, event);
    if changed {
        for store in linked_stores(state, checkout) {
            crate::live_state::touch(state, &store);
        }
    }
    changed
}

/// The hosted stores linked to `checkout`, whose live state shows its runs.
fn linked_stores(state: &AppState, checkout: &str) -> Vec<String> {
    let mut stores: Vec<String> = state
        .host
        .locations()
        .into_iter()
        .filter(|(_, root)| {
            crate::notification_router::linked(state, root)
                .iter()
                .any(|linked| linked.id == checkout)
        })
        .map(|(id, _)| id)
        .collect();
    stores.sort();
    stores
}

impl AppState {
    /// Every stored run by checkout id (HS2-CC7TCB: a restarted server follows the ones
    /// still in flight).
    pub(crate) fn ci_reports_snapshot(&self) -> CiRunsByCheckout {
        self.ci_reports
            .with_runs(self.machine_home(), |runs| runs.clone())
    }
}

/// `POST /checkouts/{reference}/ci-status` — record the latest CI run a local tool observed
/// for a registered checkout and push it in the linked stores' live state (HS2-M8HHEA).
/// An applied report of a GitHub Actions run still in flight is followed to its end
/// (HS2-CC7TCB, [`crate::ci_follow`]).
pub(crate) async fn report_ci_status(
    State(state): State<AppState>,
    RoutePath(reference): RoutePath<String>,
    Json(report): Json<CiReport>,
) -> Result<Json<CiReportResponse>, ApiError> {
    let checkout = state
        .checkout_registry
        .describe(&reference)
        .map_err(crate::routes::checkouts::checkout_lookup_error)?;
    let now = now_ms();
    let run = report
        .into_run(now)
        .map_err(|message| ApiError::new(StatusCode::BAD_REQUEST, message))?;
    let checkout_id = checkout.id.clone();
    let (applied, stored, stores, follow) = tokio::task::spawn_blocking(move || {
        let (applied, mut stored) =
            state
                .ci_reports
                .record(state.machine_home(), &checkout_id, run);
        let stores = linked_stores(&state, &checkout_id);
        if !applied {
            return (applied, stored, stores, crate::ci_follow::FollowState::None);
        }
        let follow = crate::ci_follow::follow_reported(&state, &checkout_id, &stored);
        // Only a provider observation confirms an active run. If this refresh preserved a
        // previous observation, but there's no watcher now, hide it before notifying clients.
        if !follow.is_following() && stored.verify(VerificationEvent::Unfollowed) {
            state.ci_reports.verify(
                state.machine_home(),
                &checkout_id,
                &stored,
                VerificationEvent::Unfollowed,
            );
        }
        for store in &stores {
            crate::live_state::touch(&state, store);
        }
        (applied, stored, stores, follow)
    })
    .await
    .map_err(|_| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the CI report did not finish",
        )
    })?;
    Ok(Json(CiReportResponse {
        ci: LiveCi::of(&checkout.id, &stored),
        checkout: checkout.id,
        applied,
        stores,
        follow,
    }))
}

/// The response of `DELETE /checkouts/{reference}/ci-status`.
#[derive(Debug, Serialize)]
pub struct CiClearResponse {
    /// The resolved checkout id.
    pub checkout: String,
    /// False when nothing stored matched the query.
    pub cleared: bool,
    /// The newest run still stored for this checkout, or `unavailable`.
    pub ci: LiveCi,
    /// HS2-D3JJ49: every run still stored for this checkout, newest first, shown or not.
    pub runs: Vec<LiveCi>,
    /// Hosted stores linked to this checkout.
    pub stores: Vec<String>,
}

/// `DELETE /checkouts/{reference}/ci-status` — forget CI runs stored for a checkout
/// (HS2-7PSBHQ, HS2-D3JJ49): every run, or only those matching `reported_at_ms`, `branch`,
/// and `workflow`. Idempotent: clearing nothing answers `cleared: false`. A cleared run's
/// stores push their live state again. The provider keeps the run itself; the next report
/// stores it again.
pub(crate) async fn clear_ci_status(
    State(state): State<AppState>,
    RoutePath(reference): RoutePath<String>,
    axum::extract::Query(query): axum::extract::Query<CiClearQuery>,
) -> Result<Json<CiClearResponse>, ApiError> {
    let checkout = state
        .checkout_registry
        .describe(&reference)
        .map_err(crate::routes::checkouts::checkout_lookup_error)?;
    let checkout_id = checkout.id.clone();
    let (cleared, left, stores) = tokio::task::spawn_blocking(move || {
        let (cleared, left) = state
            .ci_reports
            .clear(state.machine_home(), &checkout_id, &query);
        let stores = linked_stores(&state, &checkout_id);
        if cleared {
            for store in &stores {
                crate::live_state::touch(&state, store);
            }
        }
        (cleared, left, stores)
    })
    .await
    .map_err(|_| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "clearing the CI status did not finish",
        )
    })?;
    let mut runs: Vec<LiveCi> = left
        .iter()
        .map(|run| LiveCi::of(&checkout.id, run))
        .collect();
    runs.sort_by(|a, b| {
        let at = |ci: &LiveCi| (ci.started_at_ms.or(ci.reported_at_ms), ci.reported_at_ms);
        at(b).cmp(&at(a))
    });
    Ok(Json(CiClearResponse {
        ci: runs.first().cloned().unwrap_or_else(LiveCi::unavailable),
        checkout: checkout.id,
        cleared,
        runs,
        stores,
    }))
}

#[cfg(test)]
#[path = "ci_status_tests.rs"]
mod tests;
