//! Main CI status per checkout (HS2-M8HHEA, `docs/32-notifier-plugins.md` §32.8.5).
//!
//! The server has no CI source of its own and never polls one. The tool that already watches
//! CI (`scripts/ci-gate.mjs status`, an integrator, a user's hook) reports each run it learns
//! about to the local `POST /checkouts/{reference}/ci-status` route, or headlessly with
//! `hotsheet-cli ci report`. The server keeps the latest run per checkout id, persists it
//! machine-locally in `<machine home>/ci-status.json`, and pushes it to glance surfaces as the
//! `ci` member of every linked store's live state ([`crate::live_state`]). No inbound
//! exposure is needed, and any CI that can run a command can report.

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

/// The machine-local file holding the latest run per checkout.
pub const CI_STATUS_FILE: &str = "ci-status.json";

/// The persisted format. A file of another version is ignored rather than misread.
pub const CI_STATUS_VERSION: u32 = 1;

/// Longest accepted `run_url`.
const MAX_URL: usize = 2048;
/// Longest accepted `branch` or `workflow`.
const MAX_LABEL: usize = 255;

/// A CI run's state. `unavailable` appears only in the live state, when no linked checkout
/// has reported; a report never carries it.
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

/// One reported run, stored per checkout. Times are Unix milliseconds.
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

    /// The ordering key used to pick one run among a store's linked checkouts: the start time
    /// when known, then the report time.
    fn recency(&self) -> (i64, i64) {
        (
            self.started_at_ms.unwrap_or(self.reported_at_ms),
            self.reported_at_ms,
        )
    }
}

/// `LiveStoreState.ci`: the newest run reported by any checkout linked to the store, or
/// `status: unavailable` with every other member `null` when none has reported.
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
        Ok(CiRun {
            status,
            branch,
            sha,
            run_url,
            workflow,
            started_at_ms: self.started_at_ms,
            finished_at_ms: self.finished_at_ms,
            reported_at_ms,
        })
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CiStatusFile {
    version: u32,
    #[serde(default)]
    checkouts: BTreeMap<String, CiRun>,
}

/// The latest run per checkout id, loaded lazily from the machine home.
#[derive(Clone, Default)]
pub(crate) struct CiReports {
    runs: Arc<Mutex<Option<BTreeMap<String, CiRun>>>>,
}

pub fn ci_status_path(machine_home: &Path) -> PathBuf {
    machine_home.join(CI_STATUS_FILE)
}

/// The persisted runs, or none when the file is missing, unreadable, or another version.
pub fn load_runs(path: &Path) -> BTreeMap<String, CiRun> {
    let Ok(bytes) = std::fs::read(path) else {
        return BTreeMap::new();
    };
    match serde_json::from_slice::<CiStatusFile>(&bytes) {
        Ok(file) if file.version == CI_STATUS_VERSION => file.checkouts,
        Ok(_) => BTreeMap::new(),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "ignoring an unreadable CI status file");
            BTreeMap::new()
        }
    }
}

/// Write the runs atomically. A failure is logged: the run stays in memory and is pushed.
fn save_runs(path: &Path, runs: &BTreeMap<String, CiRun>) {
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
        std::fs::rename(&temp, path)
    };
    if let Err(error) = write() {
        tracing::warn!(path = %path.display(), %error, "could not save the CI status file");
    }
}

impl CiReports {
    fn with_runs<T>(
        &self,
        machine_home: &Path,
        f: impl FnOnce(&mut BTreeMap<String, CiRun>) -> T,
    ) -> T {
        let mut runs = self.runs.lock_or_recover();
        let runs = runs.get_or_insert_with(|| load_runs(&ci_status_path(machine_home)));
        f(runs)
    }

    /// Record `run` for `checkout` unless it is stale. Returns whether it was applied and
    /// the run now stored.
    pub(crate) fn record(&self, machine_home: &Path, checkout: &str, run: CiRun) -> (bool, CiRun) {
        self.with_runs(machine_home, |runs| {
            if let Some(stored) = runs.get(checkout)
                && !run.supersedes(stored)
            {
                return (false, stored.clone());
            }
            runs.insert(checkout.to_owned(), run.clone());
            save_runs(&ci_status_path(machine_home), runs);
            (true, run)
        })
    }

    /// The newest run among `checkouts`, as the live state's `ci` member.
    pub(crate) fn live(&self, machine_home: &Path, checkouts: &[String]) -> LiveCi {
        self.with_runs(machine_home, |runs| newest(runs, checkouts))
    }
}

