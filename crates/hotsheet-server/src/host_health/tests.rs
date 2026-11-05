//! HS2-DTAVQA: parsers over captured tool output, the evaluator's transition matrix and
//! hysteresis, and the routes plus cursor replay through the real router.

use super::source::{
    self, CpuReading, DiskReading, HostReading, HostSource, MemoryReading, NetworkReading,
    PowerReading, Probe, ThermalReading,
};
use super::{Emission, HostIdentity, Level, Monitor, RECOVERY_SAMPLES, SNAPSHOT_MIN_INTERVAL_MS};
use hotsheet_ticketing::host_health::{HostHealthThresholds, default_thresholds};

const GIB: u64 = 1024 * 1024 * 1024;
const MINUTE: u64 = 60_000;
const T0: u64 = 1_793_000_000_000;

fn host() -> HostIdentity {
    HostIdentity {
        name: "studio".into(),
        os: "macos",
        arch: "aarch64",
    }
}

/// A healthy Mac on AC power: plenty of disk, no pressure, idle CPU, network up.
fn healthy() -> HostReading {
    HostReading {
        disk: Probe::Value(DiskReading {
            free_bytes: 500 * GIB,
            total_bytes: 1000 * GIB,
        }),
        memory: Probe::Value(MemoryReading {
            pressure: Some(Level::Normal),
            swap_used_bytes: Some(GIB),
        }),
        cpu: Probe::Value(CpuReading {
            busy_percent: 12.0,
            top_process: Some("node".into()),
            top_process_percent: Some(30.0),
        }),
        power: Probe::Value(PowerReading {
            on_battery: Some(false),
            battery_percent: Some(100),
            lid_closed: Some(false),
            sleep_prevented: Some(true),
            idle_sleep_minutes: Some(0),
        }),
        network: Probe::Value(NetworkReading {
            reachable: true,
            interface: Some("en0".into()),
        }),
        thermal: Probe::Value(ThermalReading {
            throttled: false,
            cpu_speed_limit: None,
        }),
    }
}

fn with_disk(free_gib: u64) -> HostReading {
    HostReading {
        disk: Probe::Value(DiskReading {
            free_bytes: free_gib * GIB,
            total_bytes: 1000 * GIB,
        }),
        ..healthy()
    }
}

fn crossings(emissions: &[Emission]) -> Vec<(&'static str, &'static str)> {
    emissions
        .iter()
        .filter(|emission| emission.id != "snapshot")
        .map(|emission| (emission.id, emission.status))
        .collect()
}

// ---- parsers -----------------------------------------------------------------------------

