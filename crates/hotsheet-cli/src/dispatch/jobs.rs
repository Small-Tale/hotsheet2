//! Jobs (`docs/33-jobs.md`): read the machine-local Job store and run the watch monitor
//! (HS2-5TC1GQ), including the GitHub Actions CI gate of a commit (HS2-3R9V0R). None of
//! this needs a server or a ticket store. Reporting, running, cancelling and tailing
//! commands belong to HS2-V4Q2MA.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use hotsheet_jobs::{
    ChangeWaiter, CiTarget, CiWatchOptions, Clock, GateReading, GateState, GithubActionsSource,
    Job, JobStore, JournalProbe, Monitor, MonitorClaim, MonitorConfig, MonitorControl, MonitorInfo,
    MonitorSlot, NoProbe, SlotState, StoreConfig, SystemClock, TicketLink, UlidSource, Wake,
    ensure_ci_watch, gate_state, rewatch_ci, wait_for_gate_change,
};

use crate::Cmd;

use super::job_github::{UreqGithubApi, repo_from_remote};
use super::{Ctx, misrouted};

/// How long `job watch start` and `stop` wait for the monitor to answer.
const MONITOR_ANSWER: Duration = Duration::from_secs(15);
/// File events arrive in bursts; one short pause folds a burst into one monitor step.
const EVENT_SETTLE: Duration = Duration::from_millis(25);

#[derive(Subcommand)]
pub(crate) enum JobCmd {
    /// List a checkout's Jobs as JSON, after turning expired producers into `lost` and
    /// applying retention. Reads the machine-local Job store; no server is needed.
    List {
        /// Checkout id, alias, or path (default: the current directory).
        #[arg(long, default_value = ".")]
        checkout: String,
    },
    /// Print one Job as JSON with its derived liveness, progress and ETA.
    Show {
        id: String,
        /// Checkout id, alias, or path (default: the current directory).
        #[arg(long, default_value = ".")]
        checkout: String,
        /// Also print feed entries after this sequence (0 reads from the start).
        #[arg(long)]
        feed_after: Option<u64>,
        #[arg(long, default_value_t = 200, value_parser = clap::value_parser!(u32).range(1..=1000))]
        feed_limit: u32,
    },
    /// The machine's watch monitor: one background process that observes external
    /// operations such as CI runs for every watch Job and reports them as Jobs.
    Watch {
        #[command(subcommand)]
        cmd: JobWatchCmd,
    },
    /// The monitor process itself. Started by `job watch start`; not for direct use.
    #[command(hide = true)]
    Monitor(MonitorArgs),
}

#[derive(Subcommand)]
pub(crate) enum JobWatchCmd {
    /// Start the monitor if it is not running. Repeating it changes nothing. The monitor
    /// exits by itself once no watch Job has needed it for a short while.
    Start(MonitorArgs),
    /// Stop the monitor if it is running. Repeating it changes nothing. Watch Jobs are
    /// not finished by this: they read as stalled and resume on the next start.
    Stop,
    /// Print whether the monitor is running.
    Status,
    /// Watch the CI gate of a commit: start or find its watch Job and make sure the
    /// monitor runs. Repeating it changes nothing. The gate is green when a successful
    /// push run of the workflow on the branch contains the commit, as
    /// `scripts/ci-gate.mjs status --covers <sha>` defines it.
    Ci {
        #[command(flatten)]
        target: CiArgs,
        /// The ticket waiting on this gate, as `<connection_id>:<native_id>`.
        #[arg(long)]
        ticket: Option<String>,
        /// After a red or cancelled verdict, watch again in a new linked Job that
        /// ignores the run already judged (for a pushed fix or a re-run).
        #[arg(long)]
        rewatch: bool,
        /// Do not offer cancelling the commit's own run; stop watching stays available.
        #[arg(long)]
        no_cancel: bool,
        #[command(flatten)]
        monitor: MonitorArgs,
    },
    /// Print the CI gate of a commit as JSON from its watch Job, without contacting the
    /// provider: `pending`, `green`, `red`, `superseded`, `cancelled` or `unknown`.
    Gate {
        #[command(flatten)]
        target: CiArgs,
        /// Wait up to this many seconds for a verdict before printing. The wait wakes on
        /// the Job store's file changes; it does not poll.
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..=86_400))]
        wait_seconds: Option<u64>,
    },
}

