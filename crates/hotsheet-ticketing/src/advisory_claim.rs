//! Best-effort external-provider process claims (HS2-BPKM76).
//!
//! Provider markers are evidence of intent, never an exclusive lease. Every reader
//! reconciles all markers for one qualified ticket before selecting work. A malformed
//! marker or two live operations makes automatic dispatch unsafe.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::provider::TicketRef;

/// A provider-native marker event. `provider_order` is a stable order derived
/// from complete issue-property keys, never the writer's wall clock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisoryClaimEvent {
    pub ticket: TicketRef,
    pub worker: String,
    pub operation_id: String,
    pub provider_order: u64,
    pub kind: AdvisoryClaimEventKind,
}

/// Validate machine-supplied intent before writing a provider property.
/// A caller retries with order zero; only complete provider reads fill order.
pub fn valid_write_event(event: &AdvisoryClaimEvent, ticket: &TicketRef, now_unix_ms: i64) -> bool {
    let identity = |value: &str| {
        !value.is_empty()
            && value.len() <= 128
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
            })
    };
    event.ticket == *ticket
        && event.provider_order == 0
        && identity(&event.worker)
        && identity(&event.operation_id)
        && match &event.kind {
            AdvisoryClaimEventKind::Claim { expires_unix_ms }
            | AdvisoryClaimEventKind::Renew { expires_unix_ms } => *expires_unix_ms > now_unix_ms,
            AdvisoryClaimEventKind::Release => true,
        }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AdvisoryClaimEventKind {
    Claim { expires_unix_ms: i64 },
    Renew { expires_unix_ms: i64 },
    Release,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisoryClaim {
    pub worker: String,
    pub operation_id: String,
    pub expires_unix_ms: i64,
}

/// A projection is unsafe for dispatch when markers are malformed or two
/// independent operations remain live. `claims` is sorted by operation id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisoryClaimProjection {
    pub claims: Vec<AdvisoryClaim>,
    pub conflicted: bool,
    pub invalid_markers: usize,
}