#[test]
fn macos_tool_output_parses_into_readings() {
    assert_eq!(
        source::macos::parse_pressure_level("1\n"),
        Some(Level::Normal)
    );
    assert_eq!(
        source::macos::parse_pressure_level("2"),
        Some(Level::Warning)
    );
    assert_eq!(
        source::macos::parse_pressure_level("4"),
        Some(Level::Critical)
    );
    assert_eq!(source::macos::parse_pressure_level("7"), None);
    assert_eq!(
        source::macos::parse_swapusage(
            "total = 3072.00M  used = 1334.38M  free = 1737.62M  (encrypted)"
        ),
        Some((1334.38 * 1024.0 * 1024.0) as u64)
    );
    assert_eq!(
        source::macos::parse_swapusage("total = 0.00M  used = 0.00M  free = 0.00M"),
        Some(0)
    );
    assert_eq!(source::macos::parse_swapusage("garbage"), None);

    let ps = " 56.0 /System/Library/Frameworks/ATS.framework/Support/fontd\n 31.3 node\n  0.0 /usr/sbin/cron\nnot-a-row\n";
    let cpu = source::parse_ps(ps, 10.0).unwrap();
    assert!((cpu.busy_percent - 8.73).abs() < 0.01, "{cpu:?}");
    assert_eq!(
        cpu.top_process.as_deref(),
        Some("fontd"),
        "basename only, no path"
    );
    assert_eq!(cpu.top_process_percent, Some(56.0));
    assert_eq!(source::parse_ps("", 8.0), None);
    // A saturated machine clamps at 100.
    assert_eq!(
        source::parse_ps("900.0 cc\n", 4.0).unwrap().busy_percent,
        100.0
    );

    let laptop_ac = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=25493603)\t100%; charged; 0:00 remaining present: true\n";
    assert_eq!(
        source::macos::parse_pmset_batt(laptop_ac),
        (Some(false), Some(100))
    );
    let laptop_battery = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1)\t17%; discharging; 0:41 remaining present: true\n";
    assert_eq!(
        source::macos::parse_pmset_batt(laptop_battery),
        (Some(true), Some(17))
    );
    assert_eq!(
        source::macos::parse_pmset_batt("Now drawing from 'AC Power'\n"),
        (Some(false), None),
        "a desktop Mac has no battery line"
    );
    assert_eq!(source::macos::parse_pmset_batt(""), (None, None));

    let assertions = "Assertion status system-wide:\n   BackgroundTask                 0\n   PreventSystemSleep             0\n   PreventUserIdleSystemSleep     1\nListed by owning process:\n   pid 37408(caffeinate): [0x1] 00:02:05 PreventUserIdleSystemSleep named: \"caffeinate\"\n";
    assert_eq!(
        source::macos::parse_sleep_assertions(assertions),
        Some(true)
    );
    assert_eq!(
        source::macos::parse_sleep_assertions(
            "   PreventSystemSleep             0\n   PreventUserIdleSystemSleep     0\n"
        ),
        Some(false)
    );
    assert_eq!(source::macos::parse_sleep_assertions("nothing"), None);

    let pmset = " standby              1\n sleep                0 (sleep prevented by caffeinate, powerd)\n displaysleep         10\n";
    assert_eq!(source::macos::parse_pmset_sleep(pmset), Some(0));
    assert_eq!(source::macos::parse_pmset_sleep(" sleep 15\n"), Some(15));
    assert_eq!(source::macos::parse_pmset_sleep(" displaysleep 10\n"), None);

    let ioreg = "      \"AppleClamshellCausesSleep\" = Yes\n      \"AppleClamshellState\" = Yes\n";
    assert_eq!(source::macos::parse_clamshell(ioreg), Some(true));
    assert_eq!(
        source::macos::parse_clamshell("      \"AppleClamshellState\" = No\n"),
        Some(false)
    );
    assert_eq!(source::macos::parse_clamshell(""), None, "no lid (desktop)");

    let clear = "Note: No thermal warning level has been recorded\nNote: No performance warning level has been recorded\n";
    assert_eq!(
        source::macos::parse_therm(clear),
        Some(ThermalReading {
            throttled: false,
            cpu_speed_limit: None
        })
    );
    let limited = "CPU_Scheduler_Limit \t= 100\nCPU_Available_CPUs \t= 8\nCPU_Speed_Limit \t= 70\n";
    assert_eq!(
        source::macos::parse_therm(limited),
        Some(ThermalReading {
            throttled: true,
            cpu_speed_limit: Some(70)
        })
    );
    assert_eq!(
        source::macos::parse_therm("Thermal warning level set to 2.\n").map(|t| t.throttled),
        Some(true)
    );
    // A sandboxed or failing pmset reports errors only: unavailable, not "normal".
    assert_eq!(
        source::macos::parse_therm(
            "Error:Failed to get thermal warning level with error code 0xe00002bc\n"
        ),
        None
    );

    let route =
        "   route to: default\ndestination: default\n    gateway: 192.168.1.1\n  interface: en0\n";
    assert_eq!(
        source::macos::parse_route_interface(route).as_deref(),
        Some("en0")
    );
    assert_eq!(
        source::macos::route_verdict(true, route, ""),
        Some(NetworkReading {
            reachable: true,
            interface: Some("en0".into())
        })
    );
    assert_eq!(
        source::macos::route_verdict(
            false,
            "",
            "route: writing to routing socket: not in table\n"
        ),
        Some(NetworkReading {
            reachable: false,
            interface: None
        }),
        "only the kernel's answer means offline"
    );
    assert_eq!(
        source::macos::route_verdict(false, "", "route: socket: Operation not permitted\n"),
        None,
        "a sandboxed or failing tool is unavailable, never offline"
    );
}

