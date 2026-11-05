//! Raw host readings (HS2-DTAVQA): the injected [`HostSource`] the sampler asks once per tick,
//! the real [`SystemSource`], and the pure parsers of each platform tool's output.
//!
//! A signal the platform has no reader for is [`Probe::Unsupported`]; a reader that failed this
//! tick (a missing tool, an unreadable file, unexpected output) is [`Probe::Unavailable`]. Neither
//! is ever replaced by a guessed value. macOS reads `statvfs`, `sysctl`, `ps`, `pmset`, `ioreg`,
//! and `route`; Linux reads `statvfs`, `/proc`, `/sys`, and `ps`; Windows reports every signal
//! unsupported until a native reader exists (HS2-MEQ46T).

#![cfg_attr(windows, allow(dead_code))] // HS2-MEQ46T: Windows has no readers yet.

use super::Level;

/// One reading of one signal.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Probe<T> {
    /// This platform has no reader for the signal.
    #[cfg_attr(target_os = "macos", allow(dead_code))] // HS2-DTAVQA: macOS reads every signal.
    Unsupported,
    /// The reader failed this tick.
    Unavailable,
    Value(T),
}

impl<T> Probe<T> {
    fn from_option(value: Option<T>) -> Self {
        value.map_or(Self::Unavailable, Self::Value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiskReading {
    pub(crate) free_bytes: u64,
    pub(crate) total_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct MemoryReading {
    /// The OS memory-pressure level (macOS `kern.memorystatus_vm_pressure_level`, Linux PSI).
    pub(crate) pressure: Option<Level>,
    pub(crate) swap_used_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CpuReading {
    /// Whole-machine utilisation, 0 to 100.
    pub(crate) busy_percent: f64,
    /// The busiest process's executable name (never its arguments or path).
    pub(crate) top_process: Option<String>,
    /// That process's share of one core, as `ps` reports it (may exceed 100).
    pub(crate) top_process_percent: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct PowerReading {
    pub(crate) on_battery: Option<bool>,
    pub(crate) battery_percent: Option<u64>,
    pub(crate) lid_closed: Option<bool>,
    /// Whether some process holds a system sleep-prevention assertion.
    pub(crate) sleep_prevented: Option<bool>,
    /// The idle system-sleep timer in minutes; `0` means the system never idle-sleeps.
    pub(crate) idle_sleep_minutes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NetworkReading {
    /// Whether a default route exists. No packet is sent to find out.
    pub(crate) reachable: bool,
    pub(crate) interface: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThermalReading {
    pub(crate) throttled: bool,
    /// macOS `CPU_Speed_Limit` (100 = unthrottled) when the OS records one.
    pub(crate) cpu_speed_limit: Option<u64>,
}

/// One sample of every signal.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HostReading {
    pub(crate) disk: Probe<DiskReading>,
    pub(crate) memory: Probe<MemoryReading>,
    pub(crate) cpu: Probe<CpuReading>,
    pub(crate) power: Probe<PowerReading>,
    pub(crate) network: Probe<NetworkReading>,
    pub(crate) thermal: Probe<ThermalReading>,
}

impl HostReading {
    /// A reading in which no signal is supported.
    #[cfg_attr(target_os = "macos", allow(dead_code))] // HS2-DTAVQA: macOS reads every signal.
    pub(crate) fn unsupported() -> Self {
        Self {
            disk: Probe::Unsupported,
            memory: Probe::Unsupported,
            cpu: Probe::Unsupported,
            power: Probe::Unsupported,
            network: Probe::Unsupported,
            thermal: Probe::Unsupported,
        }
    }
}

/// Where the sampler gets its readings. Tests inject a scripted source.
pub(crate) trait HostSource: Send {
    fn sample(&mut self) -> HostReading;
}

/// The real host.
pub(crate) struct SystemSource {
    #[cfg_attr(not(unix), allow(dead_code))] // HS2-MEQ46T: Windows has no disk reader yet.
    disk_path: std::path::PathBuf,
    /// Linux `/proc/stat` busy and total jiffies from the previous tick.
    #[cfg(target_os = "linux")]
    last_cpu: Option<(u64, u64)>,
}

impl SystemSource {
    /// Sample the volume holding `disk_path` (the primary store root).
    pub(crate) fn new(disk_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            disk_path: disk_path.into(),
            #[cfg(target_os = "linux")]
            last_cpu: None,
        }
    }
}

impl HostSource for SystemSource {
    fn sample(&mut self) -> HostReading {
        platform_sample(self)
    }
}

/// Run a tool and return its stdout, or `None` when it cannot run or fails.
#[cfg_attr(not(any(target_os = "macos", target_os = "linux")), allow(dead_code))] // HS2-MEQ46T
fn run(program: &str, args: &[&str]) -> Option<String> {
    let output = hotsheet_ticketing::git::launch(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(unix)]
fn statvfs_disk(path: &std::path::Path) -> Option<DiskReading> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is a valid NUL-terminated string and `stat` is a writable statvfs buffer.
    let result = unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    // SAFETY: statvfs returned 0, so it initialised the buffer.
    let stat = unsafe { stat.assume_init() };
    #[allow(clippy::useless_conversion)] // HS2-DTAVQA: statvfs field widths differ by platform.
    let block = u64::from(stat.f_frsize);
    #[allow(clippy::useless_conversion)] // HS2-DTAVQA: statvfs field widths differ by platform.
    let (available, blocks) = (u64::from(stat.f_bavail), u64::from(stat.f_blocks));
    Some(DiskReading {
        free_bytes: available.saturating_mul(block),
        total_bytes: blocks.saturating_mul(block),
    })
}

#[cfg_attr(not(any(target_os = "macos", target_os = "linux")), allow(dead_code))] // HS2-MEQ46T
fn logical_cpus() -> f64 {
    std::thread::available_parallelism().map_or(1.0, |count| count.get() as f64)
}

#[cfg(target_os = "macos")]
fn platform_sample(source: &mut SystemSource) -> HostReading {
    let memory = {
        let pressure = run("sysctl", &["-n", "kern.memorystatus_vm_pressure_level"])
            .and_then(|text| macos::parse_pressure_level(&text));
        let swap =
            run("sysctl", &["-n", "vm.swapusage"]).and_then(|text| macos::parse_swapusage(&text));
        if pressure.is_none() && swap.is_none() {
            Probe::Unavailable
        } else {
            Probe::Value(MemoryReading {
                pressure,
                swap_used_bytes: swap,
            })
        }
    };
    let power = {
        let battery = run("pmset", &["-g", "batt"]).map(|text| macos::parse_pmset_batt(&text));
        let assertions = run("pmset", &["-g", "assertions"])
            .and_then(|text| macos::parse_sleep_assertions(&text));
        let idle_sleep = run("pmset", &["-g"]).and_then(|text| macos::parse_pmset_sleep(&text));
        let lid = run("ioreg", &["-r", "-k", "AppleClamshellState", "-d", "1"])
            .and_then(|text| macos::parse_clamshell(&text));
        battery.map_or(Probe::Unavailable, |(on_battery, battery_percent)| {
            Probe::Value(PowerReading {
                on_battery,
                battery_percent,
                lid_closed: lid,
                sleep_prevented: assertions,
                idle_sleep_minutes: idle_sleep,
            })
        })
    };
    HostReading {
        disk: Probe::from_option(statvfs_disk(&source.disk_path)),
        memory,
        cpu: Probe::from_option(
            run("ps", &["-A", "-o", "%cpu=,comm="])
                .and_then(|text| parse_ps(&text, logical_cpus())),
        ),
        power,
        network: macos_default_route(),
        thermal: Probe::from_option(
            run("pmset", &["-g", "therm"]).and_then(|t| macos::parse_therm(&t)),
        ),
    }
}

/// `route -n get default`: a route is reachable; only the kernel's "not in table" answer means
/// there is none. Any other failure (a sandbox, a missing tool) is unavailable, not offline.
#[cfg(target_os = "macos")]
fn macos_default_route() -> Probe<NetworkReading> {
    let Ok(output) = hotsheet_ticketing::git::launch("route")
        .args(["-n", "get", "default"])
        .stdin(std::process::Stdio::null())
        .output()
    else {
        return Probe::Unavailable;
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Probe::from_option(macos::route_verdict(
        output.status.success(),
        &stdout,
        &stderr,
    ))
}

#[cfg(target_os = "linux")]
fn platform_sample(source: &mut SystemSource) -> HostReading {
    let read = |path: &str| std::fs::read_to_string(path).ok();
    let memory = {
        let pressure =
            read("/proc/pressure/memory").and_then(|text| linux::parse_proc_pressure(&text));
        let swap = read("/proc/meminfo").and_then(|text| linux::parse_meminfo_swap(&text));
        if pressure.is_none() && swap.is_none() {
            Probe::Unavailable
        } else {
            Probe::Value(MemoryReading {
                pressure,
                swap_used_bytes: swap,
            })
        }
    };
    let cpu = read("/proc/stat")
        .and_then(|text| linux::parse_proc_stat(&text))
        .and_then(|now| {
            let previous = source.last_cpu.replace(now)?;
            let busy = now.0.saturating_sub(previous.0) as f64;
            let total = now.1.saturating_sub(previous.1) as f64;
            let top = run("ps", &["-A", "-o", "%cpu=,comm="])
                .and_then(|text| parse_ps(&text, logical_cpus()));
            (total > 0.0).then(|| CpuReading {
                busy_percent: (busy / total * 100.0).clamp(0.0, 100.0),
                top_process: top.as_ref().and_then(|top| top.top_process.clone()),
                top_process_percent: top.and_then(|top| top.top_process_percent),
            })
        });
    let power = linux::linux_power(
        std::path::Path::new("/sys/class/power_supply"),
        std::path::Path::new("/proc/acpi/button/lid"),
    );
    HostReading {
        disk: Probe::from_option(statvfs_disk(&source.disk_path)),
        memory,
        cpu: Probe::from_option(cpu),
        power: power.map_or(Probe::Unsupported, Probe::Value),
        network: read("/proc/net/route").map_or(Probe::Unavailable, |text| {
            let interface = linux::parse_proc_net_route(&text);
            Probe::Value(NetworkReading {
                reachable: interface.is_some(),
                interface,
            })
        }),
        // Linux exposes trip points, not a throttling verdict (HS2-MEQ46T).
        thermal: Probe::Unsupported,
    }
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn platform_sample(source: &mut SystemSource) -> HostReading {
    HostReading {
        disk: Probe::from_option(statvfs_disk(&source.disk_path)),
        ..HostReading::unsupported()
    }
}

#[cfg(not(unix))]
fn platform_sample(_source: &mut SystemSource) -> HostReading {
    HostReading::unsupported()
}

// ---- pure parsers ------------------------------------------------------------------------

/// `ps -A -o %cpu=,comm=`: whole-machine busy share and the busiest process's executable name.
#[cfg_attr(not(any(target_os = "macos", target_os = "linux")), allow(dead_code))] // HS2-MEQ46T
pub(crate) fn parse_ps(text: &str, logical_cpus: f64) -> Option<CpuReading> {
    let mut total = 0.0;
    let mut top: Option<(f64, &str)> = None;
    let mut rows = 0;
    for line in text.lines() {
        let line = line.trim();
        let Some((percent, command)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(percent) = percent.parse::<f64>() else {
            continue;
        };
        rows += 1;
        total += percent;
        if top.is_none_or(|(best, _)| percent > best) {
            top = Some((percent, command.trim()));
        }
    }
    if rows == 0 {
        return None;
    }
    let name = top.map(|(_, command)| {
        command
            .rsplit('/')
            .next()
            .unwrap_or(command)
            .trim()
            .to_string()
    });
    Some(CpuReading {
        busy_percent: (total / logical_cpus.max(1.0)).clamp(0.0, 100.0),
        top_process: name.filter(|name| !name.is_empty()),
        top_process_percent: top.map(|(percent, _)| percent),
    })
}

/// macOS tool output. Parsed and unit-tested on every host; read only on macOS.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // HS2-DTAVQA: tested on every host.
pub(crate) mod macos {
    use super::{Level, ThermalReading};

    /// `sysctl -n kern.memorystatus_vm_pressure_level`: 1 normal, 2 warn, 4 critical.
    pub(crate) fn parse_pressure_level(text: &str) -> Option<Level> {
        match text.trim() {
            "1" => Some(Level::Normal),
            "2" => Some(Level::Warning),
            "4" => Some(Level::Critical),
            _ => None,
        }
    }

    /// A size such as `1334.38M` in bytes.
    fn parse_size(text: &str) -> Option<u64> {
        let text = text.trim();
        let (number, unit) = text.split_at(text.find(|c: char| c.is_ascii_alphabetic())?);
        let scale = match unit {
            "K" => 1024.0,
            "M" => 1024.0 * 1024.0,
            "G" => 1024.0 * 1024.0 * 1024.0,
            _ => return None,
        };
        let value = number.parse::<f64>().ok()?;
        (value >= 0.0).then_some((value * scale) as u64)
    }

    /// `sysctl -n vm.swapusage`: `total = 3072.00M  used = 1334.38M  free = …`.
    pub(crate) fn parse_swapusage(text: &str) -> Option<u64> {
        let rest = &text[text.find("used = ")? + "used = ".len()..];
        parse_size(rest.split_whitespace().next()?)
    }

    /// `pmset -g batt`: whether the Mac draws from its battery and the battery's charge. A desktop
    /// Mac reports AC power and no battery line.
    pub(crate) fn parse_pmset_batt(text: &str) -> (Option<bool>, Option<u64>) {
        let on_battery = if text.contains("'Battery Power'") {
            Some(true)
        } else if text.contains("'AC Power'") || text.contains("'UPS Power'") {
            Some(false)
        } else {
            None
        };
        let percent = text.lines().find_map(|line| {
            let (before, _) = line.split_once("%;")?;
            before
                .rsplit(|c: char| !c.is_ascii_digit())
                .next()?
                .parse::<u64>()
                .ok()
                .filter(|percent| *percent <= 100)
        });
        (on_battery, percent)
    }

    /// `pmset -g assertions`: whether a system-wide sleep-prevention assertion is held.
    pub(crate) fn parse_sleep_assertions(text: &str) -> Option<bool> {
        let mut seen = false;
        let mut held = false;
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let (Some(name), Some(count), None) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            if matches!(name, "PreventUserIdleSystemSleep" | "PreventSystemSleep") {
                let Ok(count) = count.parse::<u64>() else {
                    continue;
                };
                seen = true;
                held |= count > 0;
            }
        }
        seen.then_some(held)
    }

    /// `pmset -g`: the `sleep` (idle system sleep) timer in minutes.
    pub(crate) fn parse_pmset_sleep(text: &str) -> Option<u64> {
        text.lines().find_map(|line| {
            let mut parts = line.split_whitespace();
            (parts.next()? == "sleep")
                .then(|| parts.next()?.parse::<u64>().ok())
                .flatten()
        })
    }

    /// `ioreg -r -k AppleClamshellState`: whether the lid is closed. A Mac without a lid has no key.
    pub(crate) fn parse_clamshell(text: &str) -> Option<bool> {
        text.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "\"AppleClamshellState\"").then(|| value.trim() == "Yes")
        })
    }

    /// `pmset -g therm`: whether the OS records thermal or performance throttling.
    pub(crate) fn parse_therm(text: &str) -> Option<ThermalReading> {
        let speed_limit = text.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "CPU_Speed_Limit")
                .then(|| value.trim().parse::<u64>().ok())
                .flatten()
        });
        let warning_recorded = text.lines().any(|line| {
            let line = line.to_ascii_lowercase();
            line.contains("warning level") && !line.contains("no ") && !line.contains("error")
        });
        let clear = text.contains("No thermal warning level has been recorded");
        if speed_limit.is_none() && !warning_recorded && !clear {
            return None;
        }
        Some(ThermalReading {
            throttled: warning_recorded || speed_limit.is_some_and(|limit| limit < 100),
            cpu_speed_limit: speed_limit,
        })
    }

    /// Classify a `route -n get default` run: success is a route, "not in table" is none, and
    /// anything else is unknown (`None`).
    pub(crate) fn route_verdict(
        success: bool,
        stdout: &str,
        stderr: &str,
    ) -> Option<super::NetworkReading> {
        if success {
            return Some(super::NetworkReading {
                reachable: true,
                interface: parse_route_interface(stdout),
            });
        }
        (stderr.contains("not in table") || stdout.contains("not in table")).then_some(
            super::NetworkReading {
                reachable: false,
                interface: None,
            },
        )
    }

    /// `route -n get default`: the default route's interface.
    pub(crate) fn parse_route_interface(text: &str) -> Option<String> {
        text.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == "interface").then(|| value.trim().to_string())
        })
    }
}

