//! The Job state machine, exactly the table in `docs/33-jobs.md` §33.2. Every edge not
//! listed there is rejected. Updates inside one nonterminal state are not transitions and
//! never pass through this module.

use crate::error::JobError;
use crate::model::{ProducerFamily, Status};

/// Whether `from -> to` is a listed edge for this producer family. `from` is `None` for
/// a Job that does not exist yet.
pub fn transition_allowed(from: Option<Status>, to: Status, family: ProducerFamily) -> bool {
    use Status::{Cancelled, Failed, Lost, Queued, Running, Succeeded, Waiting};
    let watch = family == ProducerFamily::Watch;
    match (from, to) {
        (None, Queued) => true,
        (Some(Queued), Running | Cancelled) => true,
        // Only a watch producer begins by observing an external condition.
        (Some(Queued), Waiting) => watch,
        (Some(Running), Waiting) | (Some(Waiting), Running) => true,
        (Some(Running | Waiting), Succeeded | Failed | Cancelled | Lost) => true,
        // Only a watch producer can reattach, because its truth is remote.
        (Some(Lost), Running | Waiting) => watch,
        _ => false,
    }
}

pub fn check_transition(
    from: Option<Status>,
    to: Status,
    family: ProducerFamily,
) -> Result<(), JobError> {
    if transition_allowed(from, to, family) {
        Ok(())
    } else {
        Err(JobError::InvalidTransition { from, to, family })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Status::{Cancelled, Failed, Lost, Queued, Running, Succeeded, Waiting};

    /// The §33.2 table written out independently of the implementation's `match`.
    fn listed(from: Option<Status>, to: Status, family: ProducerFamily) -> bool {
        let any: &[(Option<Status>, Status)] = &[
            (None, Queued),
            (Some(Queued), Running),
            (Some(Queued), Cancelled),
            (Some(Running), Waiting),
            (Some(Waiting), Running),
            (Some(Running), Succeeded),
            (Some(Running), Failed),
            (Some(Waiting), Succeeded),
            (Some(Waiting), Failed),
            (Some(Running), Cancelled),
            (Some(Waiting), Cancelled),
            (Some(Running), Lost),
            (Some(Waiting), Lost),
        ];
        let watch_only: &[(Option<Status>, Status)] = &[
            (Some(Queued), Waiting),
            (Some(Lost), Running),
            (Some(Lost), Waiting),
        ];
        any.contains(&(from, to))
            || (family == ProducerFamily::Watch && watch_only.contains(&(from, to)))
    }

    #[test]
    fn every_pair_matches_the_documented_table() {
        let mut allowed = 0;
        let mut rejected = 0;
        let froms = std::iter::once(None).chain(Status::ALL.into_iter().map(Some));
        for from in froms {
            for to in Status::ALL {
                for family in ProducerFamily::ALL {
                    let expected = listed(from, to, family);
                    assert_eq!(
                        transition_allowed(from, to, family),
                        expected,
                        "{from:?} -> {to:?} for {family}"
                    );
                    assert_eq!(check_transition(from, to, family).is_ok(), expected);
                    if expected {
                        allowed += 1;
                    } else {
                        rejected += 1;
                    }
                }
            }
        }
        // 8 sources x 7 targets x 3 families. 13 shared edges plus 3 watch-only edges.
        assert_eq!(allowed, 13 * 3 + 3);
        assert_eq!(allowed + rejected, 8 * 7 * 3);
    }

    #[test]
    fn terminal_states_never_resume() {
        for from in [Succeeded, Failed, Cancelled] {
            for to in Status::ALL {
                for family in ProducerFamily::ALL {
                    assert!(!transition_allowed(Some(from), to, family));
                }
            }
        }
        for family in [ProducerFamily::Owned, ProducerFamily::Reported] {
            for to in Status::ALL {
                assert!(!transition_allowed(Some(Lost), to, family));
            }
        }
    }

    #[test]
    fn a_rejected_edge_names_both_ends() {
        let error = check_transition(Some(Succeeded), Running, ProducerFamily::Owned).unwrap_err();
        assert_eq!(
            error.to_string(),
            "job transition succeeded -> running is not allowed for owned producers"
        );
        let error = check_transition(None, Running, ProducerFamily::Watch).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("job transition new -> running")
        );
    }
}