#[test]
fn linux_proc_and_sys_files_parse_into_readings() {
    let pressure = "some avg10=0.00 avg60=1.50 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
    assert_eq!(
        source::linux::parse_proc_pressure(pressure),
        Some(Level::Normal)
    );
    assert_eq!(
        source::linux::parse_proc_pressure(
            "some avg10=1 avg60=25.0 avg300=1 total=1\nfull avg10=0 avg60=2.0 avg300=0 total=0\n"
        ),
        Some(Level::Warning)
    );
    assert_eq!(
        source::linux::parse_proc_pressure(
            "some avg10=1 avg60=40.0 avg300=1 total=1\nfull avg10=0 avg60=12.0 avg300=0 total=0\n"
        ),
        Some(Level::Critical)
    );
    assert_eq!(source::linux::parse_proc_pressure(""), None);

    let meminfo = "MemTotal: 100 kB\nSwapTotal:       2048 kB\nSwapFree:        1024 kB\n";
    assert_eq!(
        source::linux::parse_meminfo_swap(meminfo),
        Some(1024 * 1024)
    );
    assert_eq!(source::linux::parse_meminfo_swap("MemTotal: 1 kB\n"), None);

    assert_eq!(
        source::linux::parse_proc_stat("cpu  10 0 10 70 10 0 0 0 0 0\ncpu0 1 1 1 1\n"),
        Some((20, 100))
    );
    assert_eq!(source::linux::parse_proc_stat("intr 1\n"), None);

    let route = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\nlo\t0000007F\t00000000\t0001\t0\t0\t0\t000000FF\neth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\n";
    assert_eq!(
        source::linux::parse_proc_net_route(route).as_deref(),
        Some("eth0")
    );
    let down = "Iface\tDestination\tGateway \tFlags\neth0\t00000000\t0101A8C0\t0002\n";
    assert_eq!(
        source::linux::parse_proc_net_route(down),
        None,
        "a down route"
    );

    let root = tempfile::tempdir().unwrap();
    let supplies = root.path().join("power_supply");
    let lids = root.path().join("lid");
    std::fs::create_dir_all(supplies.join("AC")).unwrap();
    std::fs::create_dir_all(supplies.join("BAT0")).unwrap();
    std::fs::create_dir_all(lids.join("LID0")).unwrap();
    std::fs::write(supplies.join("AC/type"), "Mains\n").unwrap();
    std::fs::write(supplies.join("AC/online"), "0\n").unwrap();
    std::fs::write(supplies.join("BAT0/type"), "Battery\n").unwrap();
    std::fs::write(supplies.join("BAT0/capacity"), "42\n").unwrap();
    std::fs::write(supplies.join("BAT0/status"), "Discharging\n").unwrap();
    std::fs::write(lids.join("LID0/state"), "state:      closed\n").unwrap();
    let power = source::linux::linux_power(&supplies, &lids).unwrap();
    assert_eq!(power.on_battery, Some(true));
    assert_eq!(power.battery_percent, Some(42));
    assert_eq!(power.lid_closed, Some(true));
    assert_eq!(
        power.sleep_prevented, None,
        "no Linux reader: unknown, not false"
    );
    std::fs::write(supplies.join("AC/online"), "1\n").unwrap();
    assert_eq!(
        source::linux::linux_power(&supplies, &lids)
            .unwrap()
            .on_battery,
        Some(false)
    );
    let empty = root.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert_eq!(
        source::linux::linux_power(&empty, &lids),
        None,
        "no supplies: unsupported"
    );
}

// ---- evaluator ---------------------------------------------------------------------------

#[test]
fn a_healthy_first_sample_is_all_normal_and_announces_one_snapshot() {
    let mut monitor = Monitor::new(host());
    let emissions = monitor.observe(healthy(), T0, &default_thresholds());
    assert_eq!(
        emissions,
        vec![Emission {
            id: "snapshot",
            status: "normal",
            reason: None
        }]
    );
    let snapshot = monitor.snapshot();
    assert_eq!(snapshot.level, Level::Normal);
    assert_eq!(snapshot.host.name, "studio");
    assert!(snapshot.sampled_at.is_some());
    assert_eq!(snapshot.signals.disk.free_bytes, Some(500 * GIB));
    assert_eq!(snapshot.signals.cpu.top_process.as_deref(), Some("node"));
    // The same coarse values again announce nothing (dark cockpit, quiet bus).
    assert!(
        monitor
            .observe(healthy(), T0 + MINUTE, &default_thresholds())
            .is_empty()
    );
}

