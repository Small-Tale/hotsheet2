//! Follow a reported GitHub Actions run to its end (HS2-CC7TCB,
//! `docs/32-notifier-plugins.md` §32.8.5).
//!
//! A reporter such as `scripts/ci-gate.mjs status` reports a run once. When it reports a
//! run that is still queued or running, nothing used to report its end, so the main CI
//! chip counted "running" for hours after the run failed. When such a report names a
//! GitHub Actions run, the server now hands the run to the existing GitHub Actions watch
//! Job (HS2-3R9V0R) and makes sure the machine's Job monitor runs (HS2-VVHVF0). The
//! monitor polls GitHub with its conditional requests and rate-limit backoff; the server
//! itself still never polls. A follower thread waits on the Job store's file changes,
//! and when the watch reaches a verdict it records the finished run through the same
//! record-and-push path as a reported one ([`crate::ci_status::apply_run`]).
//!
//! Nothing is followed for another CI's URL, for a run without a full commit, branch,
//! and workflow, for a run this server already follows, or when no GitHub token is
//! available. The client's stale-report display (`clients/web/src/ci-staleness.ts`)
//! covers those cases, and the user can clear such a run (HS2-7PSBHQ).
//!
//! HS2-7PSBHQ: a restarted server re-checks once a stored run still in flight that was
//! reported before the follow window ([`recheck_stale`]), so a run whose follower never
//! started (an older server, no token then, a server stopped for the whole run) settles
//! instead of staying unknown for good.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hotsheet_cli::orchestration::{DetachedJobMonitor, JobMonitor};
use hotsheet_jobs::{
    CiTarget, CiWatchOptions, Job, JobStore, Status, ensure_ci_watch, find_ci_watch, gate_of,
    gate_state, rewatch_ci, wait_for_gate_change,
};
use hotsheet_sync::LockExt;
use serde::Serialize;

use crate::AppState;
use crate::ci_status::{CiReport, CiRun};

/// How long after its report a run is followed. GitHub Actions cancels a job after 6 h
/// by default (`timeout-minutes: 360`), so a run still unfinished then is not worth
/// polling for: the follower asks the watch to stop.
pub const FOLLOW_LIMIT_MS: i64 = 6 * 60 * 60 * 1000;
/// HS2-7PSBHQ: how long a restarted server re-checks a run reported before the follow
/// window, which nothing ended. The watch's first observations settle a run that finished
/// long ago, so this bounds one round of provider queries, not a new follow.
pub const RECHECK_LIMIT_MS: i64 = 15 * 60 * 1000;
/// The longest single wait, so a stopping server's followers end within it.
const WAIT_SLICE: Duration = Duration::from_secs(60);
/// How long a token check is remembered. Each `gh` check is a child launch.
const TOKEN_RECHECK_MS: i64 = 15 * 60 * 1000;
/// Overrides the GitHub API root for GitHub Enterprise Server, as for the watch.
const API_URL_ENV: &str = "HOTSHEET_GITHUB_API_URL";

/// What a report started, in `POST /checkouts/{reference}/ci-status`'s `follow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowState {
    /// A watch now follows the run to its end.
    Started,
    /// This server already follows the run; nothing new was started.
    Following,
    /// Nothing to follow: a finished or stale report, or a run that is not a GitHub
    /// Actions run with a full commit, branch, and workflow.
    None,
    /// A GitHub run, but no GitHub token is available to watch it.
    NoToken,
    /// This server starts no Job monitor (test builds, `HOTSHEET_JOB_MONITOR_AUTOSTART=off`).
    Off,
}

/// Whether a GitHub token is available to the Job monitor.
pub trait TokenProbe: Send + Sync {
    fn available(&self, home: &Path) -> bool;
}

/// The shipped [`TokenProbe`]: `GH_TOKEN` or `GITHUB_TOKEN`, otherwise whether
/// `gh auth token` succeeds, the same sources the watch's HTTP client reads. The token
/// itself is discarded unread; the answer is remembered for 15 minutes.
#[derive(Default)]
pub struct GhTokenProbe {
    checked: Mutex<Option<(bool, i64)>>,
}