/// The commit whose CI gate is meant, and where its runs are.
#[derive(Args, Clone)]
pub(crate) struct CiArgs {
    /// Full 40-character commit SHA.
    #[arg(long)]
    commit: String,
    /// `owner/name`. Defaults to the checkout's `origin` remote.
    #[arg(long)]
    repo: Option<String>,
    /// The shipping branch whose push runs count.
    #[arg(long, default_value = "main")]
    branch: String,
    /// The workflow's name as the provider shows it.
    #[arg(long, default_value = "CI")]
    workflow: String,
    /// Checkout id, alias, or path (default: the current directory).
    #[arg(long, default_value = ".")]
    checkout: String,
}

/// Timing overrides for tests. The defaults are the product behavior.
#[derive(Args, Clone, Copy, Default)]
pub(crate) struct MonitorArgs {
    #[arg(long, hide = true)]
    lease_ms: Option<u64>,
    #[arg(long, hide = true)]
    lost_after_ms: Option<u64>,
    #[arg(long, hide = true)]
    poll_ms: Option<u64>,
    /// HS2-R49D43: the ceiling of the unchanged-poll backoff. A test that changes its
    /// fixture after a slow step (seconds on a loaded machine) otherwise waits out a
    /// backed-off poll of up to two minutes.
    #[arg(long, hide = true)]
    max_poll_ms: Option<u64>,
    #[arg(long, hide = true)]
    idle_exit_ms: Option<u64>,
}

impl MonitorArgs {
    fn store_config(self) -> StoreConfig {
        let defaults = StoreConfig::default();
        StoreConfig {
            lease_ms: self.lease_ms.unwrap_or(defaults.lease_ms),
            lost_after_ms: self.lost_after_ms.unwrap_or(defaults.lost_after_ms),
            ..defaults
        }
    }

    fn monitor_config(self) -> MonitorConfig {
        let defaults = MonitorConfig::default();
        let poll_ms = self.poll_ms.unwrap_or(defaults.poll_ms);
        MonitorConfig {
            poll_ms,
            max_poll_ms: self
                .max_poll_ms
                .unwrap_or(defaults.max_poll_ms)
                .max(poll_ms),
            idle_exit_ms: self.idle_exit_ms.unwrap_or(defaults.idle_exit_ms),
            ..defaults
        }
    }

    fn flags(self) -> Vec<String> {
        [
            ("--lease-ms", self.lease_ms),
            ("--lost-after-ms", self.lost_after_ms),
            ("--poll-ms", self.poll_ms),
            ("--max-poll-ms", self.max_poll_ms),
            ("--idle-exit-ms", self.idle_exit_ms),
        ]
        .into_iter()
        .filter_map(|(flag, value)| value.map(|value| [flag.to_owned(), value.to_string()]))
        .flatten()
        .collect()
    }
}

pub(super) fn run(cmd: Cmd, _ctx: &Ctx) -> Result<()> {
    let Cmd::Job { cmd } = cmd else {
        misrouted("jobs")
    };
    let home = hotsheet_plugins::hotsheet_home();
    match cmd {
        JobCmd::List { checkout } => list(&home, &checkout),
        JobCmd::Show {
            id,
            checkout,
            feed_after,
            feed_limit,
        } => show(&home, &checkout, &id, feed_after, feed_limit),
        JobCmd::Watch { cmd } => match cmd {
            JobWatchCmd::Start(args) => watch_start(&home, args),
            JobWatchCmd::Stop => watch_stop(&home),
            JobWatchCmd::Status => {
                let mut status = monitor_status(&MonitorSlot::at(&home))?;
                // HS2-RTEQ7R: where a GitLab watch would take its token from, if any.
                status["gitlab_token"] = super::job_gitlab::token_source(&home)
                    .map(|source| source.code())
                    .into();
                // HS2-17HVNP: whether a Buildkite or CircleCI token is configured, and
                // HS2-7H0S8R a Jenkins one, HS2-JZ1DD2 an Azure DevOps one, HS2-BWF72S a
                // Bitbucket one.
                for (key, provider) in [
                    ("buildkite_token", super::job_ci_rest::BUILDKITE),
                    ("circleci_token", super::job_ci_rest::CIRCLECI),
                    ("jenkins_token", super::job_ci_rest::JENKINS),
                    ("azure_pipelines_token", super::job_ci_rest::AZURE_PIPELINES),
                    (
                        "bitbucket_pipelines_token",
                        super::job_ci_rest::BITBUCKET_PIPELINES,
                    ),
                ] {
                    status[key] = provider.token_source().into();
                }
                print_json(&status)
            }
            JobWatchCmd::Ci {
                target,
                ticket,
                rewatch,
                no_cancel,
                monitor,
            } => watch_ci(
                &home,
                &target,
                ticket.as_deref(),
                rewatch,
                !no_cancel,
                monitor,
            ),
            JobWatchCmd::Gate {
                target,
                wait_seconds,
            } => watch_gate(&home, &target, wait_seconds),
        },
        JobCmd::Monitor(args) => monitor(&home, args),
    }
}