/// Linux `/proc` and `/sys` files. Parsed and unit-tested on every host; read only on Linux.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))] // HS2-DTAVQA: tested on every host.
pub(crate) mod linux {
    use super::{Level, PowerReading};

    /// Linux `/proc/pressure/memory`: `full avg60` at or above 10 is critical, `some avg60` at or
    /// above 20 a warning.
    pub(crate) fn parse_proc_pressure(text: &str) -> Option<Level> {
        let avg60 = |kind: &str| {
            text.lines()
                .find(|line| line.starts_with(kind))?
                .split_whitespace()
                .find_map(|field| field.strip_prefix("avg60="))?
                .parse::<f64>()
                .ok()
        };
        let some = avg60("some")?;
        Some(if avg60("full").unwrap_or(0.0) >= 10.0 {
            Level::Critical
        } else if some >= 20.0 {
            Level::Warning
        } else {
            Level::Normal
        })
    }

    /// Linux `/proc/meminfo`: swap in use, in bytes.
    pub(crate) fn parse_meminfo_swap(text: &str) -> Option<u64> {
        let kib = |name: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(name))?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        };
        Some(
            kib("SwapTotal:")?
                .saturating_sub(kib("SwapFree:")?)
                .saturating_mul(1024),
        )
    }

    /// Linux `/proc/stat`: the aggregate `cpu` line's busy and total jiffies.
    pub(crate) fn parse_proc_stat(text: &str) -> Option<(u64, u64)> {
        let line = text.lines().find(|line| line.starts_with("cpu "))?;
        let values: Vec<u64> = line
            .split_whitespace()
            .skip(1)
            .filter_map(|value| value.parse().ok())
            .collect();
        if values.len() < 4 {
            return None;
        }
        let total: u64 = values.iter().sum();
        // idle + iowait are not busy.
        let idle = values[3] + values.get(4).copied().unwrap_or(0);
        Some((total.saturating_sub(idle), total))
    }

    /// Linux `/proc/net/route`: the interface of an up default route (destination `00000000`).
    pub(crate) fn parse_proc_net_route(text: &str) -> Option<String> {
        text.lines().skip(1).find_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let (interface, destination, flags) = (fields.first()?, fields.get(1)?, fields.get(3)?);
            let up = u32::from_str_radix(flags, 16).ok()? & 0x1 != 0;
            (*destination == "00000000" && up).then(|| (*interface).to_string())
        })
    }

    /// Linux power from `/sys/class/power_supply` and the ACPI lid. `None` when the machine reports
    /// no power supply at all. Sleep assertions and the idle timer have no Linux reader.
    pub(crate) fn linux_power(
        supplies: &std::path::Path,
        lids: &std::path::Path,
    ) -> Option<PowerReading> {
        let read = |path: std::path::PathBuf| {
            std::fs::read_to_string(path)
                .ok()
                .map(|text| text.trim().to_string())
        };
        let mut any = false;
        let mut mains_online = None;
        let mut battery_percent = None;
        let mut discharging = None;
        for entry in std::fs::read_dir(supplies).ok()?.flatten() {
            let path = entry.path();
            match read(path.join("type")).as_deref() {
                Some("Mains") => {
                    any = true;
                    mains_online = Some(read(path.join("online")).as_deref() == Some("1"));
                }
                Some("Battery") => {
                    any = true;
                    battery_percent =
                        read(path.join("capacity")).and_then(|value| value.parse().ok());
                    discharging = Some(read(path.join("status")).as_deref() == Some("Discharging"));
                }
                _ => {}
            }
        }
        if !any {
            return None;
        }
        let lid_closed = std::fs::read_dir(lids).ok().and_then(|entries| {
            entries
                .flatten()
                .find_map(|entry| read(entry.path().join("state")))
                .map(|state| state.contains("closed"))
        });
        Some(PowerReading {
            on_battery: match (mains_online, discharging) {
                (Some(true), _) => Some(false),
                (_, Some(discharging)) => Some(discharging),
                (Some(false), None) => None,
                (None, None) => None,
            },
            battery_percent,
            lid_closed,
            sleep_prevented: None,
            idle_sleep_minutes: None,
        })
    }
}