#[test]
fn disk_crossings_escalate_at_once_and_recover_only_after_a_margin_and_three_samples() {
    // Free-space thresholds only (warn < 20 GiB, critical < 5 GiB): these synthetic jumps would
    // otherwise also fit a time-to-full projection.
    let thresholds = HostHealthThresholds {
        disk_warning_hours_to_full: 0,
        disk_critical_hours_to_full: 0,
        ..default_thresholds()
    };
    let mut monitor = Monitor::new(host());
    let mut now = T0;
    let mut step = |monitor: &mut Monitor, free: u64| {
        now += MINUTE;
        crossings(&monitor.observe(with_disk(free), now, &thresholds))
    };
    assert!(step(&mut monitor, 100).is_empty());
    assert_eq!(step(&mut monitor, 19), vec![("disk", "warning")]);
    assert_eq!(monitor.snapshot().level, Level::Warning);
    assert_eq!(
        monitor.snapshot().signals.disk.header.reason,
        Some("low_free_space")
    );
    // Normal -> critical skips nothing: escalation is immediate.
    assert_eq!(step(&mut monitor, 4), vec![("disk", "critical")]);
    // Oscillating around the critical line (5 GiB, margin to 5.5) never flaps.
    for free in [5, 4, 5, 4, 5] {
        assert!(step(&mut monitor, free).is_empty(), "flapped at {free} GiB");
    }
    assert_eq!(
        monitor.snapshot().signals.disk.header.status,
        super::Status::Critical
    );
    // Recovered past the margin: three consecutive samples before it drops to warning.
    for _ in 1..RECOVERY_SAMPLES {
        assert!(step(&mut monitor, 10).is_empty());
    }
    assert_eq!(step(&mut monitor, 10), vec![("disk", "warning")]);
    // A relapse during recovery resets the count.
    assert!(step(&mut monitor, 30).is_empty());
    assert!(step(&mut monitor, 19).is_empty());
    assert!(step(&mut monitor, 30).is_empty());
    assert!(step(&mut monitor, 30).is_empty());
    assert_eq!(step(&mut monitor, 30), vec![("disk", "normal")]);
    assert_eq!(monitor.snapshot().level, Level::Normal);
    // Empty-then-refill: it can cross again.
    assert_eq!(step(&mut monitor, 3), vec![("disk", "critical")]);
}

#[test]
fn disk_projection_fits_the_last_hour_and_calls_out_time_to_full() {
    let thresholds = default_thresholds(); // warn under 24 h, critical under 2 h
    let mut monitor = Monitor::new(host());
    // 100 GiB free, losing 2 GiB per minute (120 GiB/h): full in well under an hour.
    let mut emissions = Vec::new();
    for minute in 0..=12 {
        emissions.extend(crossings(&monitor.observe(
            with_disk(100 - 2 * minute),
            T0 + minute * MINUTE,
            &thresholds,
        )));
    }
    let disk = &monitor.snapshot().signals.disk;
    let rate = disk.consumption_bytes_per_hour.unwrap() as f64 / GIB as f64;
    assert!((rate - 120.0).abs() < 1.0, "{rate}");
    let hours = disk.hours_to_full.unwrap();
    assert!((hours - 76.0 / 120.0).abs() < 0.1, "{hours}");
    assert_eq!(emissions, vec![("disk", "critical")]);
    assert_eq!(disk.header.reason, Some("projected_full"));

    // Too short a span projects nothing.
    let mut fresh = Monitor::new(host());
    fresh.observe(with_disk(100), T0, &thresholds);
    fresh.observe(with_disk(90), T0 + MINUTE, &thresholds);
    assert_eq!(fresh.snapshot().signals.disk.hours_to_full, None);
    assert_eq!(
        fresh.snapshot().signals.disk.consumption_bytes_per_hour,
        None
    );

    // Growing free space consumes nothing; a zero projection threshold disables it.
    let mut growing = Monitor::new(host());
    for minute in 0..=12 {
        growing.observe(with_disk(100 + minute), T0 + minute * MINUTE, &thresholds);
    }
    assert_eq!(
        growing.snapshot().signals.disk.consumption_bytes_per_hour,
        Some(0)
    );
    assert_eq!(growing.snapshot().signals.disk.hours_to_full, None);
    let disabled = HostHealthThresholds {
        disk_warning_hours_to_full: 0,
        disk_critical_hours_to_full: 0,
        ..thresholds
    };
    let mut quiet = Monitor::new(host());
    let mut emitted = Vec::new();
    for minute in 0..=12 {
        emitted.extend(crossings(&quiet.observe(
            with_disk(100 - 2 * minute),
            T0 + minute * MINUTE,
            &disabled,
        )));
    }
    assert!(emitted.is_empty(), "{emitted:?}");
}

