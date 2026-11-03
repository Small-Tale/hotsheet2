//! macOS Mach bootstrap namespace health (HS2-CY01TZ).
//!
//! Every process inherits its Mach bootstrap port from the process that launched it. When the
//! launching context is torn down, a long-lived detached broker can keep a dead bootstrap port.
//! Each terminal it spawns then inherits that dead namespace. FSEvents, DNS, Keychain,
//! opendirectoryd (user lookup), sysmond and WindowServer lookups all fail with
//! `MACH_SEND_INVALID_DEST`. That looks like sandbox, network, or credential trouble, so the
//! broker probes one cheap lookup and reports the namespace state explicitly.

use serde::{Deserialize, Serialize};

/// `MACH_SEND_INVALID_DEST`: the bootstrap port is null or a dead name.
pub const MACH_SEND_INVALID_DEST: i32 = 0x1000_0003;

/// The service probed. opendirectoryd's libinfo endpoint exists on every macOS release and a
/// lookup neither starts a service nor needs a privilege.
pub const PROBE_SERVICE: &str = "com.apple.system.opendirectoryd.libinfo";

/// State of this process's Mach bootstrap namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum BootstrapHealth {
    /// The namespace answered the lookup.
    Live,
    /// The bootstrap port is null or dead: macOS system services are unreachable.
    Dead { code: i32 },
    /// The lookup failed for another reason. Callers do not refuse work on this result.
    Indeterminate { code: i32 },
    /// Not macOS: there is no Mach bootstrap namespace to probe.
    Unsupported,
}

impl BootstrapHealth {
    pub fn is_dead(self) -> bool {
        matches!(self, Self::Dead { .. })
    }

    /// Classify a `bootstrap_look_up` return code.
    pub fn from_lookup_code(code: i32) -> Self {
        match code {
            0 => Self::Live,
            MACH_SEND_INVALID_DEST => Self::Dead { code },
            code => Self::Indeterminate { code },
        }
    }
}

/// Explains the dead state and names the recovery. Shared by the broker's refusal and the
/// server's adoption warning so both read the same.
pub const DEAD_BOOTSTRAP_MESSAGE: &str = "terminal broker's macOS bootstrap namespace is dead \
(MACH_SEND_INVALID_DEST), so new terminals could not reach FSEvents, DNS, Keychain or \
WindowServer (HS2-CY01TZ). Stop the broker with `hotsheet-cli serve --stop \
--kill-all-terminals` (this ends retained terminals), then relaunch Hot Sheet from the Dock \
or Finder";

/// Probe this process's bootstrap namespace with one lookup.
#[cfg(target_os = "macos")]
pub fn probe() -> BootstrapHealth {
    BootstrapHealth::from_lookup_code(macos::look_up(PROBE_SERVICE))
}

/// Probe this process's bootstrap namespace with one lookup.
#[cfg(not(target_os = "macos"))]
pub fn probe() -> BootstrapHealth {
    BootstrapHealth::Unsupported
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::CString;

    type MachPort = u32;
    type KernReturn = i32;

    unsafe extern "C" {
        static bootstrap_port: MachPort;
        fn bootstrap_look_up(
            bp: MachPort,
            name: *const libc::c_char,
            sp: *mut MachPort,
        ) -> KernReturn;
        fn mach_task_self() -> MachPort;
        fn mach_port_deallocate(task: MachPort, name: MachPort) -> KernReturn;
    }

    /// Look up `service` on the inherited bootstrap port and release any right received.
    pub(super) fn look_up(service: &str) -> i32 {
        let Ok(name) = CString::new(service) else {
            return -1;
        };
        let mut port: MachPort = 0;
        // SAFETY: `bootstrap_port` is libSystem's process-global bootstrap right, read once.
        // `name` is a valid NUL-terminated string and `port` a valid out-pointer.
        let code = unsafe { bootstrap_look_up(bootstrap_port, name.as_ptr(), &mut port) };
        if code == 0 && port != 0 {
            // SAFETY: a successful lookup returned one send right this function owns.
            unsafe { mach_port_deallocate(mach_task_self(), port) };
        }
        code
    }
}

/// Launch-time hook that gives a spawned child a null bootstrap port, reproducing the dead
/// namespace exactly (`bootstrap_look_up` then returns `MACH_SEND_INVALID_DEST`). Tests only.
#[doc(hidden)]
#[cfg(target_os = "macos")]
pub fn null_bootstrap_for_child(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    const TASK_BOOTSTRAP_PORT: i32 = 4;
    unsafe extern "C" {
        fn mach_task_self() -> u32;
        fn task_set_special_port(task: u32, which: i32, port: u32) -> i32;
    }
    // SAFETY: runs in the forked child before exec; it only makes one async-signal-safe Mach
    // call on the child's own task.
    unsafe {
        command.pre_exec(|| {
            if task_set_special_port(mach_task_self(), TASK_BOOTSTRAP_PORT, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::other("task_set_special_port failed"))
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_cy01tz_classifies_lookup_codes() {
        assert_eq!(BootstrapHealth::from_lookup_code(0), BootstrapHealth::Live);
        assert_eq!(
            BootstrapHealth::from_lookup_code(MACH_SEND_INVALID_DEST),
            BootstrapHealth::Dead {
                code: MACH_SEND_INVALID_DEST
            }
        );
        // An unknown service name (BOOTSTRAP_UNKNOWN_SERVICE) means the namespace answered.
        assert_eq!(
            BootstrapHealth::from_lookup_code(1102),
            BootstrapHealth::Indeterminate { code: 1102 }
        );
        assert!(!BootstrapHealth::Indeterminate { code: 1102 }.is_dead());
    }

    #[test]
    fn hs2_cy01tz_health_round_trips_as_tagged_json() {
        let dead = BootstrapHealth::Dead {
            code: MACH_SEND_INVALID_DEST,
        };
        let json = serde_json::to_string(&dead).unwrap();
        assert_eq!(json, r#"{"state":"dead","code":268435459}"#);
        assert_eq!(
            serde_json::from_str::<BootstrapHealth>(&json).unwrap(),
            dead
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn hs2_cy01tz_this_test_process_has_a_live_namespace() {
        // A command sandbox may deny the lookup (Indeterminate); only a dead port fails here.
        assert!(!probe().is_dead(), "{:?}", probe());
    }
}
