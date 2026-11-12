//! The dynamic step tree and its weighted rollup (`docs/33-jobs.md` §33.1).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::error::JobError;
use crate::model::{MAX_TITLE, MAX_TOKEN, text};

pub(crate) const MAX_STEPS: usize = 512;
pub(crate) const MAX_DEPTH: usize = 8;
const MAX_UNIT: usize = 32;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Active,
    Succeeded,
    Failed,
    Cancelled,
    Skipped,
}

/// One node. `id` is stable within the Job; `weight` is its share of its parent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub id: String,
    pub label: String,
    pub status: StepStatus,
    pub done: Option<u64>,
    /// Absent means indeterminate.
    pub total: Option<u64>,
    pub unit: Option<String>,
    pub weight: u32,
    pub children: Vec<Step>,
}

impl Step {
    /// A pending leaf of weight 1.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            status: StepStatus::Pending,
            done: None,
            total: None,
            unit: None,
            weight: 1,
            children: Vec::new(),
        }
    }
}

/// A step reported with a parent id instead of nested children, as a CLI or MCP
/// producer sends it. [`tree_from_flat`] builds and validates the tree.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlatStep {
    pub id: String,
    pub parent: Option<String>,
    pub label: String,
    pub status: StepStatus,
    pub done: Option<u64>,
    pub total: Option<u64>,
    pub unit: Option<String>,
    pub weight: u32,
}

/// `fraction` is within `0.0..=1.0`. `indeterminate` is true when an active step without
/// a total contributes, so the fraction is a lower bound.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Progress {
    pub fraction: f64,
    pub indeterminate: bool,
}

/// Reject invalid totals, duplicate ids, zero weights and oversized trees.
pub fn validate_steps(steps: &[Step]) -> Result<(), JobError> {
    let mut seen = BTreeSet::new();
    validate_level(steps, 1, &mut seen)
}

fn validate_level<'a>(
    steps: &'a [Step],
    depth: usize,
    seen: &mut BTreeSet<&'a str>,
) -> Result<(), JobError> {
    if depth > MAX_DEPTH {
        return Err(JobError::Invalid("steps.depth"));
    }
    for step in steps {
        validate_fields(
            &step.id,
            &step.label,
            step.done,
            step.total,
            step.unit.as_deref(),
            step.weight,
        )?;
        if !seen.insert(step.id.as_str()) {
            return Err(JobError::Invalid("steps.duplicate_id"));
        }
        if seen.len() > MAX_STEPS {
            return Err(JobError::Invalid("steps.count"));
        }
        validate_level(&step.children, depth + 1, seen)?;
    }
    Ok(())
}

fn validate_fields(
    id: &str,
    label: &str,
    done: Option<u64>,
    total: Option<u64>,
    unit: Option<&str>,
    weight: u32,
) -> Result<(), JobError> {
    crate::model::token("steps.id", id)?;
    debug_assert!(id.len() <= MAX_TOKEN);
    text("steps.label", label, MAX_TITLE)?;
    if let Some(unit) = unit {
        text("steps.unit", unit, MAX_UNIT)?;
    }
    if weight == 0 {
        return Err(JobError::Invalid("steps.weight"));
    }
    match (done, total) {
        (_, Some(0)) => Err(JobError::Invalid("steps.total")),
        (Some(done), Some(total)) if done > total => Err(JobError::Invalid("steps.done")),
        _ => Ok(()),
    }
}

/// Build a tree from parent-linked steps, keeping report order among siblings. Unknown
/// parents, duplicate ids and cycles are rejected.
pub fn tree_from_flat(flat: Vec<FlatStep>) -> Result<Vec<Step>, JobError> {
    if flat.len() > MAX_STEPS {
        return Err(JobError::Invalid("steps.count"));
    }
    let mut ids = BTreeSet::new();
    for step in &flat {
        if !ids.insert(step.id.clone()) {
            return Err(JobError::Invalid("steps.duplicate_id"));
        }
    }
    let mut children: BTreeMap<Option<String>, Vec<usize>> = BTreeMap::new();
    for (index, step) in flat.iter().enumerate() {
        if let Some(parent) = &step.parent
            && !ids.contains(parent)
        {
            return Err(JobError::Invalid("steps.parent"));
        }
        children.entry(step.parent.clone()).or_default().push(index);
    }
    let mut placed = 0;
    let tree = build(&flat, &children, None, &mut placed);
    // A cycle has no root, so its members are never reached from the top level.
    if placed != flat.len() {
        return Err(JobError::Invalid("steps.cycle"));
    }
    validate_steps(&tree)?;
    Ok(tree)
}