#[test]
fn cpu_must_stay_busy_for_the_sustain_window_and_a_dip_restarts_it() {
    let thresholds = HostHealthThresholds {
        cpu_sustain_minutes: 3,
        ..default_thresholds()
    };
    let busy = |percent: f64| HostReading {
        cpu: Probe::Value(CpuReading {
            busy_percent: percent,
            top_process: Some("cargo".into()),
            top_process_percent: Some(780.0),
        }),
        ..healthy()
    };
    let mut monitor = Monitor::new(host());
    let mut at = T0;
    let mut tick = |monitor: &mut Monitor, percent: f64| {
        let out = crossings(&monitor.observe(busy(percent), at, &thresholds));
        at += MINUTE;
        out
    };
    assert!(tick(&mut monitor, 95.0).is_empty()); // busy since T0
    assert!(tick(&mut monitor, 95.0).is_empty()); // 1 min
    assert!(tick(&mut monitor, 50.0).is_empty()); // dip resets
    assert!(tick(&mut monitor, 95.0).is_empty());
    assert!(tick(&mut monitor, 95.0).is_empty());
    assert!(tick(&mut monitor, 95.0).is_empty()); // 2 min
    assert_eq!(tick(&mut monitor, 95.0), vec![("cpu", "warning")]); // 3 min
    let cpu = &monitor.snapshot().signals.cpu;
    assert_eq!(cpu.top_process.as_deref(), Some("cargo"));
    assert_eq!(cpu.busy_for_seconds, Some(180));
    assert_eq!(cpu.header.reason, Some("sustained_cpu"));
    // 87% is inside the 5-point margin: still busy, no flap.
    assert!(tick(&mut monitor, 87.0).is_empty());
    for _ in 1..RECOVERY_SAMPLES {
        assert!(tick(&mut monitor, 20.0).is_empty());
    }
    assert_eq!(tick(&mut monitor, 20.0), vec![("cpu", "normal")]);
}

#[test]
fn memory_follows_os_pressure_and_swap_growth() {
    let thresholds = default_thresholds(); // swap growth warns at 1 GiB/h
    let memory = |pressure: Level, swap_gib: f64| HostReading {
        memory: Probe::Value(MemoryReading {
            pressure: Some(pressure),
            swap_used_bytes: Some((swap_gib * GIB as f64) as u64),
        }),
        ..healthy()
    };
    let mut monitor = Monitor::new(host());
    assert!(crossings(&monitor.observe(memory(Level::Normal, 1.0), T0, &thresholds)).is_empty());
    assert_eq!(
        crossings(&monitor.observe(memory(Level::Critical, 1.0), T0 + MINUTE, &thresholds)),
        vec![("memory", "critical")]
    );
    assert_eq!(
        monitor.snapshot().signals.memory.header.reason,
        Some("pressure")
    );

    // Swap growing 3 GiB/h under normal pressure warns once the fit has a 10-minute span.
    let mut swapping = Monitor::new(host());
    let mut out = Vec::new();
    for minute in 0..=12 {
        out.extend(crossings(&swapping.observe(
            memory(Level::Normal, 1.0 + 0.05 * minute as f64),
            T0 + minute * MINUTE,
            &thresholds,
        )));
    }
    assert_eq!(out, vec![("memory", "warning")]);
    let signal = &swapping.snapshot().signals.memory;
    assert_eq!(signal.header.reason, Some("swap_growth"));
    let growth = signal.swap_growth_bytes_per_hour.unwrap() as f64 / GIB as f64;
    assert!((growth - 3.0).abs() < 0.05, "{growth}");
}

