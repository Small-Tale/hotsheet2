//! Jobs (`docs/33-jobs.md`): read the machine-local Job store and run the watch monitor
//! (HS2-5TC1GQ). None of this needs a server or a ticket store. Reporting, running,
//! cancelling and tailing commands belong to HS2-V4Q2MA.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use hotsheet_jobs::{
    ChangeWaiter, Clock, Job, JobStore, JournalProbe, Monitor, MonitorClaim, MonitorConfig,
    MonitorControl, MonitorInfo, MonitorSlot, NoProbe, SlotState, StoreConfig, SystemClock,
    UlidSource, Wake,
};

use crate::Cmd;

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
            max_poll_ms: defaults.max_poll_ms.max(poll_ms),
            idle_exit_ms: self.idle_exit_ms.unwrap_or(defaults.idle_exit_ms),
            ..defaults
        }
    }

    fn flags(self) -> Vec<String> {
        [
            ("--lease-ms", self.lease_ms),
            ("--lost-after-ms", self.lost_after_ms),
            ("--poll-ms", self.poll_ms),
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
            JobWatchCmd::Status => print_json(&monitor_status(&MonitorSlot::at(&home))?),
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
    let slot = MonitorSlot::at(home);
    if let SlotState::Held(info) = slot.state()? {
        return print_json(
            &serde_json::json!({ "running": true, "started": false, "monitor": info }),
        );
    }
    let mut command = Command::new(std::env::current_exe().context("locating hotsheet-cli")?);
    command
        .args(["job", "monitor"])
        .args(args.flags())
        .env("HOTSHEET_HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach(&mut command);
    hotsheet_process_journal::LaunchContext {
        journal: hotsheet_process_journal::ProcessJournal::at(home),
        supervisor: "hotsheet-cli",
        purpose: hotsheet_process_journal::Purpose::JobMonitor,
        project_id: None,
        session_id: None,
        binary_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
    }
    .detached(&mut command)
    .context("starting the Job monitor")?;

    // Two starts can race: both spawn, one monitor claims the slot and the other exits.
    // Either way this start reports the monitor that is running.
    let deadline = Instant::now() + MONITOR_ANSWER;
    loop {
        if let SlotState::Held(Some(info)) = slot.state()? {
            return print_json(
                &serde_json::json!({ "running": true, "started": true, "monitor": info }),
            );
        }
        if Instant::now() >= deadline {
            bail!("the Job monitor did not start");
        }
        std::thread::sleep(EVENT_SETTLE);
    }
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

#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    // Its own process group, so a Ctrl-C in the starting terminal does not reach it.
    command.process_group(0);
}

#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn detach(_command: &mut Command) {}

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
    register_sources(&mut monitor);
    let waiter = ChangeWaiter::new(slot.dir())?;
    monitor.run(&mut ProcessControl { claim, waiter })?;
    Ok(())
}

fn register_sources(monitor: &mut Monitor) {
    // A file-backed source so process tests can drive a real monitor offline. It is not
    // compiled into release builds.
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os("HOTSHEET_JOB_WATCH_FIXTURES") {
        monitor.register(Box::new(hotsheet_jobs::FixtureWatchSource::new(dir)));
    }
    #[cfg(not(debug_assertions))]
    let _ = monitor;
}
