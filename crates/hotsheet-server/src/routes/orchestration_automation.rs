//! HS2-AMJNER: the orchestration work this server runs between requests.
//!
//! One thread per server waits on the directories the runtime names
//! ([`OrchestrationRuntime::automation_directories`]) with the platform file watcher,
//! and runs one automation pass when any of them changes: template applies waiting on
//! a permission a person answered, or on a planner that is free again, continue under
//! their own operation id; HS2-KXKA7X: triggers fire (a schedule's occurrence, a run's
//! failed gate, a queued delivery whose run finished). It also wakes at the earliest
//! instant the last pass named (a schedule's next occurrence
//! or a backed-off resume), and at the watchers' own one-second safety re-check, which
//! reads local files only. Idle, it makes no request and contacts nothing.
use crate::AppState;
use hotsheet_cli::orchestration::{AutomationMemory, OrchestrationRuntime};

/// The longest the thread waits before it reads its local records again: the change
/// waiters' safety re-check. A stop interrupts the file wait.
const AUTOMATION_RECHECK: std::time::Duration = std::time::Duration::from_secs(1);

impl AppState {
    /// Start this server's automation thread. Call inside the runtime, once per
    /// serving process.
    pub fn spawn_orchestration_automation(&self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let state = self.clone();
        let tasks = self.orchestration_tasks().threads().clone();
        // HS2-H1CSSN: kept by this server's orchestration tasks and joined at shutdown.
        let _ = self
            .orchestration_tasks()
            .threads()
            .spawn("orchestration-automation", move || {
                let Ok(shared) = super::orchestration::server_runtime(&state, handle.clone())
                else {
                    return;
                };
                let directories = shared.automation_directories();
                // HS2-KXKA7X: the trigger files as last seen, so a trigger turned on by
                // a headless CLI gets its target follower and branch watch here.
                let mut seen_triggers: Option<Vec<(String, Option<std::time::SystemTime>)>> = None;
                for directory in &directories {
                    let _ = std::fs::create_dir_all(directory);
                }
                let paths: Vec<&std::path::Path> = directories
                    .iter()
                    .map(std::path::PathBuf::as_path)
                    .collect();
                let waiter = hotsheet_jobs::ChangeWaiter::starting(&paths).ok();
                let _stop = waiter
                    .as_ref()
                    .map(|waiter| tasks.on_stop(waiter.cancel_handle()));
                let mut memory = AutomationMemory::default();
                while !state.is_stopping() {
                    let report = OrchestrationRuntime::automation_pass_now(&shared, &mut memory);
                    for owner in report.triggered.keys() {
                        super::orchestration::note_orchestration_change(&state, owner);
                    }
                    let triggers = trigger_files(&shared);
                    if seen_triggers.as_ref() != Some(&triggers) {
                        for (owner, _) in &triggers {
                            super::orchestration::ensure_trigger_followers(&state, &handle, owner);
                        }
                        seen_triggers = Some(triggers);
                    }
                    let wait = report
                        .next_wake_ms
                        .map(|at| std::time::Duration::from_millis(at.saturating_sub(now_ms())))
                        .map_or(AUTOMATION_RECHECK, |until| until.min(AUTOMATION_RECHECK));
                    match &waiter {
                        Some(waiter) => {
                            let _ = waiter.wait(wait);
                        }
                        None => {
                            tasks.wait_stop(wait);
                        }
                    }
                }
            });
    }
}

/// Each trigger file of this machine with when it last changed.
fn trigger_files(shared: &OrchestrationRuntime) -> Vec<(String, Option<std::time::SystemTime>)> {
    shared
        .trigger_owners()
        .into_iter()
        .map(|owner| {
            let modified = std::fs::metadata(shared.trigger_file(&owner))
                .and_then(|metadata| metadata.modified())
                .ok();
            (owner, modified)
        })
        .collect()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}