#[test]
fn power_and_sleep_risk_rank_lid_battery_and_assertions() {
    let thresholds = default_thresholds(); // battery critical at 20%
    let power =
        |on_battery: bool, percent: u64, lid: bool, prevented: bool, sleep: u64| HostReading {
            power: Probe::Value(PowerReading {
                on_battery: Some(on_battery),
                battery_percent: Some(percent),
                lid_closed: Some(lid),
                sleep_prevented: Some(prevented),
                idle_sleep_minutes: Some(sleep),
            }),
            ..healthy()
        };
    let verdict = |reading: HostReading| {
        let mut monitor = Monitor::new(host());
        monitor.observe(reading, T0, &thresholds);
        let header = monitor.snapshot().signals.power.header.clone();
        (header.status, header.reason)
    };
    use super::Status::{Critical, Normal, Warning};
    assert_eq!(verdict(power(false, 100, false, true, 0)), (Normal, None));
    // Clamshell on AC with an external display is fine.
    assert_eq!(verdict(power(false, 100, true, false, 0)), (Normal, None));
    assert_eq!(
        verdict(power(false, 100, false, false, 10)),
        (Warning, Some("idle_sleep_allowed"))
    );
    assert_eq!(
        verdict(power(false, 100, false, true, 10)),
        (Normal, None),
        "an assertion holds the machine awake"
    );
    assert_eq!(
        verdict(power(true, 80, false, true, 0)),
        (Warning, Some("on_battery"))
    );
    assert_eq!(
        verdict(power(true, 20, false, true, 0)),
        (Critical, Some("battery_low"))
    );
    assert_eq!(
        verdict(power(true, 80, true, true, 0)),
        (Critical, Some("lid_closed_on_battery"))
    );
    // Unknown assertion state (Linux) never claims idle-sleep risk.
    let unknown = HostReading {
        power: Probe::Value(PowerReading {
            on_battery: Some(false),
            idle_sleep_minutes: Some(10),
            ..PowerReading::default()
        }),
        ..healthy()
    };
    assert_eq!(verdict(unknown), (Normal, None));
}

#[test]
fn network_and_thermal_callouts() {
    let thresholds = default_thresholds();
    let mut monitor = Monitor::new(host());
    monitor.observe(healthy(), T0, &thresholds);
    let offline = HostReading {
        network: Probe::Value(NetworkReading {
            reachable: false,
            interface: None,
        }),
        thermal: Probe::Value(ThermalReading {
            throttled: true,
            cpu_speed_limit: Some(60),
        }),
        ..healthy()
    };
    assert_eq!(
        crossings(&monitor.observe(offline, T0 + MINUTE, &thresholds)),
        vec![("network", "critical"), ("thermal", "warning")]
    );
    assert_eq!(monitor.snapshot().level, Level::Critical);
    assert_eq!(
        monitor.snapshot().signals.network.header.reason,
        Some("no_default_route")
    );
}

#[test]
fn unsupported_and_unavailable_signals_never_call_out_or_fake_values() {
    let thresholds = default_thresholds();
    let mut monitor = Monitor::new(host());
    let emissions = monitor.observe(HostReading::unsupported(), T0, &thresholds);
    assert_eq!(crossings(&emissions), Vec::<(&str, &str)>::new());
    let snapshot = monitor.snapshot();
    assert_eq!(snapshot.level, Level::Normal);
    for header in snapshot.signals.headers() {
        assert_eq!(header.status, super::Status::Unsupported);
        assert_eq!(header.reason, None);
    }
    assert_eq!(snapshot.signals.disk.free_bytes, None);
    assert_eq!(snapshot.signals.power.on_battery, None);

    // A failing reader does not clear a live callout at once: it needs the recovery samples.
    let mut flaky = Monitor::new(host());
    flaky.observe(with_disk(2), T0, &thresholds);
    let failed = HostReading {
        disk: Probe::Unavailable,
        ..healthy()
    };
    for sample in 1..RECOVERY_SAMPLES {
        assert!(
            crossings(&flaky.observe(failed.clone(), T0 + u64::from(sample) * MINUTE, &thresholds))
                .is_empty()
        );
    }
    assert_eq!(
        crossings(&flaky.observe(failed, T0 + 10 * MINUTE, &thresholds)),
        vec![("disk", "unavailable")]
    );
    assert_eq!(flaky.snapshot().level, Level::Normal);
}