impl TokenProbe for GhTokenProbe {
    fn available(&self, home: &Path) -> bool {
        let in_env = ["GH_TOKEN", "GITHUB_TOKEN"]
            .into_iter()
            .filter_map(|name| std::env::var(name).ok())
            .any(|value| !value.trim().is_empty());
        if in_env {
            return true;
        }
        let now = now_ms();
        let mut checked = self.checked.lock_or_recover();
        if let Some((answer, at)) = *checked
            && now - at < TOKEN_RECHECK_MS
        {
            return answer;
        }
        let mut command = hotsheet_ticketing::git::launch("gh");
        command
            .args(["auth", "token"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let answer = hotsheet_process_journal::LaunchContext {
            journal: hotsheet_process_journal::ProcessJournal::at(home),
            supervisor: "hotsheet-server",
            purpose: hotsheet_process_journal::Purpose::CliExternal,
            project_id: None,
            session_id: None,
            binary_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
        }
        .output(&mut command)
        .is_ok_and(|output| output.status.success());
        *checked = Some((answer, now));
        answer
    }
}

/// The followers of this server: the Job monitor it starts, the token check, and the
/// runs it follows now.
#[derive(Clone)]
pub struct CiFollow {
    monitor: Option<Arc<dyn JobMonitor>>,
    token: Arc<dyn TokenProbe>,
    following: Arc<Mutex<BTreeSet<String>>>,
}

impl Default for CiFollow {
    /// The monitor that a server starts on its own, or none where autostart is off, as
    /// it is for every test build (HS2-VVHVF0).
    fn default() -> Self {
        Self {
            monitor: DetachedJobMonitor::automatic()
                .map(|monitor| Arc::new(monitor) as Arc<dyn JobMonitor>),
            token: Arc::new(GhTokenProbe::default()),
            following: Arc::default(),
        }
    }
}

impl CiFollow {
    /// Followers that start `monitor` and check for a token with `token`.
    pub fn new(monitor: Option<Arc<dyn JobMonitor>>, token: Arc<dyn TokenProbe>) -> Self {
        Self {
            monitor,
            token,
            following: Arc::default(),
        }
    }

    /// How many runs this server follows now.
    pub fn following(&self) -> usize {
        self.following.lock_or_recover().len()
    }
}

fn now_ms() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

/// The host of a configured GitHub Enterprise Server API root, such as `ghe.example`
/// from `https://ghe.example/api/v3`.
fn enterprise_host(api_root: Option<&str>) -> Option<String> {
    let rest = api_root?.trim().strip_prefix("https://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// `owner/name` of the repository a GitHub Actions run page belongs to:
/// `https://github.com/<owner>/<name>/actions/runs/<id>[/…]`, or the same on the host of
/// the configured GitHub Enterprise Server API root. Any other URL is not followed.
pub fn github_run_repo(url: &str, api_root: Option<&str>) -> Option<String> {
    let rest = url.trim().strip_prefix("https://")?;
    let rest = rest.split(['?', '#']).next()?;
    let mut parts = rest.split('/');
    let host = parts.next()?.to_ascii_lowercase();
    if host != "github.com" && Some(host) != enterprise_host(api_root) {
        return None;
    }
    let (owner, name) = (parts.next()?, parts.next()?);
    let (actions, runs, id) = (parts.next()?, parts.next()?, parts.next()?);
    let id_ok = !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit());
    let named = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    };
    (actions == "actions" && runs == "runs" && id_ok && named(owner) && named(name))
        .then(|| format!("{owner}/{name}"))
}

/// The CI gate the watch follows for a reported run: only a run that is still queued or
/// running, on a GitHub Actions run page, with a full 40-character commit, a branch,
/// and a workflow. Without all of them there is nothing exact to follow.
pub fn follow_target(run: &CiRun, api_root: Option<&str>) -> Option<CiTarget> {
    if run.status.is_terminal() {
        return None;
    }
    let repo = github_run_repo(run.run_url.as_deref()?, api_root)?;
    let sha = run.sha.as_deref().filter(|sha| sha.len() == 40)?;
    CiTarget::new(&repo, sha, run.workflow.as_deref()?, run.branch.as_deref()?).ok()
}

/// The finished run a watch Job's verdict stands for, as a report: green is `success`,
/// red `failure`, and a cancelled covering run `cancelled`. A superseded commit, a lost
/// observer, an unknown commit, and a stopped watch name no run, so nothing is reported.
pub fn verdict_report(job: &Job) -> Option<CiReport> {
    let refs = &job.outcome.as_ref()?.refs;
    let status = match (job.status, refs.get("gate").map(String::as_str)) {
        (Status::Succeeded, Some("green")) => "success",
        (Status::Failed, Some("red")) => "failure",
        (Status::Cancelled, Some("cancelled")) => "cancelled",
        _ => return None,
    };
    let number = |key: &str| refs.get(key).and_then(|text| text.parse::<i64>().ok());
    let started = number("run_created_ms");
    let finished =
        number("run_updated_ms").filter(|finished| started.is_none_or(|s| *finished >= s));
    Some(CiReport {
        status: status.to_owned(),
        branch: refs.get("branch").cloned(),
        sha: Some(refs.get("run_head_sha")?.clone()),
        run_url: refs.get("run_url").cloned(),
        workflow: refs.get("workflow").cloned(),
        started_at_ms: started,
        finished_at_ms: finished,
    })
}

/// The verdict of the followed run's own commit keeps the start its reporter gave when
/// that is later than the run's `created_at` (a reporter that counts from a re-run's
/// start), so the finished run is not refused as older than the report it ends.
pub fn aligned_start(mut report: CiReport, followed: &CiRun) -> CiReport {
    let same_commit = report
        .sha
        .as_deref()
        .zip(followed.sha.as_deref())
        .is_some_and(|(verdict, reported)| verdict.eq_ignore_ascii_case(reported));
    if let Some(start) = followed.started_at_ms
        && same_commit
        && report.started_at_ms.is_none_or(|verdict| verdict < start)
    {
        report.started_at_ms = Some(start);
        report.finished_at_ms = report.finished_at_ms.filter(|finished| *finished >= start);
    }
    report
}

fn follow_key(checkout: &str, target: &CiTarget) -> String {
    format!(
        "{checkout}\n{}\n{}\n{}\n{}",
        target.repo, target.sha, target.workflow, target.branch
    )
}

/// The watch Job for `target`, or the verdict a retained one already reached.
enum Watch {
    /// Wait on this Job.
    Follow(Job),
    /// HS2-7PSBHQ: a re-check found a retained watch with a final verdict.
    Final(Job),
}

/// The watch Job to follow for `target`: the retained one while it still waits, a new
/// one after an earlier verdict that is not green (a re-run of the commit), or none when
/// the commit already passed. A re-check (`recheck`, HS2-7PSBHQ) of a run reported long
/// ago instead takes any final verdict the retained watch reached: that run is over, and
/// a re-watch would wait for a re-run that never comes.
fn watch_job(store: &JobStore, checkout: &str, target: &CiTarget, recheck: bool) -> Option<Watch> {
    let options = CiWatchOptions::default();
    let job = match find_ci_watch(store, checkout, target).ok()? {
        Some(job) if recheck && gate_of(&job).is_final() => return Some(Watch::Final(job)),
        Some(job) if job.status.is_finished() && job.status != Status::Lost => {
            rewatch_ci(store, checkout, target, &options).ok()?
        }
        Some(job) => job,
        None => ensure_ci_watch(store, checkout, target, &options).ok()?,
    };
    if !gate_of(&job).is_final() {
        Some(Watch::Follow(job))
    } else if recheck {
        Some(Watch::Final(job))
    } else {
        None
    }
}

/// Record the verdict of a finished watch `job` as the end of `followed`. Returns whether
/// the watch named a finished run.
fn record_verdict(state: &AppState, checkout: &str, job: &Job, followed: &CiRun) -> bool {
    let Some(report) = verdict_report(job).map(|report| aligned_start(report, followed)) else {
        return false;
    };
    let Ok(run) = report.into_run(now_ms()) else {
        return false;
    };
    crate::ci_status::apply_run(state, checkout, run);
    true
}

/// HS2-CC7TCB: after an applied report of `run` for `checkout`, follow it when it is a
/// GitHub Actions run still in flight. Blocking: it may start the Job monitor and run
/// `gh`. Returns what it did; a started follower runs on its own thread.
pub(crate) fn follow_reported(state: &AppState, checkout: &str, run: &CiRun) -> FollowState {
    follow(
        state,
        checkout,
        run,
        run.reported_at_ms.saturating_add(FOLLOW_LIMIT_MS),
        false,
    )
}

/// HS2-7PSBHQ: re-check once a stored run still in flight that was reported before the
/// follow window: record the verdict a retained watch already reached, or watch the run
/// again for [`RECHECK_LIMIT_MS`] so the monitor's first observations settle it.
pub(crate) fn recheck_stale(state: &AppState, checkout: &str, run: &CiRun) -> FollowState {
    follow(
        state,
        checkout,
        run,
        now_ms().saturating_add(RECHECK_LIMIT_MS),
        true,
    )
}

fn follow(
    state: &AppState,
    checkout: &str,
    run: &CiRun,
    deadline: i64,
    recheck: bool,
) -> FollowState {
    let api_root = std::env::var(API_URL_ENV).ok();
    let Some(target) = follow_target(run, api_root.as_deref()) else {
        return FollowState::None;
    };
    let follow = state.ci_follow();
    let Some(monitor) = follow.monitor.clone() else {
        return FollowState::Off;
    };
    let key = follow_key(checkout, &target);
    if follow.following.lock_or_recover().contains(&key) {
        return FollowState::Following;
    }
    let home = state.machine_home().to_path_buf();
    if !follow.token.available(&home) {
        return FollowState::NoToken;
    }
    let store = JobStore::at(&home);
    let job = match watch_job(&store, checkout, &target, recheck) {
        None => return FollowState::None,
        // Only a re-check takes a final verdict; nothing is left to follow either way.
        Some(Watch::Final(job)) => {
            record_verdict(state, checkout, &job, run);
            return FollowState::None;
        }
        Some(Watch::Follow(job)) => job,
    };
    if !follow.following.lock_or_recover().insert(key.clone()) {
        return FollowState::Following;
    }
    if monitor.ensure(&home).is_err() {
        tracing::warn!(
            checkout,
            "could not start the Job monitor to follow a CI run"
        );
    }
    let (state, checkout, followed) = (state.clone(), checkout.to_owned(), run.clone());
    let spawned = std::thread::Builder::new().name("ci-follow".into()).spawn({
        let key = key.clone();
        move || {
            follow_until_verdict(
                &state, &store, &checkout, &target, &job.id, &followed, deadline,
            );
            state.ci_follow().following.lock_or_recover().remove(&key);
        }
    });
    if spawned.is_err() {
        follow.following.lock_or_recover().remove(&key);
        return FollowState::None;
    }
    FollowState::Started
}

/// Wait on the Job store's file changes until the watch reaches a verdict, the watch
/// ends without one, the server stops, or `deadline` passes (then stop watching).
fn follow_until_verdict(
    state: &AppState,
    store: &JobStore,
    checkout: &str,
    target: &CiTarget,
    job_id: &str,
    followed: &CiRun,
    deadline: i64,
) {
    let Ok(mut reading) = gate_state(store, checkout, target) else {
        return;
    };
    loop {
        if reading.state.is_final() {
            if let Some(job) = find_ci_watch(store, checkout, target).ok().flatten() {
                record_verdict(state, checkout, &job, followed);
            }
            return;
        }
        if reading.ended || reading.job_id.is_none() || state.is_stopping() {
            return;
        }
        let left = deadline.saturating_sub(now_ms());
        if left <= 0 {
            let id = reading.job_id.as_deref().unwrap_or(job_id);
            let _ = store.stop_watching(checkout, id);
            return;
        }
        let slice = Duration::from_millis(u64::try_from(left).unwrap_or(0)).min(WAIT_SLICE);
        match wait_for_gate_change(store, checkout, target, &reading.state, slice) {
            Ok(next) => reading = next,
            Err(_) => return,
        }
    }
}

/// HS2-CC7TCB: a restarted server follows the stored runs that are still in flight and
/// were reported within [`FOLLOW_LIMIT_MS`]. HS2-7PSBHQ: one reported before that, which
/// nothing ended, is re-checked once ([`recheck_stale`]) instead of staying unknown.
/// Idle when there are none.
pub(crate) fn resume(state: &AppState) {
    let now = now_ms();
    for (checkout, run) in state.ci_reports_snapshot() {
        if run.status.is_terminal() {
            continue;
        }
        if now - run.reported_at_ms < FOLLOW_LIMIT_MS {
            follow_reported(state, &checkout, &run);
        } else {
            recheck_stale(state, &checkout, &run);
        }
    }
}

impl AppState {
    /// Follow, on a blocking thread, the stored CI runs that a previous run of this server
    /// left in flight (HS2-CC7TCB).
    pub fn spawn_ci_follow_recovery(&self) {
        let state = self.clone();
        std::thread::spawn(move || resume(&state));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_status::CiStatus;
    use hotsheet_jobs::{
        Capabilities, FeedState, JobKind, Outcome, ProducerFamily, RECORD_VERSION,
    };
    use std::collections::BTreeMap;

    const SHA: &str = "8f6c8d81f00000000000000000000000000000aa";
    const URL: &str = "https://github.com/acme/widgets/actions/runs/2983";

    fn run(status: CiStatus) -> CiRun {
        CiRun {
            status,
            branch: Some("main".into()),
            sha: Some(SHA.into()),
            run_url: Some(URL.into()),
            workflow: Some("CI".into()),
            started_at_ms: Some(1_000),
            finished_at_ms: None,
            reported_at_ms: 2_000,
        }
    }

    #[test]
    fn hs2_cc7tcb_only_github_actions_run_pages_are_followed() {
        for url in [
            URL,
            "https://github.com/acme/widgets/actions/runs/2983/attempts/2",
            "https://GitHub.com/acme/my_repo.js/actions/runs/1?check_suite_focus=true",
        ] {
            assert!(github_run_repo(url, None).is_some(), "{url}");
        }
        assert_eq!(github_run_repo(URL, None).as_deref(), Some("acme/widgets"));
        for url in [
            "http://github.com/acme/widgets/actions/runs/1",
            "https://gitlab.com/acme/widgets/-/pipelines/1",
            "https://github.com/acme/widgets/pull/1",
            "https://github.com/acme/widgets/actions/runs/",
            "https://github.com/acme/widgets/actions/runs/12x",
            "https://github.com.evil.test/acme/widgets/actions/runs/1",
            "https://ci.example.test/acme/widgets/actions/runs/1",
            "https://github.com/acme",
        ] {
            assert_eq!(github_run_repo(url, None), None, "{url}");
        }
        // GitHub Enterprise Server: the host of the configured API root, HTTPS only.
        let ghe = "https://ghe.example.test/acme/widgets/actions/runs/5";
        assert_eq!(github_run_repo(ghe, None), None);
        assert_eq!(
            github_run_repo(ghe, Some("https://ghe.example.test/api/v3")).as_deref(),
            Some("acme/widgets")
        );
        assert_eq!(
            github_run_repo(ghe, Some("http://ghe.example.test/api/v3")),
            None
        );
    }

    #[test]
    fn hs2_cc7tcb_a_target_needs_a_run_in_flight_with_a_full_commit_branch_and_workflow() {
        let target = follow_target(&run(CiStatus::InProgress), None).unwrap();
        assert_eq!(
            (target.repo.as_str(), target.sha.as_str()),
            ("acme/widgets", SHA)
        );
        assert_eq!(
            (target.workflow.as_str(), target.branch.as_str()),
            ("CI", "main")
        );
        assert!(follow_target(&run(CiStatus::Queued), None).is_some());
        for status in [CiStatus::Success, CiStatus::Failure, CiStatus::Cancelled] {
            assert_eq!(follow_target(&run(status), None), None, "{status:?}");
        }
        let mut short = run(CiStatus::InProgress);
        short.sha = Some("8f6c8d8".into());
        assert_eq!(follow_target(&short, None), None);
        for strip in [
            |run: &mut CiRun| run.branch = None,
            |run: &mut CiRun| run.workflow = None,
            |run: &mut CiRun| run.run_url = None,
            |run: &mut CiRun| run.run_url = Some("https://ci.example.test/runs/1".into()),
        ] {
            let mut partial = run(CiStatus::InProgress);
            strip(&mut partial);
            assert_eq!(follow_target(&partial, None), None, "{partial:?}");
        }
    }

    fn job(status: Status, refs: &[(&str, &str)]) -> Job {
        Job {
            version: RECORD_VERSION,
            id: "01ARZ3NDEKTSV4RRFFQ69G5FE9".into(),
            checkout_id: "c".into(),
            kind: JobKind {
                family: ProducerFamily::Watch,
                name: "github-actions".into(),
            },
            job_key: "k".into(),
            title: "t".into(),
            capabilities: Capabilities::default(),
            ticket: None,
            external: None,
            retry_of: None,
            url: None,
            status,
            created_ms: 1,
            updated_ms: 1,
            started_ms: None,
            finished_ms: None,
            revision: 1,
            lease: None,
            cancel_requested_ms: None,
            cancel_failure: None,
            stop_watching_requested_ms: None,
            producer_eta_ms: None,
            history_estimate_ms: None,
            outcome: Some(Outcome {
                summary: None,
                refs: refs
                    .iter()
                    .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                    .collect::<BTreeMap<_, _>>(),
            }),
            steps: Vec::new(),
            latest: BTreeMap::new(),
            feed: FeedState::default(),
        }
    }

    #[test]
    fn hs2_cc7tcb_a_verdict_is_reported_as_the_run_it_rests_on() {
        let run_refs = [
            ("branch", "main"),
            ("workflow", "CI"),
            ("run_head_sha", SHA),
            ("run_url", URL),
            ("run_created_ms", "1000"),
            ("run_updated_ms", "3000"),
        ];
        for (status, gate, expected) in [
            (Status::Succeeded, "green", "success"),
            (Status::Failed, "red", "failure"),
            (Status::Cancelled, "cancelled", "cancelled"),
        ] {
            let mut refs = run_refs.to_vec();
            refs.push(("gate", gate));
            let report = verdict_report(&job(status, &refs)).unwrap();
            assert_eq!(report.status, expected);
            let stored = report.into_run(4_000).unwrap();
            assert_eq!(stored.sha.as_deref(), Some(SHA));
            assert_eq!(stored.run_url.as_deref(), Some(URL));
            assert_eq!(stored.branch.as_deref(), Some("main"));
            assert_eq!(stored.workflow.as_deref(), Some("CI"));
            assert_eq!(
                (stored.started_at_ms, stored.finished_at_ms),
                (Some(1_000), Some(3_000))
            );
            // The finished run replaces the in-flight report of the same run.
            assert!(stored.supersedes(&run(CiStatus::InProgress)));
        }
        // A finish before the start is dropped rather than refused.
        let mut odd = run_refs.to_vec();
        odd.retain(|(key, _)| *key != "run_updated_ms");
        odd.extend([("gate", "red"), ("run_updated_ms", "10")]);
        let report = verdict_report(&job(Status::Failed, &odd)).unwrap();
        assert_eq!(report.finished_at_ms, None);
        // Verdicts that name no run report nothing.
        for (status, refs) in [
            (Status::Failed, vec![("gate", "superseded")]),
            (Status::Failed, vec![("gate", "unknown")]),
            (Status::Cancelled, vec![("cancel", "stopped_watching")]),
            (Status::Lost, vec![]),
            (
                Status::Waiting,
                vec![("gate", "green"), ("run_head_sha", SHA)],
            ),
        ] {
            assert!(
                verdict_report(&job(status, &refs)).is_none(),
                "{status:?} {refs:?}"
            );
        }
    }

    #[test]
    fn hs2_cc7tcb_the_verdict_keeps_a_later_reported_start_of_the_same_commit() {
        let verdict = |sha: &str, started: Option<i64>, finished: Option<i64>| CiReport {
            status: "failure".into(),
            sha: Some(sha.into()),
            started_at_ms: started,
            finished_at_ms: finished,
            ..CiReport::default()
        };
        // The reporter counted from 1 000 (a re-run's start); the run was created at 500.
        let aligned = aligned_start(
            verdict(SHA, Some(500), Some(3_000)),
            &run(CiStatus::InProgress),
        );
        assert_eq!(
            (aligned.started_at_ms, aligned.finished_at_ms),
            (Some(1_000), Some(3_000))
        );
        assert!(
            aligned
                .into_run(4_000)
                .unwrap()
                .supersedes(&run(CiStatus::InProgress))
        );
        // A finish before that start is dropped, and a missing start is filled in.
        let aligned = aligned_start(verdict(SHA, None, Some(900)), &run(CiStatus::InProgress));
        assert_eq!(
            (aligned.started_at_ms, aligned.finished_at_ms),
            (Some(1_000), None)
        );
        // A later created time, or another commit's run, is left alone.
        let later = aligned_start(verdict(SHA, Some(1_500), None), &run(CiStatus::InProgress));
        assert_eq!(later.started_at_ms, Some(1_500));
        let other = "9".repeat(40);
        let descendant =
            aligned_start(verdict(&other, Some(500), None), &run(CiStatus::InProgress));
        assert_eq!(descendant.started_at_ms, Some(500));
    }

    #[test]
    fn hs2_cc7tcb_only_an_https_enterprise_root_names_a_host() {
        assert_eq!(enterprise_host(None), None);
        assert_eq!(
            enterprise_host(Some(" https://GHE.example.test/api/v3 ")).as_deref(),
            Some("ghe.example.test")
        );
        assert_eq!(enterprise_host(Some("http://ghe.example.test")), None);
        assert_eq!(enterprise_host(Some("https://")), None);
    }
}