fn build(
    flat: &[FlatStep],
    children: &BTreeMap<Option<String>, Vec<usize>>,
    parent: Option<&str>,
    placed: &mut usize,
) -> Vec<Step> {
    let Some(indexes) = children.get(&parent.map(str::to_owned)) else {
        return Vec::new();
    };
    indexes
        .iter()
        .map(|&index| {
            let step = &flat[index];
            *placed += 1;
            Step {
                id: step.id.clone(),
                label: step.label.clone(),
                status: step.status,
                done: step.done,
                total: step.total,
                unit: step.unit.clone(),
                weight: step.weight,
                children: build(flat, children, Some(&step.id), placed),
            }
        })
        .collect()
}

/// Weighted progress of sibling steps; `None` for an empty list.
pub fn rollup(steps: &[Step]) -> Option<Progress> {
    if steps.is_empty() {
        return None;
    }
    let mut weight_sum = 0.0;
    let mut weighted = 0.0;
    let mut indeterminate = false;
    for step in steps {
        let progress = step_progress(step);
        let weight = f64::from(step.weight.max(1));
        weight_sum += weight;
        weighted += weight * progress.fraction;
        indeterminate |= progress.indeterminate;
    }
    Some(Progress {
        fraction: (weighted / weight_sum).clamp(0.0, 1.0),
        indeterminate,
    })
}