#[test]
fn coarse_snapshot_notices_are_rate_limited_and_only_on_real_change() {
    let thresholds = default_thresholds();
    let mut monitor = Monitor::new(host());
    let snapshots = |emissions: Vec<Emission>| {
        emissions
            .into_iter()
            .filter(|emission| emission.id == "snapshot")
            .count()
    };
    assert_eq!(
        snapshots(monitor.observe(with_disk(500), T0, &thresholds)),
        1
    );
    // Changed coarse value, but inside the rate limit: held back.
    assert_eq!(
        snapshots(monitor.observe(with_disk(400), T0 + MINUTE, &thresholds)),
        0
    );
    // Past the interval with a still-different value: announced.
    assert_eq!(
        snapshots(monitor.observe(
            with_disk(400),
            T0 + SNAPSHOT_MIN_INTERVAL_MS + MINUTE,
            &thresholds
        )),
        1
    );
    // Past the interval but unchanged: quiet.
    assert_eq!(
        snapshots(monitor.observe(
            with_disk(400),
            T0 + 3 * SNAPSHOT_MIN_INTERVAL_MS,
            &thresholds
        )),
        0
    );
}

// ---- server ------------------------------------------------------------------------------

mod server {
    use super::*;
    use crate::{AppState, app};
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use hotsheet_ticketing::{FsStore, StoreMetadata};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const SECRET: &str = "host-health-secret";

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path().join("store"), &StoreMetadata::new("HS")).unwrap();
        let state = AppState::new(store, SECRET.into())
            .unwrap()
            .with_machine_home(dir.path().join("machine-home"));
        (dir, state)
    }

    fn request(method: &str, uri: &str, body: Option<&str>) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("x-hotsheet-secret", SECRET)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.map_or(Body::empty(), |body| Body::from(body.to_string())))
            .unwrap()
    }

    async fn json(
        router: &axum::Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let response = router
            .clone()
            .oneshot(request(method, uri, body))
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn the_snapshot_route_labels_the_host_and_requires_the_secret() {
        let (_dir, state) = state();
        let router = app(state.clone());
        let anonymous = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/host-health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
        // Before the first sample every signal is unavailable and the level is normal.
        let (status, before) = json(&router, "GET", "/host-health", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(before["sampled_at"], serde_json::Value::Null);
        assert_eq!(before["signals"]["disk"]["status"], "unavailable");
        assert_eq!(before["host"]["os"], std::env::consts::OS);
        assert!(!before["host"]["name"].as_str().unwrap().is_empty());

        super::super::record(&state, with_disk(3), T0, &default_thresholds());
        let (_, after) = json(&router, "GET", "/host-health", None).await;
        assert_eq!(after["level"], "critical");
        assert_eq!(after["signals"]["disk"]["status"], "critical");
        assert_eq!(after["signals"]["disk"]["reason"], "low_free_space");
        assert_eq!(after["signals"]["disk"]["free_bytes"], 3 * GIB);
        assert_eq!(after["signals"]["cpu"]["top_process"], "node");
        assert_eq!(after["signals"]["thermal"]["throttled"], false);
    }

    #[tokio::test]
    async fn crossings_are_cursored_change_events_a_long_poll_replays() {
        let (_dir, state) = state();
        let router = app(state.clone());
        let (_, start) = json(&router, "GET", "/ws/poll?timeout_ms=0", None).await;
        let cursor = start["cursor"].as_u64().unwrap();
        let thresholds = default_thresholds();
        super::super::record(&state, healthy(), T0, &thresholds);
        super::super::record(&state, with_disk(15), T0 + MINUTE, &thresholds);
        super::super::record(&state, with_disk(15), T0 + 2 * MINUTE, &thresholds); // no change
        let (_, replay) = json(
            &router,
            "GET",
            &format!("/ws/poll?since={cursor}&timeout_ms=0"),
            None,
        )
        .await;
        let events: Vec<(String, String, String, serde_json::Value)> = replay["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["kind"] == "host_health")
            .map(|event| {
                assert!(
                    event["cursor"].as_u64().unwrap() > cursor,
                    "durable, cursored"
                );
                assert_eq!(event["store"], "", "server-wide, not a store event");
                (
                    event["id"].as_str().unwrap().to_string(),
                    event["slug"].as_str().unwrap().to_string(),
                    event["kind"].as_str().unwrap().to_string(),
                    event["message"].clone(),
                )
            })
            .collect();
        assert_eq!(
            events,
            vec![
                (
                    "snapshot".into(),
                    "normal".into(),
                    "host_health".into(),
                    serde_json::Value::Null
                ),
                (
                    "disk".into(),
                    "warning".into(),
                    "host_health".into(),
                    "low_free_space".into()
                ),
            ]
        );
        // A reconnect at the latest cursor replays nothing more.
        let latest = replay["cursor"].as_u64().unwrap();
        let (_, caught_up) = json(
            &router,
            "GET",
            &format!("/ws/poll?since={latest}&timeout_ms=0"),
            None,
        )
        .await;
        assert!(caught_up["events"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn thresholds_round_trip_validate_and_feed_the_sampler() {
        let (dir, state) = state();
        let router = app(state.clone());
        let (status, initial) = json(&router, "GET", "/host-health/thresholds", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            initial["configured"]["disk_warning_free_gib"],
            serde_json::Value::Null
        );
        assert_eq!(initial["effective"], initial["defaults"]);

        let (status, saved) = json(
            &router,
            "PUT",
            "/host-health/thresholds",
            Some(r#"{"disk_warning_free_gib":600,"sample_seconds":30}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(saved["configured"]["disk_warning_free_gib"], 600);
        assert_eq!(saved["effective"]["disk_warning_free_gib"], 600);
        assert_eq!(saved["effective"]["sample_seconds"], 30);
        // Stored machine-wide, where `hotsheet-cli settings set … --scope global` writes.
        let settings = hotsheet_ticketing::Settings::with_global_home(
            state.store.root(),
            dir.path().join("machine-home"),
        );
        assert_eq!(
            settings
                .get(
                    "host_health.disk_warning_free_gib",
                    hotsheet_ticketing::Scope::Global
                )
                .unwrap(),
            Some(serde_json::json!(600))
        );
        // The sampler reads them: 500 GiB free is now a warning.
        let effective = super::super::thresholds(&state);
        assert_eq!(effective.disk_warning_free_gib, 600);
        super::super::record(&state, healthy(), T0, &effective);
        let (_, snapshot) = json(&router, "GET", "/host-health", None).await;
        assert_eq!(snapshot["signals"]["disk"]["status"], "warning");

        let (status, _) = json(
            &router,
            "PUT",
            "/host-health/thresholds",
            Some(r#"{"cpu_warning_percent":0}"#),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        let (status, _) = json(
            &router,
            "PUT",
            "/host-health/thresholds",
            Some(r#"{"disk_free":1}"#),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown names are rejected"
        );

        // Clearing returns every threshold to its default.
        let (_, cleared) = json(&router, "PUT", "/host-health/thresholds", Some("{}")).await;
        assert_eq!(cleared["effective"], cleared["defaults"]);
        assert_eq!(
            settings
                .get(
                    "host_health.disk_warning_free_gib",
                    hotsheet_ticketing::Scope::Global
                )
                .unwrap(),
            None
        );
    }

    /// A scripted source: the sampler task runs it, emits, and stops with the server.
    struct Scripted(std::sync::mpsc::Sender<()>);
    impl HostSource for Scripted {
        fn sample(&mut self) -> HostReading {
            let _ = self.0.send(());
            with_disk(1)
        }
    }

    #[tokio::test]
    async fn the_sampler_task_samples_the_injected_source_once_per_start() {
        let (_dir, state) = state();
        let (sent, received) = std::sync::mpsc::channel();
        state.spawn_host_health_sampler_with(Box::new(Scripted(sent.clone())));
        // A second start is a no-op (one sampler per server).
        state.spawn_host_health_sampler_with(Box::new(Scripted(sent)));
        let mut rx = state.events.subscribe();
        let event = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let event = rx.recv().await.unwrap();
                if event.kind == "host_health" && event.id == "disk" {
                    return event;
                }
            }
        })
        .await
        .expect("the sampler emitted a disk crossing");
        assert_eq!(event.slug, "critical");
        assert_eq!(
            received.try_iter().count(),
            1,
            "one sampler, one first sample"
        );
        state.begin_stopping();
    }
}

/// `.cargo/config.toml` keeps every spawned test server from sampling this host.
#[test]
fn cargo_test_processes_run_with_the_sampler_off() {
    assert_eq!(std::env::var(super::SAMPLER_ENV).as_deref(), Ok("off"));
}