/// Reconcile one qualified ticket. Expiry uses an injected clock; the caller's
/// skew allowance delays expiry, rather than risking premature takeover.
pub fn reconcile_claims(
    ticket: &TicketRef,
    events: &[AdvisoryClaimEvent],
    now_unix_ms: i64,
    skew_allowance_ms: i64,
) -> AdvisoryClaimProjection {
    // Different events at the same provider order are ambiguous and must not
    // be resolved by arrival order.
    let mut ordered = BTreeMap::new();
    let mut invalid_markers = 0;
    for event in events {
        if &event.ticket != ticket
            || event.worker.trim().is_empty()
            || event.operation_id.trim().is_empty()
            || event.provider_order == 0
        {
            invalid_markers += 1;
            continue;
        }
        match ordered.insert(event.provider_order, event) {
            Some(previous) if previous != event => {
                invalid_markers += 1;
                ordered.insert(event.provider_order, previous);
            }
            _ => {}
        }
    }
    // Release is a tombstone for this operation id. A delayed renew cannot
    // resurrect it, even if provider delivery or property enumeration reorders
    // the events.
    let released_operations = ordered
        .values()
        .filter(|event| matches!(&event.kind, AdvisoryClaimEventKind::Release))
        .map(|event| event.operation_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut by_operation: BTreeMap<&str, (&str, Option<i64>, bool)> = BTreeMap::new();
    for event in ordered.values() {
        let current = by_operation.get(event.operation_id.as_str()).copied();
        match (current, &event.kind) {
            (None, AdvisoryClaimEventKind::Claim { expires_unix_ms }) if *expires_unix_ms > 0 => {
                by_operation.insert(
                    &event.operation_id,
                    (&event.worker, Some(*expires_unix_ms), false),
                );
            }
            (
                Some((worker, Some(expiry), false)),
                AdvisoryClaimEventKind::Claim { expires_unix_ms },
            ) if worker == event.worker && *expires_unix_ms == expiry => {}
            (
                Some((worker, Some(expiry), false)),
                AdvisoryClaimEventKind::Renew { expires_unix_ms },
            ) if worker == event.worker && *expires_unix_ms >= expiry => {
                by_operation.insert(&event.operation_id, (worker, Some(*expires_unix_ms), false));
            }
            (Some((worker, _, false)), AdvisoryClaimEventKind::Release)
                if worker == event.worker =>
            {
                by_operation.insert(&event.operation_id, (worker, None, true));
            }
            (Some((worker, None, true)), AdvisoryClaimEventKind::Release)
                if worker == event.worker => {}
            _ => invalid_markers += 1,
        }
    }
    let allowance = skew_allowance_ms.max(0);
    let claims = by_operation
        .into_iter()
        .filter_map(|(operation_id, (worker, expiry, _))| {
            if released_operations.contains(operation_id) {
                return None;
            }
            let expires_unix_ms = expiry?;
            (expires_unix_ms.saturating_add(allowance) > now_unix_ms).then(|| AdvisoryClaim {
                worker: worker.into(),
                operation_id: operation_id.into(),
                expires_unix_ms,
            })
        })
        .collect::<Vec<_>>();
    AdvisoryClaimProjection {
        conflicted: invalid_markers > 0 || claims.len() > 1,
        claims,
        invalid_markers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket() -> TicketRef {
        TicketRef {
            connection_id: "synthetic-source".into(),
            native_id: "17".into(),
        }
    }

    fn event(
        worker: &str,
        operation_id: &str,
        order: u64,
        kind: AdvisoryClaimEventKind,
    ) -> AdvisoryClaimEvent {
        AdvisoryClaimEvent {
            ticket: ticket(),
            worker: worker.into(),
            operation_id: operation_id.into(),
            provider_order: order,
            kind,
        }
    }

    #[test]
    fn hs2_bpkm76_concurrent_live_claims_are_conflicted() {
        let events = [
            event(
                "worker-a",
                "op-a",
                1,
                AdvisoryClaimEventKind::Claim {
                    expires_unix_ms: 5_000,
                },
            ),
            event(
                "worker-b",
                "op-b",
                2,
                AdvisoryClaimEventKind::Claim {
                    expires_unix_ms: 5_000,
                },
            ),
        ];
        let projection = reconcile_claims(&ticket(), &events, 1_000, 0);
        assert!(projection.conflicted);
        assert_eq!(projection.claims.len(), 2);
    }

    #[test]
    fn hs2_bpkm76_renew_release_and_out_of_order_replay() {
        let events = [
            event("worker-a", "op-a", 3, AdvisoryClaimEventKind::Release),
            event(
                "worker-a",
                "op-a",
                1,
                AdvisoryClaimEventKind::Claim {
                    expires_unix_ms: 2_000,
                },
            ),
            event(
                "worker-a",
                "op-a",
                2,
                AdvisoryClaimEventKind::Renew {
                    expires_unix_ms: 6_000,
                },
            ),
        ];
        assert!(
            reconcile_claims(&ticket(), &events, 1_000, 0)
                .claims
                .is_empty()
        );
        assert_eq!(
            reconcile_claims(&ticket(), &events[..2], 1_000, 0)
                .claims
                .len(),
            0
        );
    }

    #[test]
    fn hs2_bpkm76_release_is_terminal_and_duplicate_order_conflicts() {
        let events = [
            event(
                "worker-a",
                "op-a",
                1,
                AdvisoryClaimEventKind::Claim {
                    expires_unix_ms: 2_000,
                },
            ),
            event("worker-a", "op-a", 2, AdvisoryClaimEventKind::Release),
            event(
                "worker-a",
                "op-a",
                3,
                AdvisoryClaimEventKind::Renew {
                    expires_unix_ms: 8_000,
                },
            ),
            event(
                "worker-b",
                "op-b",
                1,
                AdvisoryClaimEventKind::Claim {
                    expires_unix_ms: 8_000,
                },
            ),
        ];
        let projection = reconcile_claims(&ticket(), &events, 1_000, 0);
        assert!(projection.claims.is_empty());
        assert!(projection.conflicted);
        assert_eq!(projection.invalid_markers, 2);
    }

    #[test]
    fn hs2_bpkm76_replayed_same_marker_is_idempotent() {
        let events = [
            event(
                "worker-a",
                "op-a",
                1,
                AdvisoryClaimEventKind::Claim {
                    expires_unix_ms: 2_000,
                },
            ),
            event(
                "worker-a",
                "op-a",
                2,
                AdvisoryClaimEventKind::Claim {
                    expires_unix_ms: 2_000,
                },
            ),
            event(
                "worker-a",
                "op-a",
                3,
                AdvisoryClaimEventKind::Renew {
                    expires_unix_ms: 3_000,
                },
            ),
            event(
                "worker-a",
                "op-a",
                4,
                AdvisoryClaimEventKind::Renew {
                    expires_unix_ms: 3_000,
                },
            ),
            event("worker-a", "op-a", 5, AdvisoryClaimEventKind::Release),
            event("worker-a", "op-a", 6, AdvisoryClaimEventKind::Release),
        ];
        let projection = reconcile_claims(&ticket(), &events, 1_000, 0);
        assert_eq!(projection.invalid_markers, 0);
        assert!(!projection.conflicted);
        assert!(projection.claims.is_empty());
    }

    #[test]
    fn hs2_bpkm76_expiry_respects_skew_and_invalid_markers_fail_closed() {
        let mut invalid = event(
            "worker-b",
            "op-a",
            2,
            AdvisoryClaimEventKind::Renew {
                expires_unix_ms: 10_000,
            },
        );
        invalid.ticket.native_id = "other".into();
        let events = [
            event(
                "worker-a",
                "op-a",
                1,
                AdvisoryClaimEventKind::Claim {
                    expires_unix_ms: 2_000,
                },
            ),
            invalid,
        ];
        assert_eq!(
            reconcile_claims(&ticket(), &events[..1], 2_100, 0)
                .claims
                .len(),
            0
        );
        let projection = reconcile_claims(&ticket(), &events, 2_100, 200);
        assert_eq!(projection.claims.len(), 1);
        assert!(projection.conflicted);
        assert_eq!(projection.invalid_markers, 1);
    }
}