/// The newest stored run among `checkouts`; checkouts are visited in id order so ties are
/// stable.
pub fn newest(runs: &BTreeMap<String, CiRun>, checkouts: &[String]) -> LiveCi {
    let mut ids: Vec<&String> = checkouts.iter().collect();
    ids.sort();
    ids.dedup();
    ids.into_iter()
        .filter_map(|id| runs.get(id).map(|run| (id, run)))
        .fold(None::<(&String, &CiRun)>, |best, (id, run)| match best {
            Some((_, current)) if current.recency() >= run.recency() => best,
            _ => Some((id, run)),
        })
        .map_or_else(LiveCi::unavailable, |(id, run)| LiveCi::of(id, run))
}

// ---- route -----------------------------------------------------------------------

/// The response of `POST /checkouts/{reference}/ci-status`.
#[derive(Debug, Serialize)]
pub struct CiReportResponse {
    /// The resolved checkout id.
    pub checkout: String,
    /// False when the report was older than the stored run and left it unchanged.
    pub applied: bool,
    /// The run now stored for this checkout.
    pub ci: LiveCi,
    /// Hosted stores linked to this checkout, whose live state shows the run.
    pub stores: Vec<String>,
}

/// `POST /checkouts/{reference}/ci-status` — record the latest CI run a local tool observed
/// for a registered checkout and push it in the linked stores' live state (HS2-M8HHEA).
pub(crate) async fn report_ci_status(
    State(state): State<AppState>,
    RoutePath(reference): RoutePath<String>,
    Json(report): Json<CiReport>,
) -> Result<Json<CiReportResponse>, ApiError> {
    let checkout = state
        .checkout_registry
        .describe(&reference)
        .map_err(crate::routes::checkouts::checkout_lookup_error)?;
    let now = (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64;
    let run = report
        .into_run(now)
        .map_err(|message| ApiError::new(StatusCode::BAD_REQUEST, message))?;
    let (applied, stored) = state
        .ci_reports
        .record(state.machine_home(), &checkout.id, run);
    let mut stores: Vec<String> = state
        .host
        .locations()
        .into_iter()
        .filter(|(_, root)| {
            crate::notification_router::linked(&state, root)
                .iter()
                .any(|linked| linked.id == checkout.id)
        })
        .map(|(id, _)| id)
        .collect();
    stores.sort();
    if applied {
        for store in &stores {
            crate::live_state::touch(&state, store);
        }
    }
    Ok(Json(CiReportResponse {
        ci: LiveCi::of(&checkout.id, &stored),
        checkout: checkout.id,
        applied,
        stores,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(status: &str) -> CiReport {
        CiReport {
            status: status.into(),
            ..CiReport::default()
        }
    }

    fn run(status: CiStatus, started: Option<i64>, reported: i64) -> CiRun {
        CiRun {
            status,
            branch: Some("main".into()),
            sha: Some("abcdef1".into()),
            run_url: None,
            workflow: Some("CI".into()),
            started_at_ms: started,
            finished_at_ms: None,
            reported_at_ms: reported,
        }
    }

    #[test]
    fn hs2_m8hhea_report_validation_accepts_a_full_run_and_normalizes_it() {
        let full = CiReport {
            status: " success ".into(),
            branch: Some(" main ".into()),
            sha: Some("ABCDEF0123456789".into()),
            run_url: Some("https://github.com/o/r/actions/runs/1".into()),
            workflow: Some("CI".into()),
            started_at_ms: Some(10),
            finished_at_ms: Some(20),
        };
        let run = full.into_run(30).unwrap();
        assert_eq!(run.status, CiStatus::Success);
        assert_eq!(run.branch.as_deref(), Some("main"));
        assert_eq!(run.sha.as_deref(), Some("abcdef0123456789"));
        assert_eq!(run.reported_at_ms, 30);
        let minimal = report("queued").into_run(1).unwrap();
        assert_eq!(minimal.status, CiStatus::Queued);
        assert_eq!(minimal.sha, None);
    }

    #[test]
    fn hs2_m8hhea_report_validation_rejects_bad_fields() {
        let cases: Vec<(CiReport, &str)> = vec![
            (report("unavailable"), "status must be"),
            (report("passed"), "status must be"),
            (
                CiReport {
                    sha: Some("xyz1234".into()),
                    ..report("success")
                },
                "sha must be",
            ),
            (
                CiReport {
                    sha: Some("abc".into()),
                    ..report("success")
                },
                "sha must be",
            ),
            (
                CiReport {
                    run_url: Some("file:///etc/passwd".into()),
                    ..report("success")
                },
                "run_url must be",
            ),
            (
                CiReport {
                    run_url: Some("https://x/ y".into()),
                    ..report("success")
                },
                "run_url must be",
            ),
            (
                CiReport {
                    branch: Some(String::new()),
                    ..report("success")
                },
                "branch must be",
            ),
            (
                CiReport {
                    workflow: Some("a\nb".into()),
                    ..report("success")
                },
                "workflow must not",
            ),
            (
                CiReport {
                    started_at_ms: Some(-1),
                    ..report("queued")
                },
                "must not be negative",
            ),
            (
                CiReport {
                    finished_at_ms: Some(5),
                    ..report("in_progress")
                },
                "only valid for",
            ),
            (
                CiReport {
                    started_at_ms: Some(10),
                    finished_at_ms: Some(5),
                    ..report("failure")
                },
                "before started_at_ms",
            ),
        ];
        for (case, expected) in cases {
            let error = case.clone().into_run(1).unwrap_err();
            assert!(error.contains(expected), "{case:?}: {error}");
        }
    }

    #[test]
    fn hs2_m8hhea_staleness_keeps_the_newest_run_and_terminal_states() {
        let stored = run(CiStatus::Success, Some(100), 200);
        // An older run is stale; a newer one supersedes.
        assert!(!run(CiStatus::Failure, Some(99), 300).supersedes(&stored));
        assert!(run(CiStatus::Queued, Some(101), 300).supersedes(&stored));
        // The same run going back to in progress after success is stale...
        assert!(!run(CiStatus::InProgress, Some(100), 300).supersedes(&stored));
        // ...but a terminal correction of the same run applies (a re-run's conclusion).
        assert!(run(CiStatus::Failure, Some(100), 300).supersedes(&stored));
        // Same start but another commit is another run.
        let mut other = run(CiStatus::InProgress, Some(100), 300);
        other.sha = Some("1234567".into());
        assert!(other.supersedes(&stored));
        // Without both start times, the later report wins.
        assert!(run(CiStatus::Queued, None, 300).supersedes(&stored));
        assert!(run(CiStatus::Queued, Some(1), 300).supersedes(&run(CiStatus::Success, None, 1)));
        // A queued run moving to in progress then success applies in order.
        let queued = run(CiStatus::Queued, Some(500), 1);
        let running = run(CiStatus::InProgress, Some(500), 2);
        let done = run(CiStatus::Success, Some(500), 3);
        assert!(running.supersedes(&queued));
        assert!(done.supersedes(&running));
        assert!(!running.supersedes(&done));
    }

    #[test]
    fn hs2_m8hhea_newest_picks_the_latest_run_among_linked_checkouts() {
        let mut runs = BTreeMap::new();
        assert_eq!(newest(&runs, &["a".into()]), LiveCi::unavailable());
        runs.insert("a".to_owned(), run(CiStatus::Success, Some(100), 500));
        runs.insert("b".to_owned(), run(CiStatus::Failure, Some(200), 400));
        runs.insert("c".to_owned(), run(CiStatus::Queued, Some(900), 900));
        // c is not linked, so it never shows.
        let live = newest(&runs, &["a".into(), "b".into()]);
        assert_eq!(live.status, CiStatus::Failure);
        assert_eq!(live.checkout.as_deref(), Some("b"));
        assert_eq!(live.reported_at_ms, Some(400));
        // Order of the linked list does not matter; duplicates count once.
        assert_eq!(newest(&runs, &["b".into(), "a".into(), "b".into()]), live);
        // An unlinked checkout list reports unavailable explicitly.
        let none = newest(&runs, &[]);
        assert_eq!(none.status, CiStatus::Unavailable);
        assert_eq!(
            serde_json::to_value(&none).unwrap(),
            serde_json::json!({"status": "unavailable", "checkout": null, "branch": null,
                "sha": null, "run_url": null, "workflow": null, "started_at_ms": null,
                "finished_at_ms": null, "reported_at_ms": null})
        );
    }

    #[test]
    fn hs2_m8hhea_reports_persist_and_bad_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let reports = CiReports::default();
        assert_eq!(
            reports.live(&home, &["a".into()]).status,
            CiStatus::Unavailable
        );
        let (applied, _) = reports.record(&home, "a", run(CiStatus::InProgress, Some(10), 1));
        assert!(applied);
        let (applied, stored) = reports.record(&home, "a", run(CiStatus::Queued, Some(5), 2));
        assert!(!applied, "an older run is stale");
        assert_eq!(stored.status, CiStatus::InProgress);
        let (applied, _) = reports.record(&home, "a", run(CiStatus::Success, Some(10), 3));
        assert!(applied);

        // A fresh registry (a restarted server) reads the persisted run.
        let restarted = CiReports::default();
        let live = restarted.live(&home, &["a".into()]);
        assert_eq!(live.status, CiStatus::Success);
        assert_eq!(live.reported_at_ms, Some(3));

        // An unreadable file or another version starts empty.
        let path = ci_status_path(&home);
        std::fs::write(&path, b"{not json").unwrap();
        assert!(load_runs(&path).is_empty());
        std::fs::write(&path, br#"{"version": 99, "checkouts": {}}"#).unwrap();
        assert!(load_runs(&path).is_empty());
        assert!(load_runs(&dir.path().join("missing.json")).is_empty());
    }
}