fn print_json(value: &serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// A registered checkout by id, alias or path; otherwise the id of an existing
/// directory; otherwise a literal id that already has Jobs on this machine.
pub(crate) fn resolve_checkout_id(home: &Path, reference: &str) -> Result<String> {
    let registry =
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json"));
    if let Ok(checkout) = registry.describe(reference) {
        return Ok(checkout.id);
    }
    let path = Path::new(reference);
    if path.is_dir() {
        return Ok(hotsheet_ticketing::checkouts::checkout_id(path)?);
    }
    let store = JobStore::at(home);
    if store.checkout_dir(reference).is_ok_and(|dir| dir.is_dir()) {
        return Ok(reference.to_owned());
    }
    bail!("unknown checkout: pass --checkout with a checkout id, alias, or path")
}

/// The stored Job plus what a reader would otherwise have to derive.
fn job_view(job: &Job, now_ms: u64) -> Result<serde_json::Value> {
    let mut view = serde_json::to_value(job)?;
    if let Some(fields) = view.as_object_mut() {
        fields.insert(
            "liveness".into(),
            serde_json::to_value(job.liveness(now_ms))?,
        );
        fields.insert("progress".into(), serde_json::to_value(job.progress())?);
        fields.insert("eta".into(), serde_json::to_value(job.eta())?);
    }
    Ok(view)
}

fn monitor_status(slot: &MonitorSlot) -> Result<serde_json::Value> {
    Ok(match slot.state()? {
        SlotState::Free => serde_json::json!({ "running": false, "monitor": null }),
        SlotState::Held(info) => serde_json::json!({ "running": true, "monitor": info }),
    })
}

fn list(home: &Path, checkout: &str) -> Result<()> {
    let checkout_id = resolve_checkout_id(home, checkout)?;
    let store = JobStore::at(home);
    let journal = hotsheet_process_journal::ProcessJournal::at(home);
    // Owned Jobs whose launch has ended are lost at once; the rest by lease expiry.
    match JournalProbe::load(&journal) {
        Ok(probe) => store.reconcile(&checkout_id, &probe)?,
        Err(_) => store.reconcile(&checkout_id, &NoProbe)?,
    };
    let listing = store.list(&checkout_id)?;
    let now = store.now_ms();
    let jobs = listing
        .jobs
        .iter()
        .map(|job| job_view(job, now))
        .collect::<Result<Vec<_>>>()?;
    print_json(&serde_json::json!({
        "checkout_id": checkout_id,
        "jobs": jobs,
        "unreadable": listing.unreadable,
    }))
}

fn show(
    home: &Path,
    checkout: &str,
    id: &str,
    feed_after: Option<u64>,
    feed_limit: u32,
) -> Result<()> {
    let checkout_id = resolve_checkout_id(home, checkout)?;
    let store = JobStore::at(home);
    let job = store.get(&checkout_id, id)?;
    let mut view = job_view(&job, store.now_ms())?;
    if let (Some(after), Some(fields)) = (feed_after, view.as_object_mut()) {
        let page = store.read_feed(&checkout_id, id, after, feed_limit as usize)?;
        fields.insert("feed_page".into(), serde_json::to_value(page)?);
    }
    print_json(&view)
}

fn watch_start(home: &Path, args: MonitorArgs) -> Result<()> {
    let (info, started) = ensure_monitor(home, args)?;
    print_json(&serde_json::json!({ "running": true, "started": started, "monitor": info }))
}

/// Start the detached monitor unless one already runs, and report the one that does.
/// HS2-VVHVF0: the workflow itself is the shared runtime's, so the server and MCP start
/// the monitor the same way.
fn ensure_monitor(home: &Path, args: MonitorArgs) -> Result<(MonitorInfo, bool)> {
    hotsheet_cli::orchestration::DetachedJobMonitor::new("hotsheet-cli")
        .flags(args.flags())
        .ensure_info(home)
        .context("starting the Job monitor")
}

/// HS2-5JBYPV: make sure the monitor runs with its default timing, for a caller that
/// just created a watch Job outside `job watch ci`. Returns whether this call started it.
pub(crate) fn ensure_default_monitor(home: &Path) -> Result<bool> {
    ensure_monitor(home, MonitorArgs::default()).map(|(_, started)| started)
}

/// The checkout id and the CI target the arguments name. Without `--repo` the
/// repository comes from the checkout's `origin` remote.
fn ci_target(home: &Path, args: &CiArgs) -> Result<(String, CiTarget)> {
    let checkout_id = resolve_checkout_id(home, &args.checkout)?;
    let repo = match &args.repo {
        Some(repo) => repo.clone(),
        None => origin_repo(home, &args.checkout)
            .context("no GitHub repository found for this checkout: pass --repo <owner>/<name>")?,
    };
    let target = CiTarget::new(&repo, &args.commit, &args.workflow, &args.branch).context(
        "pass --commit as a full 40-character SHA, --repo as <owner>/<name>, and a plain \
         --branch name",
    )?;
    Ok((checkout_id, target))
}

fn origin_repo(home: &Path, reference: &str) -> Option<String> {
    let registry =
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json"));
    let root = match registry.describe(reference) {
        Ok(checkout) => std::path::PathBuf::from(checkout.root),
        Err(_) => std::path::PathBuf::from(reference),
    };
    let output = hotsheet_ticketing::git::command_in(&root)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    repo_from_remote(&String::from_utf8(output.stdout).ok()?)
}

fn gate_json(reading: &GateReading) -> serde_json::Value {
    let (state, run) = match &reading.state {
        GateState::Pending => ("pending", None),
        GateState::Green { run } => ("green", Some(run)),
        GateState::Red { run } => ("red", Some(run)),
        GateState::Superseded => ("superseded", None),
        GateState::Cancelled => ("cancelled", None),
        GateState::Unknown => ("unknown", None),
    };
    serde_json::json!({
        "state": state,
        "run": run.map(|run| serde_json::json!({
            "id": run.id,
            "attempt": run.attempt,
            "head_sha": run.head_sha,
            "url": run.url,
        })),
        "job_id": reading.job_id,
        "revision": reading.revision,
    })
}

fn watch_ci(
    home: &Path,
    args: &CiArgs,
    ticket: Option<&str>,
    rewatch: bool,
    can_cancel: bool,
    monitor: MonitorArgs,
) -> Result<()> {
    let (checkout_id, target) = ci_target(home, args)?;
    let ticket = ticket
        .map(|qualified| {
            let (connection_id, native_id) = qualified
                .split_once(':')
                .filter(|(connection, native)| !connection.is_empty() && !native.is_empty())
                .context("pass --ticket as <connection_id>:<native_id>")?;
            Ok::<_, anyhow::Error>(TicketLink {
                connection_id: connection_id.to_owned(),
                native_id: native_id.to_owned(),
            })
        })
        .transpose()?;
    let options = CiWatchOptions { ticket, can_cancel };
    let store = JobStore::at(home);
    let job = if rewatch {
        rewatch_ci(&store, &checkout_id, &target, &options)?
    } else {
        ensure_ci_watch(&store, &checkout_id, &target, &options)?
    };
    // A finished watch needs no observer; anything else does.
    let monitor = if job.status.is_finished() && job.status != hotsheet_jobs::Status::Lost {
        monitor_status(&MonitorSlot::at(home))?
    } else {
        let (info, started) = ensure_monitor(home, monitor)?;
        serde_json::json!({ "running": true, "started": started, "monitor": info })
    };
    let reading = gate_state(&store, &checkout_id, &target)?;
    print_json(&serde_json::json!({
        "checkout_id": checkout_id,
        "job": job_view(&job, store.now_ms())?,
        "gate": gate_json(&reading),
        "monitor": monitor,
    }))
}

fn watch_gate(home: &Path, args: &CiArgs, wait_seconds: Option<u64>) -> Result<()> {
    let (checkout_id, target) = ci_target(home, args)?;
    let store = JobStore::at(home);
    let mut reading = gate_state(&store, &checkout_id, &target)?;
    if let Some(seconds) = wait_seconds {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        // Only a pending gate can still become a verdict.
        while reading.state == GateState::Pending {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            reading =
                wait_for_gate_change(&store, &checkout_id, &target, &reading.state, remaining)?;
        }
    }
    print_json(&gate_json(&reading))
}

fn watch_stop(home: &Path) -> Result<()> {
    let slot = MonitorSlot::at(home);
    let deadline = Instant::now() + MONITOR_ANSWER;
    let mut stopped = None;
    loop {
        match slot.state()? {
            SlotState::Free => {
                return print_json(&serde_json::json!({
                    "running": false,
                    "stopped": stopped.is_some(),
                    "monitor": stopped,
                }));
            }
            // The request names one monitor, so it cannot stop a later one by mistake.
            SlotState::Held(Some(info)) => {
                if stopped.as_ref() != Some(&info) {
                    slot.request_stop(&info.owner)?;
                    stopped = Some(info);
                }
            }
            // Still publishing itself; ask again in a moment.
            SlotState::Held(None) => {}
        }
        if Instant::now() >= deadline {
            bail!("the Job monitor did not stop");
        }
        std::thread::sleep(EVENT_SETTLE);
    }
}

/// Waits between monitor steps: until the next renewal or poll is due, or earlier when
/// something changes under the Job directory (a new Job, a cancel request, a stop).
struct ProcessControl {
    claim: MonitorClaim,
    waiter: ChangeWaiter,
}

impl MonitorControl for ProcessControl {
    fn wait_until(&mut self, wake_ms: u64) -> Wake {
        loop {
            if self.claim.stop_requested() {
                return Wake::Stop;
            }
            let now = SystemClock.now_ms();
            if now >= wake_ms {
                return Wake::Continue;
            }
            if self.waiter.wait(Duration::from_millis(wake_ms - now)) {
                std::thread::sleep(EVENT_SETTLE);
                self.waiter.wait(Duration::ZERO);
                return if self.claim.stop_requested() {
                    Wake::Stop
                } else {
                    Wake::Continue
                };
            }
        }
    }
}

fn monitor(home: &Path, args: MonitorArgs) -> Result<()> {
    let slot = MonitorSlot::at(home);
    let info = MonitorInfo {
        owner: format!("monitor:{}", ulid::Ulid::new()),
        pid: std::process::id(),
        started_ms: SystemClock.now_ms(),
    };
    // Another monitor already serves this machine: nothing to do.
    let Some(claim) = slot.try_claim(&info)? else {
        return Ok(());
    };
    let store = JobStore::with(
        home,
        Arc::new(SystemClock),
        Arc::new(UlidSource),
        args.store_config(),
    );
    let mut monitor = Monitor::new(store, info.owner, args.monitor_config());
    register_sources(&mut monitor, home);
    // HS2-6RQVFY: the monitor reads its store each step, so its stream starts in the
    // background rather than holding the first step for a start that took seconds.
    let waiter = ChangeWaiter::starting(&[slot.dir()])?;
    monitor.run(&mut ProcessControl { claim, waiter })?;
    Ok(())
}

fn register_sources(monitor: &mut Monitor, home: &Path) {
    monitor.register(Box::new(GithubActionsSource::new(UreqGithubApi::new(home))));
    // HS2-TPAHYA: GitLab CI pipelines, through the same REST port.
    monitor.register(Box::new(hotsheet_jobs::GitlabCiSource::new(
        super::job_gitlab::UreqGitlabApi::new(home),
    )));
    // HS2-17HVNP: Buildkite builds and CircleCI pipelines, through the same REST port.
    monitor.register(Box::new(hotsheet_jobs::BuildkiteSource::new(
        super::job_ci_rest::UreqCiRestApi::new(super::job_ci_rest::BUILDKITE),
    )));
    monitor.register(Box::new(hotsheet_jobs::CircleciSource::new(
        super::job_ci_rest::UreqCiRestApi::new(super::job_ci_rest::CIRCLECI),
    )));
    // HS2-7H0S8R: Jenkins builds, through the same REST port.
    monitor.register(Box::new(hotsheet_jobs::JenkinsSource::new(
        super::job_ci_rest::UreqCiRestApi::new(super::job_ci_rest::JENKINS),
    )));
    // HS2-JZ1DD2: Azure Pipelines builds, through the same REST port.
    monitor.register(Box::new(hotsheet_jobs::AzurePipelinesSource::new(
        super::job_ci_rest::UreqCiRestApi::new(super::job_ci_rest::AZURE_PIPELINES),
    )));
    // HS2-BWF72S: Bitbucket Pipelines, through the same REST port.
    monitor.register(Box::new(hotsheet_jobs::BitbucketPipelinesSource::new(
        super::job_ci_rest::UreqCiRestApi::new(super::job_ci_rest::BITBUCKET_PIPELINES),
    )));
    // A file-backed source so process tests can drive a real monitor offline. It is not
    // compiled into release builds.
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os("HOTSHEET_JOB_WATCH_FIXTURES") {
        monitor.register(Box::new(hotsheet_jobs::FixtureWatchSource::new(dir)));
    }
}
