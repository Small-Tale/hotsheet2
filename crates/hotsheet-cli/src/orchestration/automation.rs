//! HS2-AMJNER: the orchestration work this machine does on its own between requests,
//! as one pass a host runs whenever something it depends on changed.
//!
//! A pass continues template applies that wait on a permission a person has now
//! answered, or on a planner that is now free. HS2-KXKA7X: it also runs every trigger
//! pass (due schedule occurrences, failed run gates, queued deliveries, template
//! changes that need a review, an AI session's answered turn On) and names the next
//! instant a schedule is due, so the host wakes then and not on an interval. The server runs it from one thread
//! that waits on the directories [`OrchestrationRuntime::automation_directories`]
//! names (a permission answer and a finished planner turn both write there), never on
//! an interval; `hotsheet-cli orchestration template resume` runs one pass headless.
use std::collections::BTreeMap;
use std::path::PathBuf;

use hotsheet_ticketing::orchestration::{TEMPLATE_APPLY_DIR, TRIGGER_DIR};
use serde::{Deserialize, Serialize};

use super::request::{Clock, Rng};
use super::templates::{ResumeMemory, ResumeReport};
use super::triggers::DeliveryView;
use super::*;

/// What a host keeps between automation passes, per orchestration project.
#[derive(Debug, Default, Clone)]
pub struct AutomationMemory {
    resume: BTreeMap<String, ResumeMemory>,
}

/// What one automation pass did, per orchestration project.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationReport {
    /// Template applies continued, by owner.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resumed: BTreeMap<String, ResumeReport>,
    /// HS2-KXKA7X: trigger deliveries, by owner.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub triggered: BTreeMap<String, Vec<DeliveryView>>,
    /// The earliest instant a pass has work at, when one is known (a backed-off
    /// resume); the host wakes then even when nothing changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_wake_ms: Option<u64>,
}

fn owners_in(directory: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut owners: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let owner = name.strip_suffix(".json")?;
            (!owner.is_empty() && !owner.contains('.')).then(|| owner.to_owned())
        })
        .collect();
    owners.sort();
    owners
}

impl OrchestrationRuntime {
    /// The orchestration projects with automation records on this machine.
    pub fn automation_owners(&self) -> Vec<String> {
        let mut owners = owners_in(&self.home.join(TEMPLATE_APPLY_DIR));
        owners.extend(owners_in(&self.home.join(TRIGGER_DIR)));
        owners.sort();
        owners.dedup();
        owners
    }

    /// The directories whose changes can give a pass something to do: permission
    /// answers (the permission signal and the scheduler wake marker an answer writes),
    /// plan updates and the run event journal (a planner finishing its turn), and the
    /// apply records themselves.
    pub fn automation_directories(&self) -> Vec<PathBuf> {
        [
            super::PERMISSION_SIGNAL_DIRECTORY,
            super::SCHEDULER_WAKE_DIRECTORY,
            hotsheet_ticketing::orchestration::PLAN_UPDATE_DIR,
            "orchestration-run-events",
            TEMPLATE_APPLY_DIR,
            TRIGGER_DIR,
        ]
        .into_iter()
        .map(|leaf| self.home.join(leaf))
        .collect()
    }

    /// One automation pass over every orchestration project on this machine. A
    /// project that cannot be read is skipped; the next pass tries again.
    pub fn automation_pass(
        &self,
        memory: &mut AutomationMemory,
        tx: &mut Transaction<'_>,
    ) -> AutomationReport {
        let mut report = AutomationReport::default();
        let owners = self.automation_owners();
        memory.resume.retain(|owner, _| owners.contains(owner));
        for owner in owners {
            let resume = memory.resume.entry(owner.clone()).or_default();
            if let Ok(resumed) = self.template_resume_waiting(&owner, resume, tx)
                && !resumed.resumed.is_empty()
            {
                report.resumed.insert(owner.clone(), resumed);
            }
            if let Some(at) = resume.next_try_ms() {
                report.next_wake_ms = Some(report.next_wake_ms.map_or(at, |known| known.min(at)));
            }
            // HS2-KXKA7X: triggers. A project with none reads one missing file.
            if let Ok(passed) = self.trigger_pass(&owner, tx) {
                if !passed.deliveries.is_empty() {
                    report.triggered.insert(owner.clone(), passed.deliveries);
                }
                if let Some(at) = passed.next_wake_ms {
                    report.next_wake_ms =
                        Some(report.next_wake_ms.map_or(at, |known| known.min(at)));
                }
            }
        }
        report
    }

    /// [`Self::automation_pass`] on the real clock, for a host's background thread.
    pub fn automation_pass_now(&self, memory: &mut AutomationMemory) -> AutomationReport {
        let mut rng = Rng;
        self.automation_pass(
            memory,
            &mut Transaction {
                clock: &Clock,
                rng: &mut rng,
            },
        )
    }
}
