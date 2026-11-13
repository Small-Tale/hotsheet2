//! The external sources a reconciliation reads besides the target ticket
//! (HS2-7QNK7Y and its siblings), each as a typed [`Observed<ExternalWork>`] built
//! from this machine's own records and its adapters' records. The classification is
//! pure and lives in the core (`reconcile_sources.rs`); this module only reads.
use hotsheet_ticketing::orchestration::{
    ExternalWork, GateError, GateFilter, GateKind, GateObservation, Observations, Observed, Run,
    TargetFacts, ci_work,
};

use super::*;

/// The code of a source whose records on this machine could not be read.
const GATE_RECORDS_UNREADABLE: &str = "gate_records_unreadable";

impl OrchestrationRuntime {
    /// Every observation reconciliation compares a run with, given what the target
    /// read showed.
    pub(super) fn run_observations(
        &self,
        owner_id: &str,
        run: &Run,
        target: Observed<TargetFacts>,
    ) -> Observations {
        Observations {
            ci: self.run_ci_work(owner_id, &run.run_id),
            ..Observations::target_only(target)
        }
    }

    /// The run's standing gates of one kind, each with its adapter's own answer.
    /// Reads this machine's records and the adapters' records; contacts no provider.
    fn gate_readings(
        &self,
        owner_id: &str,
        run_id: &str,
        kind: GateKind,
    ) -> Result<Vec<GateObservation>, OrchestrationError> {
        let gates = self.gate_registry().list(
            owner_id,
            &GateFilter {
                run_id: Some(run_id.into()),
                current_only: true,
                ..GateFilter::default()
            },
        )?;
        Ok(gates
            .into_iter()
            .filter(|gate| gate.kind == kind)
            .map(|gate| {
                let reading = match self.gate_provider(&gate.provider, gate.kind) {
                    Some(provider) => provider.read(&gate).map_err(|error| error.code().into()),
                    None => Err(GateError::ProviderUnsupported.code().into()),
                };
                (gate, reading)
            })
            .collect())
    }

    /// HS2-7QNK7Y: the CI runs gating a run, for reconciliation. Verified when the
    /// run's waiting CI gates are the watches the CI adapter shows live (or there are
    /// none in flight); mismatched with `recorded_work_missing` when a watch the run
    /// waits on is gone; unverifiable with `watch_lost` or the adapter's code when it
    /// cannot say.
    pub fn run_ci_work(&self, owner_id: &str, run_id: &str) -> Observed<ExternalWork> {
        match self.gate_readings(owner_id, run_id, GateKind::Ci) {
            Ok(readings) => ci_work(&readings),
            Err(_) => Observed::Unverifiable(GATE_RECORDS_UNREADABLE.into()),
        }
    }
}
