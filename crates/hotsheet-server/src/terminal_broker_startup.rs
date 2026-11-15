//! How long the server waits for a terminal broker it has just started (HS2-KCM23W).
//!
//! A broker is a freshly installed or upgraded executable, and on macOS the first launch of
//! such a file waits in a host-wide assessment queue: HS2-04D6DW measured 3.6 s under a launch
//! flood against 0.19 s idle. A fixed 2 s window therefore reported healthy, still-starting
//! brokers as failed. The wait now follows the spawned process instead of a fixed count:
//!
//! - every start gets the original grace window of [`GRACE_ATTEMPTS`] probes, so an exited
//!   contender can still adopt a winner that is about to accept (HS2-C8J6WS);
//! - after that, waiting continues only while the spawned process is still running, up to
//!   [`STARTUP_CEILING`];
//! - a process that has exited while the socket is confirmed absent fails at once.

use std::time::Duration;

/// Delay between two startup probes.
pub(super) const STARTUP_POLL: Duration = Duration::from_millis(50);

/// Probes every start gets regardless of the process state: the original 2 s window.
pub(super) const GRACE_ATTEMPTS: u32 = 40;

/// The longest the server waits for a broker whose process is still running. About eight
/// times the 3.6 s first-launch delay measured under a 300-copy launch flood (HS2-04D6DW),
/// which leaves room for a first Gatekeeper assessment of a freshly downloaded build, while
/// still bounding a process that is alive but never binds its socket.
pub(super) const STARTUP_CEILING: Duration = Duration::from_secs(30);

/// The bounds of one startup wait; tests inject shorter ceilings.
#[derive(Debug, Clone, Copy)]
pub(super) struct StartupWindow {
    pub(super) grace_attempts: u32,
    pub(super) ceiling: Duration,
}

pub(super) const BROKER_STARTUP: StartupWindow = StartupWindow {
    grace_attempts: GRACE_ATTEMPTS,
    ceiling: STARTUP_CEILING,
};

/// What the server can observe about the process it started, with the diagnostic text the
/// failure message reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ContenderState {
    Running(String),
    Exited(String),
    /// The state cannot be read (no process to observe, or `wait` failed): grace window only.
    Unknown(String),
}

impl ContenderState {
    pub(super) fn describe(&self) -> &str {
        match self {
            Self::Running(text) | Self::Exited(text) | Self::Unknown(text) => text,
        }
    }
}

/// A process the server started to host its broker.
pub(super) trait StartupContender {
    fn state(&mut self) -> ContenderState;
}

/// An in-process broker or a caller that has no process to observe: the grace window only,
/// exactly the behavior before HS2-KCM23W.
impl StartupContender for () {
    fn state(&mut self) -> ContenderState {
        ContenderState::Unknown("no process to observe".to_owned())
    }
}

/// Whether to probe again after `attempts` probes that did not reach a healthy broker.
/// `absent` means the last probe confirmed that nothing owns the socket.
pub(super) fn keep_waiting(
    window: StartupWindow,
    attempts: u32,
    elapsed: Duration,
    contender: &ContenderState,
    absent: bool,
) -> bool {
    match contender {
        // The process is gone and nobody owns the socket: nothing can still come up.
        ContenderState::Exited(_) if absent => false,
        _ if attempts < window.grace_attempts => true,
        ContenderState::Running(_) => elapsed < window.ceiling,
        ContenderState::Exited(_) | ContenderState::Unknown(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: StartupWindow = BROKER_STARTUP;

    fn running() -> ContenderState {
        ContenderState::Running("child still running".into())
    }

    fn exited() -> ContenderState {
        ContenderState::Exited("child exited".into())
    }

    fn unknown() -> ContenderState {
        ContenderState::Unknown("no process".into())
    }

    #[test]
    fn hs2_kcm23w_a_running_broker_is_waited_for_past_the_grace_window_until_the_ceiling() {
        let past_grace = Duration::from_secs(4);
        assert!(keep_waiting(WINDOW, 80, past_grace, &running(), true));
        assert!(keep_waiting(WINDOW, 80, past_grace, &running(), false));
        assert!(!keep_waiting(
            WINDOW,
            600,
            STARTUP_CEILING,
            &running(),
            true
        ));
    }

    #[test]
    fn hs2_kcm23w_an_exited_broker_with_an_absent_socket_fails_at_once() {
        assert!(!keep_waiting(WINDOW, 1, Duration::ZERO, &exited(), true));
    }

    #[test]
    fn hs2_kcm23w_an_exited_contender_keeps_only_the_grace_window_while_a_winner_owns_the_socket() {
        assert!(keep_waiting(WINDOW, 1, Duration::ZERO, &exited(), false));
        assert!(keep_waiting(
            WINDOW,
            GRACE_ATTEMPTS - 1,
            Duration::ZERO,
            &exited(),
            false
        ));
        assert!(!keep_waiting(
            WINDOW,
            GRACE_ATTEMPTS,
            Duration::ZERO,
            &exited(),
            false
        ));
    }

    #[test]
    fn hs2_kcm23w_an_unobservable_contender_keeps_the_original_window() {
        assert!(keep_waiting(
            WINDOW,
            GRACE_ATTEMPTS - 1,
            Duration::ZERO,
            &unknown(),
            true
        ));
        assert!(!keep_waiting(
            WINDOW,
            GRACE_ATTEMPTS,
            Duration::ZERO,
            &unknown(),
            true
        ));
    }

    #[test]
    fn hs2_kcm23w_the_ceiling_comfortably_exceeds_the_measured_first_launch_delay() {
        let measured_first_launch = Duration::from_millis(3_600);
        assert!(STARTUP_CEILING >= measured_first_launch * 8);
        assert_eq!(STARTUP_POLL * GRACE_ATTEMPTS, Duration::from_secs(2));
    }
}