/// One step's own progress. A failed or cancelled step keeps its last measured fraction
/// instead of reading as complete.
pub fn step_progress(step: &Step) -> Progress {
    if matches!(step.status, StepStatus::Succeeded | StepStatus::Skipped) {
        return Progress {
            fraction: 1.0,
            indeterminate: false,
        };
    }
    if let Some(progress) = rollup(&step.children) {
        return progress;
    }
    match (step.done, step.total) {
        (Some(done), Some(total)) if total > 0 => Progress {
            // Counts stay far below 2^52, so the conversion is exact.
            fraction: (done.min(total) as f64) / (total as f64),
            indeterminate: false,
        },
        _ => Progress {
            fraction: 0.0,
            indeterminate: step.status == StepStatus::Active,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: &str, status: StepStatus, done: Option<u64>, total: Option<u64>) -> Step {
        Step {
            status,
            done,
            total,
            ..Step::new(id, id)
        }
    }

    fn flat(id: &str, parent: Option<&str>) -> FlatStep {
        FlatStep {
            id: id.into(),
            parent: parent.map(Into::into),
            label: id.into(),
            status: StepStatus::Pending,
            done: None,
            total: None,
            unit: None,
            weight: 1,
        }
    }

    #[test]
    fn empty_tree_has_no_progress() {
        assert_eq!(rollup(&[]), None);
    }

    #[test]
    fn weights_scale_each_child() {
        let mut heavy = leaf("a", StepStatus::Succeeded, None, None);
        heavy.weight = 3;
        let light = leaf("b", StepStatus::Pending, None, None);
        let progress = rollup(&[heavy, light]).unwrap();
        assert!((progress.fraction - 0.75).abs() < 1e-9);
        assert!(!progress.indeterminate);
    }

    #[test]
    fn nested_children_roll_up_through_their_parent() {
        let mut parent = leaf("p", StepStatus::Active, None, None);
        parent.children = vec![
            leaf("c1", StepStatus::Succeeded, None, None),
            leaf("c2", StepStatus::Active, Some(1), Some(4)),
        ];
        let sibling = leaf("s", StepStatus::Pending, None, None);
        let progress = rollup(&[parent, sibling]).unwrap();
        // Parent = (1 + 0.25) / 2 = 0.625; with the pending sibling, 0.3125.
        assert!((progress.fraction - 0.3125).abs() < 1e-9);
        assert!(!progress.indeterminate);
    }

    #[test]
    fn active_child_without_total_is_partly_indeterminate() {
        let steps = [
            leaf("a", StepStatus::Succeeded, None, None),
            leaf("b", StepStatus::Active, Some(7), None),
        ];
        let progress = rollup(&steps).unwrap();
        assert!((progress.fraction - 0.5).abs() < 1e-9);
        assert!(progress.indeterminate);
        // A pending step without a total is simply not started.
        let pending = [leaf("c", StepStatus::Pending, None, None)];
        assert!(!rollup(&pending).unwrap().indeterminate);
    }

    #[test]
    fn failed_and_cancelled_steps_keep_their_measured_fraction() {
        for status in [StepStatus::Failed, StepStatus::Cancelled] {
            let progress = rollup(&[leaf("a", status, Some(3), Some(10))]).unwrap();
            assert!((progress.fraction - 0.3).abs() < 1e-9, "{status:?}");
            let unmeasured = rollup(&[leaf("b", status, None, None)]).unwrap();
            assert!(unmeasured.fraction.abs() < 1e-9);
            assert!(!unmeasured.indeterminate);
        }
        let mut parent = leaf("p", StepStatus::Failed, None, None);
        parent.children = vec![
            leaf("c1", StepStatus::Succeeded, None, None),
            leaf("c2", StepStatus::Failed, None, None),
        ];
        assert!((rollup(&[parent]).unwrap().fraction - 0.5).abs() < 1e-9);
    }

    #[test]
    fn skipped_counts_as_complete() {
        let progress = rollup(&[leaf("a", StepStatus::Skipped, None, Some(5))]).unwrap();
        assert!((progress.fraction - 1.0).abs() < 1e-9);
    }

    #[test]
    fn invalid_totals_weights_and_ids_are_rejected() {
        let cases: Vec<(Step, &str)> = vec![
            (
                leaf("a", StepStatus::Active, Some(1), Some(0)),
                "steps.total",
            ),
            (
                leaf("a", StepStatus::Active, Some(5), Some(4)),
                "steps.done",
            ),
            (
                Step {
                    weight: 0,
                    ..Step::new("a", "a")
                },
                "steps.weight",
            ),
            (Step::new("a/b", "a"), "steps.id"),
            (Step::new("", "a"), "steps.id"),
            (Step::new("a", " "), "steps.label"),
            (Step::new("a", "line\nbreak"), "steps.label"),
            (
                Step {
                    unit: Some(String::new()),
                    ..Step::new("a", "a")
                },
                "steps.unit",
            ),
        ];
        for (step, field) in cases {
            match validate_steps(&[step]) {
                Err(JobError::Invalid(found)) => assert_eq!(found, field),
                other => panic!("expected {field}, got {other:?}"),
            }
        }
    }

    #[test]
    fn duplicate_ids_are_rejected_anywhere_in_the_tree() {
        let mut parent = Step::new("a", "a");
        parent.children = vec![Step::new("b", "b")];
        let duplicate = Step::new("b", "again");
        assert!(matches!(
            validate_steps(&[parent, duplicate]),
            Err(JobError::Invalid("steps.duplicate_id"))
        ));
    }

    #[test]
    fn depth_and_count_are_bounded() {
        let mut deep = Step::new("d0", "d");
        for level in 1..=MAX_DEPTH {
            let mut parent = Step::new(format!("d{level}"), "d");
            parent.children = vec![deep];
            deep = parent;
        }
        assert!(matches!(
            validate_steps(&[deep]),
            Err(JobError::Invalid("steps.depth"))
        ));
        let many: Vec<Step> = (0..=MAX_STEPS)
            .map(|index| Step::new(format!("s{index}"), "s"))
            .collect();
        assert!(matches!(
            validate_steps(&many),
            Err(JobError::Invalid("steps.count"))
        ));
        assert!(validate_steps(&many[..MAX_STEPS]).is_ok());
    }

    #[test]
    fn flat_steps_build_a_tree_in_report_order() {
        let tree = tree_from_flat(vec![
            flat("child-2", Some("root")),
            flat("root", None),
            flat("child-1", Some("root")),
            flat("other", None),
        ])
        .unwrap();
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].id, "root");
        let children: Vec<&str> = tree[0].children.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(children, ["child-2", "child-1"]);
        assert_eq!(tree[1].id, "other");
    }

    #[test]
    fn flat_cycles_unknown_parents_and_duplicates_are_rejected() {
        let cycle = vec![flat("a", Some("b")), flat("b", Some("a")), flat("c", None)];
        assert!(matches!(
            tree_from_flat(cycle),
            Err(JobError::Invalid("steps.cycle"))
        ));
        let own_parent = vec![flat("a", Some("a"))];
        assert!(matches!(
            tree_from_flat(own_parent),
            Err(JobError::Invalid("steps.cycle"))
        ));
        let orphan = vec![flat("a", Some("missing"))];
        assert!(matches!(
            tree_from_flat(orphan),
            Err(JobError::Invalid("steps.parent"))
        ));
        let duplicate = vec![flat("a", None), flat("a", None)];
        assert!(matches!(
            tree_from_flat(duplicate),
            Err(JobError::Invalid("steps.duplicate_id"))
        ));
    }
}
