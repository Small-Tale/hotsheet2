//! HS2-AMJNER: the orchestration work this server runs between requests.
//!
//! One thread per server waits on the directories the runtime names
//! ([`OrchestrationRuntime::automation_directories`]) with the platform file watcher,
//! and runs one automation pass when any of them changes: template applies waiting on
//! a permission a person answered, or on a planner that is free again, continue under
//! their own operation id. It also wakes at the earliest instant the last pass named
//! (a backed-off resume), and at the watchers' own one-second safety re-check, which
//! reads local files only. Idle, it makes no request and contacts nothing.
use crate::AppState;
use hotsheet_cli::orchestration::{AutomationMemory, OrchestrationRuntime};

/// The longest the thread waits before it reads its local records again: the change
/// waiters' safety re-check, and how soon a stop is seen.
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
                let Ok(shared) = super::orchestration::server_runtime(&state, handle) else {
                    return;
                };
                let directories = shared.automation_directories();
                for directory in &directories {
                    let _ = std::fs::create_dir_all(directory);
                }
                let paths: Vec<&std::path::Path> = directories
                    .iter()
                    .map(std::path::PathBuf::as_path)
                    .collect();
                let waiter = hotsheet_jobs::ChangeWaiter::starting(&paths).ok();
                let mut memory = AutomationMemory::default();
                while !state.is_stopping() {
                    let report = OrchestrationRuntime::automation_pass_now(&shared, &mut memory);
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}
