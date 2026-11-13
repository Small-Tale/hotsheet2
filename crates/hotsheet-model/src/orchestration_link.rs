//! A structured link from an orchestration ticket to a ticket in a declared target
//! project (HS2-AZ15GA, docs/26 §26.3).

use serde::{Deserialize, Serialize};

/// Why the orchestration ticket refers to the target ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestrationRelation {
    /// A create was requested under `operation_id`; the target ticket may not exist yet.
    Planned,
    /// An existing target ticket the orchestration ticket follows.
    Observed,
    /// Orchestration created the target ticket under `operation_id`.
    Created,
    /// The target ticket's result was validated for this orchestration ticket.
    Validated,
}

/// `target_id` selects the declared project, `connection_id` its ticket source, and
/// `native_id` the ticket within that source. No slug, origin, path or machine id: a
/// link must resolve on any machine that binds the target.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrchestrationLink {
    pub target_id: String,
    pub connection_id: String,
    /// Absent only while a `planned` create has not yet produced its ticket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_id: Option<String>,
    pub relation: OrchestrationRelation,
    /// The idempotency key of the create, when orchestration created the ticket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
}

impl OrchestrationLink {
    /// Two links describe the same target ticket when they share the project and
    /// source and either the create operation or the native id.
    #[must_use]
    pub fn same_subject(&self, other: &Self) -> bool {
        self.target_id == other.target_id
            && self.connection_id == other.connection_id
            && ((self.operation_id.is_some() && self.operation_id == other.operation_id)
                || (self.native_id.is_some() && self.native_id == other.native_id))
    }

    /// The more complete of two links for one subject: the further relation wins and a
    /// known native id or operation id is never forgotten.
    #[must_use]
    pub fn merged_with(&self, other: &Self) -> Self {
        let (ahead, behind) = if other.relation > self.relation {
            (other, self)
        } else {
            (self, other)
        };
        Self {
            target_id: ahead.target_id.clone(),
            connection_id: ahead.connection_id.clone(),
            native_id: ahead.native_id.clone().or_else(|| behind.native_id.clone()),
            relation: ahead.relation,
            operation_id: ahead
                .operation_id
                .clone()
                .or_else(|| behind.operation_id.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(
        native_id: Option<&str>,
        relation: OrchestrationRelation,
        operation_id: Option<&str>,
    ) -> OrchestrationLink {
        OrchestrationLink {
            target_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            connection_id: "source".into(),
            native_id: native_id.map(Into::into),
            relation,
            operation_id: operation_id.map(Into::into),
        }
    }

    #[test]
    fn hs2_az15ga_subject_matches_by_operation_or_native_id_within_one_source() {
        use OrchestrationRelation::{Created, Observed, Planned};
        let planned = link(None, Planned, Some("op-1"));
        let created = link(Some("42"), Created, Some("op-1"));
        let observed = link(Some("42"), Observed, None);
        assert!(planned.same_subject(&created));
        assert!(created.same_subject(&observed));
        // Neither id in common: a planned create and an unrelated observation differ.
        assert!(!planned.same_subject(&observed));
        assert!(!planned.same_subject(&link(None, Planned, Some("op-2"))));
        // Two links without any id never match each other by accident.
        assert!(!link(None, Planned, None).same_subject(&link(None, Planned, None)));
        let mut other_source = created.clone();
        other_source.connection_id = "elsewhere".into();
        assert!(!created.same_subject(&other_source));
        let mut other_target = created.clone();
        other_target.target_id = "01ARZ3NDEKTSV4RRFFQ69G5FAW".into();
        assert!(!created.same_subject(&other_target));
    }

    #[test]
    fn hs2_az15ga_merge_keeps_the_further_relation_and_every_known_id() {
        use OrchestrationRelation::{Created, Observed, Planned, Validated};
        let planned = link(None, Planned, Some("op-1"));
        let created = link(Some("42"), Created, None);
        for merged in [planned.merged_with(&created), created.merged_with(&planned)] {
            assert_eq!(merged, link(Some("42"), Created, Some("op-1")));
        }
        assert_eq!(
            link(Some("42"), Observed, None)
                .merged_with(&link(Some("42"), Validated, None))
                .relation,
            Validated
        );
        // Wire shape: snake_case relation, absent ids omitted.
        assert_eq!(
            serde_json::to_value(&planned).unwrap(),
            serde_json::json!({"target_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","connection_id":"source",
                "relation":"planned","operation_id":"op-1"})
        );
    }
}
