//! Bounded advisory operation identity (HS2-0QEPRW).
//!
//! The timestamp is part of the operation ID, so a delayed same-ID event cannot
//! extend that operation past its fixed horizon. This is a validity rule, not a
//! provider lock or a claim that Jira writes are conditional.

use super::advisory_claim::{AdvisoryClaimEvent, AdvisoryClaimEventKind};
use crate::provider::TicketRef;

pub const MAX_OPERATION_HORIZON_MS: i64 = 24 * 60 * 60 * 1000;
pub const MAX_CLOCK_SKEW_MS: i64 = 60_000;
/// Cleanup waits an additional skew window so a bounded provider clock rollback
/// cannot make a deleted operation live again.
pub const RECLAIM_AFTER_ISSUE_MS: i64 = MAX_OPERATION_HORIZON_MS + 2 * MAX_CLOCK_SKEW_MS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V2EventState {
    Current,
    Inert,
}

/// None is a legacy v1 ID. A malformed v2 prefix is never legacy fallback.
pub fn issued_at_ms(operation_id: &str) -> Result<Option<i64>, &'static str> {
    let Some(rest) = operation_id.strip_prefix("v2:") else {
        return Ok(None);
    };
    let (timestamp, nonce) = rest.split_once(':').ok_or("malformed v2 operation id")?;
    if timestamp.len() != 13
        || !timestamp.bytes().all(|byte| byte.is_ascii_digit())
        || !(16..=80).contains(&nonce.len())
        || !nonce
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err("malformed v2 operation id");
    }
    let issued = timestamp
        .parse::<i64>()
        .map_err(|_| "invalid v2 issue time")?;
    if issued <= 0 || issued.checked_add(RECLAIM_AFTER_ISSUE_MS).is_none() {
        return Err("invalid v2 issue time");
    }
    Ok(Some(issued))
}

/// Validate before an event is projected or treated as old enough to reclaim.
pub fn classify(
    event: &AdvisoryClaimEvent,
    provider_now_ms: i64,
) -> Result<Option<V2EventState>, &'static str> {
    let Some(issued) = issued_at_ms(&event.operation_id)? else {
        return Ok(None);
    };
    if event.worker.is_empty()
        || event.worker.len() > 128
        || !event
            .worker
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err("invalid v2 worker identity");
    }
    if provider_now_ms <= 0 || issued > provider_now_ms.saturating_add(MAX_CLOCK_SKEW_MS) {
        return Err("v2 operation id is ahead of provider time");
    }
    if let AdvisoryClaimEventKind::Claim { expires_unix_ms }
    | AdvisoryClaimEventKind::Renew { expires_unix_ms } = &event.kind
        && (*expires_unix_ms <= issued || *expires_unix_ms > issued + MAX_OPERATION_HORIZON_MS)
    {
        return Err("v2 expiry is outside its operation horizon");
    }
    if provider_now_ms >= issued + MAX_OPERATION_HORIZON_MS + MAX_CLOCK_SKEW_MS {
        Ok(Some(V2EventState::Inert))
    } else {
        Ok(Some(V2EventState::Current))
    }
}

/// A retry or offline replay may arrive later than issuance while its bounded
/// expiry is still future. The operation ID cannot make any event live after
/// its fixed horizon, regardless of delivery delay.
pub fn valid_write(event: &AdvisoryClaimEvent, provider_now_ms: i64) -> bool {
    matches!(
        classify(event, provider_now_ms),
        Ok(Some(V2EventState::Current))
    )
}

/// Only fully valid v2 metadata can be reclaimed. The caller must separately
/// prove a complete issue-property scan, a checked Jira Date/local clock pair,
/// and delete the exact matched key.
pub fn reclaimable(event: &AdvisoryClaimEvent, ticket: &TicketRef, provider_now_ms: i64) -> bool {
    if &event.ticket != ticket || event.provider_order == 0 {
        return false;
    }
    let Ok(Some(issued)) = issued_at_ms(&event.operation_id) else {
        return false;
    };
    matches!(
        classify(event, provider_now_ms),
        Ok(Some(V2EventState::Inert))
    ) && provider_now_ms >= issued + RECLAIM_AFTER_ISSUE_MS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::TicketRef;

    fn event(issued: i64, kind: AdvisoryClaimEventKind) -> AdvisoryClaimEvent {
        AdvisoryClaimEvent {
            ticket: TicketRef {
                connection_id: "jira".into(),
                native_id: "TEST-1".into(),
            },
            worker: "worker-a".into(),
            operation_id: format!("v2:{issued:013}:0123456789abcdef"),
            provider_order: 1,
            kind,
        }
    }

    #[test]
    fn hs2_0qeprw_late_same_id_marker_is_inert_after_terminal_horizon() {
        let issued = 1_800_000_000_000;
        let claim = event(
            issued,
            AdvisoryClaimEventKind::Claim {
                expires_unix_ms: issued + MAX_OPERATION_HORIZON_MS,
            },
        );
        let after = issued + MAX_OPERATION_HORIZON_MS + MAX_CLOCK_SKEW_MS;
        assert_eq!(classify(&claim, after), Ok(Some(V2EventState::Inert)));
        assert!(!valid_write(&claim, after));
        assert!(!reclaimable(&claim, &claim.ticket, after));
        assert!(reclaimable(
            &claim,
            &claim.ticket,
            issued + RECLAIM_AFTER_ISSUE_MS
        ));
        let wrong_ticket = TicketRef {
            connection_id: "other-source".into(),
            native_id: "TEST-1".into(),
        };
        assert!(!reclaimable(
            &claim,
            &wrong_ticket,
            issued + RECLAIM_AFTER_ISSUE_MS
        ));
        // A provider clock rollback by the full allowed skew still lands at
        // the inert boundary after cleanup becomes eligible.
        assert_eq!(
            classify(&claim, issued + RECLAIM_AFTER_ISSUE_MS - MAX_CLOCK_SKEW_MS),
            Ok(Some(V2EventState::Inert))
        );
    }

    #[test]
    fn hs2_0qeprw_rejects_future_and_horizon_extending_claims() {
        let issued = 1_800_000_000_000;
        let claim = event(
            issued,
            AdvisoryClaimEventKind::Claim {
                expires_unix_ms: issued + MAX_OPERATION_HORIZON_MS,
            },
        );
        assert!(!valid_write(&claim, issued - MAX_CLOCK_SKEW_MS - 1));
        assert!(valid_write(&claim, issued + MAX_CLOCK_SKEW_MS + 1));
        assert!(!valid_write(
            &claim,
            issued + MAX_OPERATION_HORIZON_MS + MAX_CLOCK_SKEW_MS
        ));
        let beyond = event(
            issued,
            AdvisoryClaimEventKind::Renew {
                expires_unix_ms: issued + MAX_OPERATION_HORIZON_MS + 1,
            },
        );
        assert!(classify(&beyond, issued).is_err());
        let mut malformed = claim;
        malformed.operation_id = "v2:bad".into();
        assert!(issued_at_ms(&malformed.operation_id).is_err());
        malformed.operation_id = format!("v2:{issued:013}:0123456789abcdef");
        malformed.worker = "bad worker".into();
        assert!(
            classify(
                &malformed,
                issued + MAX_OPERATION_HORIZON_MS + MAX_CLOCK_SKEW_MS
            )
            .is_err()
        );
    }
}
