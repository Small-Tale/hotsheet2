//! Ticket-flow aggregates derivable from the current ticket records. Historical
//! cumulative-flow series require transition events and are therefore not fabricated.

use hotsheet_model::{Status, Ticket};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TicketFlowSummary {
    pub total: u64,
    pub open: u64,
    pub completed: u64,
    pub current_by_status: BTreeMap<String, u64>,
    pub current_by_category: BTreeMap<String, u64>,
    pub throughput_by_day: BTreeMap<String, u64>,
    pub cycle_time_seconds: DurationSummary,
    /// Always false until ticket status-transition history is persisted.
    pub historical_cumulative_flow_available: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DurationSummary {
    pub samples: u64,
    pub average: f64,
    pub p50: f64,
    pub p95: f64,
}

pub fn ticket_flow(tickets: &[Ticket]) -> TicketFlowSummary {
    let mut out = TicketFlowSummary {
        total: tickets.len() as u64,
        ..Default::default()
    };
    let mut durations = Vec::new();
    for t in tickets {
        let status = serde_json::to_value(t.status)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "unknown".into());
        *out.current_by_status.entry(status).or_default() += 1;
        *out.current_by_category
            .entry(t.category.clone())
            .or_default() += 1;
        if matches!(
            t.status,
            Status::Completed | Status::Verified | Status::Archive
        ) {
            out.completed += 1;
        } else {
            out.open += 1;
        }
        if let Some(done) = t.completed_at.as_ref().or(t.closed_at.as_ref()) {
            *out.throughput_by_day
                .entry(done.as_str().get(..10).unwrap_or("unknown").to_owned())
                .or_default() += 1;
            if let (Some(start), Some(end)) = (t.created_at.instant(), done.instant()) {
                durations.push((end - start).whole_seconds().max(0) as f64);
            }
        }
    }
    durations.sort_by(f64::total_cmp);
    if !durations.is_empty() {
        out.cycle_time_seconds.samples = durations.len() as u64;
        out.cycle_time_seconds.average = durations.iter().sum::<f64>() / durations.len() as f64;
        out.cycle_time_seconds.p50 = percentile(&durations, 0.50);
        out.cycle_time_seconds.p95 = percentile(&durations, 0.95);
    }
    out
}

fn percentile(values: &[f64], p: f64) -> f64 {
    values[((values.len() - 1) as f64 * p).ceil() as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aggregates_current_flow_and_cycle_time_without_inventing_history() {
        let mut a = Ticket::new(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().unwrap(),
            "HS-A",
            "A",
            "bug",
            "2026-01-01T00:00:00Z",
            "2026-01-03T00:00:00Z",
        );
        a.status = Status::Completed;
        a.completed_at = Some("2026-01-03T00:00:00Z".into());
        let b = Ticket::new(
            "01ARZ3NDEKTSV4RRFFQ69G5FAW".parse().unwrap(),
            "HS-B",
            "B",
            "task",
            "2026-01-02T00:00:00Z",
            "2026-01-02T00:00:00Z",
        );
        let s = ticket_flow(&[a, b]);
        assert_eq!((s.total, s.open, s.completed), (2, 1, 1));
        assert_eq!(s.throughput_by_day["2026-01-03"], 1);
        assert_eq!(s.cycle_time_seconds.average, 172800.0);
        assert!(!s.historical_cumulative_flow_available);
    }
}
