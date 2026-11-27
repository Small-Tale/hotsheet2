//! HS2-J8PRTV: capacity parsers over captured tool output, the on-demand reading and its
//! shared cache, and the hosted routes through the real router with scripted probes.

use super::*;
use std::sync::Arc;

const GIB: u64 = 1024 * MIB;

/// What a scripted source answers, and what it was asked.
#[derive(Default)]
struct Script {
    machine: MachineReading,
    disk: Option<(u64, u64)>,
    tools: Option<Vec<(String, bool)>>,
    resident_bytes: Option<u64>,
    machine_reads: u64,
    disk_reads: Vec<PathBuf>,
    resident_asked: Vec<Vec<u32>>,
    /// HS2-ZY4Z4G
    capabilities: Option<HostCapabilities>,
    toolchains_asked: Vec<Vec<String>>,
    /// HS2-0VTQFF: report models from the server's discovery, as the real host does.
    models_from_discovery: bool,
    models_asked: u32,
}

#[derive(Clone, Default)]
struct Scripted(Arc<Mutex<Script>>);

impl Scripted {
    /// A roomy machine: 8 GiB free of 16, 12.4% busy, 60 GiB of disk, codex installed.
    fn roomy() -> Self {
        let scripted = Self::default();
        scripted.set(|script| {
            script.machine = MachineReading {
                memory_free_bytes: Some(8 * GIB),
                memory_total_bytes: Some(16 * GIB),
                cpu_percent: Some(12.4),
            };
            script.disk = Some((60 * GIB, 500 * GIB));
            script.tools = Some(vec![("codex".into(), true), ("claude".into(), false)]);
        });
        scripted
    }
    fn set(&self, change: impl FnOnce(&mut Script)) {
        self.0.with_lock(change);
    }
    fn machine_reads(&self) -> u64 {
        self.0.with_lock(|script| script.machine_reads)
    }
}

impl CapacitySource for Scripted {
    fn machine(&mut self) -> MachineReading {
        self.0.with_lock(|script| {
            script.machine_reads += 1;
            script.machine
        })
    }
    fn disk(&mut self, path: &Path) -> Option<(u64, u64)> {
        self.0.with_lock(|script| {
            script.disk_reads.push(path.to_path_buf());
            script.disk
        })
    }
    fn resident_bytes(&mut self, pids: &[u32]) -> HashMap<u32, u64> {
        self.0.with_lock(|script| {
            script.resident_asked.push(pids.to_vec());
            pids.iter()
                .filter_map(|pid| Some((*pid, script.resident_bytes?)))
                .collect()
        })
    }
    fn tools(&mut self, _: &[PathBuf]) -> Option<Vec<(String, bool)>> {
        self.0.with_lock(|script| script.tools.clone())
    }
    fn capabilities(&mut self, _: &[PathBuf], toolchains: &[String]) -> Option<HostCapabilities> {
        self.0.with_lock(|script| {
            script.toolchains_asked.push(toolchains.to_vec());
            script.capabilities.clone()
        })
    }
    fn models(
        &mut self,
        discover: &dyn Fn() -> Vec<hotsheet_plugins::AiToolDescriptor>,
    ) -> Option<HashMap<String, Vec<String>>> {
        let from_discovery = self.0.with_lock(|script| {
            script.models_asked += 1;
            script.models_from_discovery
        });
        from_discovery.then(|| reported_models(&discover()))
    }
}

// ---- parsers -----------------------------------------------------------------------------

#[test]
fn host_health_capacity_parsers_read_memory_and_process_trees_or_report_unknown() {
    use super::source::{linux, macos, parse_ps_resident};
    let vm_stat = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
        Pages free:                                   131262.\n\
        Pages active:                                 548235.\n\
        Pages inactive:                               551123.\n\
        Pages speculative:                              4307.\n\
        Pages wired down:                             153575.\n";
    assert_eq!(
        macos::parse_vm_stat(vm_stat),
        Some((131_262 + 551_123 + 4_307) * 16_384)
    );
    // Without a speculative line the free and inactive pages still count.
    let older = "Mach Virtual Memory Statistics: (page size of 4096 bytes)\n\
        Pages free: 10.\nPages inactive: 5.\n";
    assert_eq!(macos::parse_vm_stat(older), Some(15 * 4096));
    for broken in [
        "",
        "Pages free: 10.\nPages inactive: 5.\n",
        "Mach Virtual Memory Statistics: (page size of 0 bytes)\nPages free: 1.\nPages inactive: 1.\n",
        "Mach Virtual Memory Statistics: (page size of 4096 bytes)\nPages free: 10.\n",
        "Mach Virtual Memory Statistics: (page size of 4096 bytes)\nPages free: ten.\nPages inactive: 5.\n",
    ] {
        assert_eq!(macos::parse_vm_stat(broken), None, "{broken:?}");
    }

    let meminfo = "MemTotal:       16303428 kB\nMemFree:          812340 kB\n\
        MemAvailable:    9421816 kB\nBuffers:          301220 kB\n";
    let memory = linux::parse_meminfo_memory(meminfo).unwrap();
    assert_eq!(memory.free_bytes, 9_421_816 * 1024);
    assert_eq!(memory.total_bytes, 16_303_428 * 1024);
    // A kernel without MemAvailable is unknown; MemFree is not a substitute.
    assert_eq!(
        linux::parse_meminfo_memory("MemTotal: 100 kB\nMemFree: 50 kB\n"),
        None
    );
    assert_eq!(linux::parse_meminfo_memory("garbage"), None);

    // pid, parent, resident kB: 100 has children 101 and 102; 102 has 103.
    let ps = "    1     0  22304\n  100     1   1000\n  101   100    200\n  102   100     30\n\
        103   102      4\n  200     1    500\nnot a row\n  300   abc    1\n";
    let resident = parse_ps_resident(ps, &[100, 200, 999]);
    assert_eq!(resident.get(&100), Some(&(1234 * 1024)));
    assert_eq!(resident.get(&200), Some(&(500 * 1024)));
    // A process that is not running has no entry: unknown, never zero.
    assert_eq!(resident.get(&999), None);
    assert_eq!(resident.len(), 2);
    // An inconsistent listing (a parent loop) still ends and counts each pid once.
    let looped = "10 11 1\n11 10 2\n";
    assert_eq!(parse_ps_resident(looped, &[10]).get(&10), Some(&(3 * 1024)));
    assert!(parse_ps_resident("", &[1]).is_empty());
}

/// HS2-4A5AV7: the Windows readings' pure halves, on every host.
#[test]
fn hs2_4a5av7_windows_memory_and_process_tree_readings_are_pure_and_bounded() {
    use super::source::{process_trees, sum_trees, windows};
    // MEMORYSTATUSEX ullAvailPhys and ullTotalPhys.
    let memory = windows::memory_amount(6 << 30, 16 << 30).unwrap();
    assert_eq!((memory.free_bytes, memory.total_bytes), (6 << 30, 16 << 30));
    assert_eq!(windows::memory_amount(0, 16 << 30).unwrap().free_bytes, 0);
    // No installed memory, or more available than installed, is not a reading.
    assert_eq!(windows::memory_amount(0, 0), None);
    assert_eq!(windows::memory_amount(17 << 30, 16 << 30), None);

    // GetSystemTimes deltas: kernel time includes idle time.
    assert_eq!(
        windows::cpu_delta((100, 1000, 500), (150, 1100, 500)),
        Some(50.0)
    );
    assert_eq!(windows::cpu_delta((0, 0, 0), (0, 0, 0)), None);
    assert_eq!(windows::cpu_delta((10, 10, 10), (5, 20, 20)), None);

    // Toolhelp (pid, parent) rows: 0 is the System Idle process, its own parent; 4 is
    // System. 100 has children 101 and 102; 102 has 103; 200 is a separate session.
    let edges = [
        (0, 0),
        (4, 0),
        (100, 4),
        (101, 100),
        (102, 100),
        (103, 102),
        (200, 4),
    ];
    let trees = process_trees(&edges, &[100, 200, 999]);
    let mut members = trees[&100].clone();
    members.sort_unstable();
    assert_eq!(members, [100, 101, 102, 103]);
    assert_eq!(trees[&200], [200]);
    // A root that is not running has no tree: unknown, never zero.
    assert!(!trees.contains_key(&999));
    // The self-parented idle process does not make every process its own descendant.
    assert_eq!(process_trees(&edges, &[0])[&0].len(), 7);
    let idle_only = process_trees(&[(0, 0)], &[0]);
    assert_eq!(idle_only[&0], [0]);
    // A parent loop through a reused pid ends and lists each pid once.
    let mut looped = process_trees(&[(10, 11), (11, 10)], &[10])[&10].clone();
    looped.sort_unstable();
    assert_eq!(looped, [10, 11]);

    // Working sets: a member that could not be opened counts 0 and keeps its tree.
    let sizes = [(100, 1000), (101, 200), (103, 4), (200, 500)].into();
    let totals = sum_trees(&trees, &sizes);
    assert_eq!(totals[&100], 1204);
    assert_eq!(totals[&200], 500);
    assert_eq!(totals.len(), 2);
    let unreadable = sum_trees(&process_trees(&[(7, 1)], &[7]), &HashMap::new());
    assert_eq!(unreadable[&7], 0);
    assert!(sum_trees(&HashMap::new(), &sizes).is_empty());
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
#[test]
fn host_health_capacity_reads_this_machines_memory_cpu_disk_and_own_process() {
    let mut system = SystemCapacity;
    let machine = system.machine();
    let (free, total) = (
        machine.memory_free_bytes.unwrap(),
        machine.memory_total_bytes.unwrap(),
    );
    assert!(free > 0 && free <= total, "{free} of {total}");
    assert!((0.0..=100.0).contains(&machine.cpu_percent.unwrap()));
    let (disk_free, disk_total) = system.disk(&std::env::temp_dir()).unwrap();
    assert!(disk_free <= disk_total && disk_total > 0);
    assert_eq!(system.disk(Path::new("/synthetic/absent/volume")), None);
    let own = std::process::id();
    assert!(
        system
            .resident_bytes(&[own])
            .get(&own)
            .is_some_and(|bytes| *bytes > 0)
    );
    assert!(system.resident_bytes(&[]).is_empty());
    // The registry lists each drivable tool once; nothing is executed to find out.
    let tools = system.tools(&[]).unwrap();
    let mut names: Vec<_> = tools.iter().map(|(tool, _)| tool).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), tools.len());
}

// ---- the on-demand reading ---------------------------------------------------------------

fn observe(
    capacity: &Capacity,
    root: &Path,
    sessions: Option<Vec<LiveSession>>,
) -> HostObservation {
    capacity.observe(
        root,
        &[],
        || sessions,
        || [("term-1".to_owned(), 4242)].into(),
        |tool| tool != "claude",
    )
}

#[test]
fn host_health_capacity_reading_is_taken_on_demand_and_shared_inside_the_cache_window() {
    let scripted = Scripted::roomy();
    let capacity = Capacity::with_source(Box::new(scripted.clone()));
    let root = tempfile::tempdir().unwrap();
    let inside = root.path().join("worktree");
    std::fs::create_dir(&inside).unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    // Constructing the state reads nothing.
    assert_eq!((capacity.samples(), scripted.machine_reads()), (0, 0));

    scripted.set(|script| script.resident_bytes = Some(300 * MIB + 1));
    let sessions = vec![
        LiveSession {
            id: "term-1".into(),
            tool: Some("codex".into()),
            busy: true,
            launch_cwd: Some(inside.display().to_string()),
        },
        LiveSession {
            id: "term-2".into(),
            tool: None,
            busy: false,
            launch_cwd: Some(elsewhere.path().display().to_string()),
        },
        LiveSession {
            id: "term-3".into(),
            tool: Some("codex".into()),
            busy: false,
            launch_cwd: None,
        },
    ];
    let first = observe(&capacity, root.path(), Some(sessions));
    assert_eq!(capacity.samples(), 1);
    assert!(first.sampled_at_ms.is_some());
    assert_eq!(
        (first.memory.free_mb, first.memory.total_mb),
        (Some(8192), Some(16_384))
    );
    assert_eq!(first.cpu.load_percent, Some(12));
    assert!(first.cpu.logical_cpus.is_some_and(|count| count >= 1));
    assert_eq!(
        (first.disk.free_mb, first.disk.total_mb),
        (Some(60 * 1024), Some(500 * 1024))
    );
    assert_eq!(
        first.tools,
        Some(vec![
            ToolAvailability {
                tool: "codex".into(),
                installed: true,
                configured: true,
                models: None,
            },
            ToolAvailability {
                tool: "claude".into(),
                installed: false,
                configured: false,
                models: None,
            },
        ])
    );
    let listed = first.sessions.clone().unwrap();
    assert_eq!(listed.len(), 3);
    // The journal knew one pid: that session has a footprint (rounded up), the others
    // are unknown rather than zero.
    assert_eq!(
        (listed[0].session_id.as_str(), listed[0].resident_mb),
        ("term-1", Some(301))
    );
    assert_eq!((listed[1].resident_mb, listed[2].resident_mb), (None, None));
    assert_eq!(
        [
            listed[0].on_checkout,
            listed[1].on_checkout,
            listed[2].on_checkout
        ],
        [Some(true), Some(false), None]
    );
    assert_eq!(listed[1].tool, None);
    assert_eq!(
        scripted.0.with_lock(|script| script.resident_asked.clone()),
        [vec![4242]]
    );

    // Inside the window every caller shares the reading: the scripted change is unseen
    // and the session lister is not asked again.
    scripted.set(|script| script.machine.cpu_percent = Some(99.0));
    let second = capacity.observe(
        root.path(),
        &[],
        || panic!("sessions are part of the shared reading"),
        || panic!("pids are part of the shared reading"),
        |tool| tool != "claude",
    );
    assert_eq!(second, first);
    assert_eq!((capacity.samples(), scripted.machine_reads()), (1, 1));
    // Another checkout shares the machine reading and adds only its own volume, once.
    let other = observe(&capacity, elsewhere.path(), None);
    let _ = observe(&capacity, elsewhere.path(), None);
    assert_eq!(other.cpu.load_percent, Some(12));
    assert_eq!(other.sessions.unwrap()[1].on_checkout, Some(true));
    assert_eq!(capacity.samples(), 1);
    assert_eq!(
        scripted.0.with_lock(|script| script.disk_reads.clone()),
        [root.path().to_path_buf(), elsewhere.path().to_path_buf()]
    );

    // Past the window the next request takes a new reading.
    capacity.set_cache(Duration::ZERO);
    let third = observe(&capacity, root.path(), Some(Vec::new()));
    assert_eq!((capacity.samples(), scripted.machine_reads()), (2, 2));
    assert_eq!(third.cpu.load_percent, Some(99));
    assert_eq!(third.sessions, Some(Vec::new()));
    // No session has a pid to look up, so no footprint is requested for any.
    assert!(
        scripted
            .0
            .with_lock(|script| script.resident_asked[1].is_empty())
    );
}

#[test]
fn host_health_capacity_reports_every_unmeasured_value_as_unknown() {
    let scripted = Scripted::default();
    let capacity = Capacity::with_source(Box::new(scripted.clone()));
    capacity.set_cache(Duration::ZERO);
    let root = tempfile::tempdir().unwrap();
    // Nothing measurable and the terminal host unreachable.
    let unknown = observe(&capacity, root.path(), None);
    assert!(unknown.sampled_at_ms.is_some());
    assert_eq!(unknown.memory, MemoryCapacity::default());
    assert_eq!(unknown.cpu.load_percent, None);
    assert_eq!(unknown.disk, DiskCapacity::default());
    assert_eq!((unknown.tools, unknown.sessions), (None, None));
    // A partial reading keeps what it has and guesses nothing else.
    scripted.set(|script| {
        script.machine = MachineReading {
            memory_free_bytes: Some(GIB),
            memory_total_bytes: None,
            cpu_percent: Some(f64::NAN),
        };
        script.tools = Some(Vec::new());
    });
    let partial = observe(&capacity, root.path(), Some(Vec::new()));
    assert_eq!(
        (partial.memory.free_mb, partial.memory.total_mb),
        (Some(1024), None)
    );
    assert_eq!(partial.cpu.load_percent, None);
    assert_eq!(partial.tools, Some(Vec::new()));
    // Out-of-range CPU readings are clamped, not trusted.
    scripted.set(|script| script.machine.cpu_percent = Some(140.0));
    assert_eq!(
        observe(&capacity, root.path(), None).cpu.load_percent,
        Some(100)
    );
}

// ---- server ------------------------------------------------------------------------------

mod server {
    use super::*;
    use crate::app;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use hotsheet_ticketing::checkouts::CheckoutRegistry;
    use hotsheet_ticketing::{FsStore, StoreMetadata};
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    const SECRET: &str = "capacity-secret";

    struct Fixture {
        _dir: tempfile::TempDir,
        state: AppState,
        scripted: Scripted,
        checkout: Checkout,
        project: PathBuf,
        home: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let store_path = dir.path().join("store");
        let store = FsStore::init_git(&store_path, &StoreMetadata::new("HS")).unwrap();
        let home = dir.path().join("machine-home");
        std::fs::create_dir(&home).unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let checkout = CheckoutRegistry::new(home.join("checkouts.json"))
            .register(&project, None, None, vec![store_path])
            .unwrap();
        let scripted = Scripted::roomy();
        let state = AppState::new(store, SECRET.into())
            .unwrap()
            .with_machine_home(&home)
            .with_capacity_source(Box::new(scripted.clone()));
        Fixture {
            _dir: dir,
            state,
            scripted,
            checkout,
            project,
            home,
        }
    }

    async fn call(
        router: &axum::Router,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("x-hotsheet-secret", SECRET)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(body.map_or(Body::empty(), |body| Body::from(body.to_string())))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_health_capacity_is_idle_without_requests_and_one_sample_serves_a_burst() {
        let f = fixture();
        let router = app(f.state.clone());
        let capacity = format!("/checkouts/{}/target-capacity", f.checkout.id);
        // An idle server, and one serving other routes, takes no capacity reading.
        for uri in ["/host-health", "/terminals", "/host-health/thresholds"] {
            assert_eq!(call(&router, "GET", uri, None).await.0, StatusCode::OK);
        }
        assert_eq!(f.state.capacity_samples(), 0);
        assert_eq!(f.scripted.machine_reads(), 0);
        // The route is ordinary authenticated data.
        let anonymous = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&capacity)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(f.state.capacity_samples(), 0);

        // A burst of callers shares one reading.
        let burst =
            futures_util::future::join_all((0..6).map(|_| call(&router, "GET", &capacity, None)))
                .await;
        for (status, snapshot) in &burst {
            assert_eq!(*status, StatusCode::OK, "{snapshot}");
            assert_eq!(snapshot, &burst[0].1);
        }
        assert_eq!(f.state.capacity_samples(), 1);
        assert_eq!(f.scripted.machine_reads(), 1);
        let snapshot = &burst[0].1;
        assert_eq!(snapshot["schema_version"], 1);
        assert_eq!(snapshot["checkout_id"], f.checkout.id.as_str());
        assert_eq!(snapshot["memory"], json!({"free_mb":8192,"total_mb":16384}));
        assert_eq!(snapshot["cpu"]["load_percent"], 12);
        assert_eq!(snapshot["disk"]["free_mb"], 60 * 1024);
        assert_eq!(snapshot["sessions"], json!([]));
        // HS2-YDZAFH: no namespace named and none held on this machine.
        assert_eq!(snapshot["claims"], json!([]));
        assert_eq!(
            snapshot["tools"],
            json!([{"tool":"codex","installed":true,"configured":true},
                {"tool":"claude","installed":false,"configured":true}])
        );
        // The admission answer inside the window reuses that reading too.
        let admission = format!("/checkouts/{}/target-admission", f.checkout.id);
        let (status, decision) =
            call(&router, "POST", &admission, Some(json!({"tool":"codex"}))).await;
        assert_eq!(status, StatusCode::OK, "{decision}");
        assert_eq!(decision["outcome"], "accepted");
        assert_eq!(f.state.capacity_samples(), 1);
        // Past the window a request samples again; with no request nothing does.
        let state = f.state.clone().with_capacity_cache(Duration::ZERO);
        assert_eq!(state.capacity_samples(), 1);
        call(&router, "GET", &capacity, None).await;
        assert_eq!(f.state.capacity_samples(), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_health_capacity_admission_answers_from_this_servers_own_probe_and_limits() {
        let f = fixture();
        let state = f.state.clone().with_capacity_cache(Duration::ZERO);
        let router = app(state);
        let admission = format!("/checkouts/{}/target-admission", f.checkout.id);
        let ask = |body: Value| {
            let (router, admission) = (router.clone(), admission.clone());
            async move {
                let (status, decision) = call(&router, "POST", &admission, Some(body)).await;
                assert_eq!(status, StatusCode::OK, "{decision}");
                decision
            }
        };
        let outcome = |decision: &Value| {
            (
                decision["outcome"].as_str().unwrap().to_owned(),
                decision["admitted"].as_u64().unwrap(),
                decision["reason"].as_str().map(str::to_owned),
            )
        };
        let limits = json!({"max_sessions":3,"min_free_ram_mb":2048,"max_cpu_percent":80,
            "min_free_disk_mb":5120});
        let codex = |sessions: u32| json!({"tool":"codex","sessions":sessions,"limits":limits});
        assert_eq!(outcome(&ask(codex(2)).await), ("accepted".into(), 2, None));
        assert_eq!(
            outcome(&ask(codex(5)).await),
            ("reduced".into(), 3, Some("session_cap_reached".into()))
        );
        // Low RAM: one megabyte short of the floor plus one 1024 MB session.
        f.scripted.set(|script| {
            script.machine.memory_free_bytes = Some((2048 + 1023) * MIB);
        });
        assert_eq!(
            outcome(&ask(codex(1)).await),
            ("refused".into(), 0, Some("below_ram_floor".into()))
        );
        // High load.
        f.scripted.set(|script| {
            script.machine.memory_free_bytes = Some(8 * GIB);
            script.machine.cpu_percent = Some(80.6);
        });
        assert_eq!(
            outcome(&ask(codex(1)).await),
            ("refused".into(), 0, Some("above_cpu_ceiling".into()))
        );
        // Low disk.
        f.scripted.set(|script| {
            script.machine.cpu_percent = Some(5.0);
            script.disk = Some((5119 * MIB, 500 * GIB));
        });
        assert_eq!(
            outcome(&ask(codex(1)).await),
            ("refused".into(), 0, Some("below_disk_floor".into()))
        );
        // A missing tool, and one the checkout opted out of.
        f.scripted.set(|script| {
            script.disk = Some((60 * GIB, 500 * GIB));
            script.tools = Some(vec![("codex".into(), true), ("claude".into(), true)]);
        });
        assert_eq!(
            outcome(&ask(json!({"tool":"absent"})).await),
            ("refused".into(), 0, Some("tool_not_installed".into()))
        );
        hotsheet_ticketing::Settings::for_project(&f.project)
            .set(
                "enabled_plugins",
                json!(["codex"]),
                hotsheet_ticketing::Scope::Shared,
            )
            .unwrap();
        assert_eq!(
            outcome(&ask(json!({"tool":"claude"})).await),
            ("refused".into(), 0, Some("tool_not_configured".into()))
        );
        assert_eq!(outcome(&ask(codex(1)).await).0, "accepted");
        // Unknown values refuse, naming what was unknown; the owner may allow them.
        f.scripted.set(|script| {
            script.machine.memory_free_bytes = None;
            script.machine.cpu_percent = None;
        });
        let unknown = ask(codex(1)).await;
        assert_eq!(
            outcome(&unknown),
            ("refused".into(), 0, Some("capacity_unknown".into()))
        );
        assert_eq!(unknown["unknown"], json!(["cpu", "memory"]));
        let local = hotsheet_ticketing::orchestration::AdmissionStore::new(&f.home);
        local.update(|config| config.allow_unknown = true).unwrap();
        assert_eq!(outcome(&ask(codex(1)).await).0, "accepted");
        // The machine owner's stricter cap beats the limits a caller sends.
        local
            .update(|config| config.max_sessions = Some(1))
            .unwrap();
        let strict = ask(codex(2)).await;
        assert_eq!(
            outcome(&strict),
            ("reduced".into(), 1, Some("session_cap_reached".into()))
        );
        assert_eq!(strict["limits"]["max_sessions"], 1);
        // A malformed question and a damaged admission file are errors with stable codes.
        let (status, error) = call(
            &router,
            "POST",
            &admission,
            Some(json!({"tool":"codex","sessions":0})),
        )
        .await;
        assert_eq!(
            (status, &error["code"]),
            (StatusCode::BAD_REQUEST, &json!("invalid_request"))
        );
        std::fs::write(f.home.join("orchestration-admission.json"), b"{").unwrap();
        let (status, error) = call(&router, "POST", &admission, Some(codex(1))).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error["code"], "corrupt_orchestration_data");
    }

    /// HS2-0VTQFF: the capacity read reports each detected tool's catalog models from
    /// the server's own discovery (the one behind `/ai-tools`), default first; the
    /// reading is shared, and a discovery invalidation asks again.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn hs2_0vtqff_the_capacity_read_reports_each_tools_catalog_models() {
        let mut f = fixture();
        let scans = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counted = scans.clone();
        f.state = f
            .state
            .clone()
            .with_capacity_cache(Duration::ZERO)
            .with_ai_tool_discovery(Duration::ZERO, move |_, _, _| {
                counted.fetch_add(1, Ordering::SeqCst);
                vec![descriptor(
                    "codex",
                    &["syn-a", "syn-b", "syn-c"],
                    Some("syn-b"),
                )]
            });
        f.scripted.set(|script| script.models_from_discovery = true);
        let router = app(f.state.clone());
        let capacity = format!("/checkouts/{}/target-capacity", f.checkout.id);
        let (status, snapshot) = call(&router, "GET", &capacity, None).await;
        assert_eq!(status, StatusCode::OK, "{snapshot}");
        assert_eq!(
            snapshot["tools"],
            json!([{"tool":"codex","installed":true,"configured":true,
                    "models":["syn-b","syn-a","syn-c"]},
                {"tool":"claude","installed":false,"configured":true}])
        );
        // A second reading (the capacity cache is off) shares the model reading.
        let (_, again) = call(&router, "GET", &capacity, None).await;
        assert_eq!(again["tools"], snapshot["tools"]);
        assert_eq!(scans.load(Ordering::SeqCst), 1);
        assert_eq!(f.scripted.0.with_lock(|script| script.models_asked), 1);
        // A tool install or plugin change asks again.
        f.state.invalidate_ai_tool_discovery();
        let (_, after) = call(&router, "GET", &capacity, None).await;
        assert_eq!(after["tools"], snapshot["tools"]);
        assert_eq!(scans.load(Ordering::SeqCst), 2);
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_health_capacity_lists_live_ai_sessions_by_tool_and_footprint_only() {
        let f = fixture();
        let router = app(f.state.clone());
        f.scripted
            .set(|script| script.resident_bytes = Some(512 * MIB));
        let spawn = |id: &str, kind, worker: &str| {
            let mut spec = hotsheet_terminals::TermSpec::new("cat");
            spec.kind = kind;
            spec.cwd = Some(f.project.clone());
            spec.env = vec![(
                hotsheet_aitools::WORKER_ID_ENV.to_owned(),
                worker.to_owned(),
            )];
            spec.provenance = Some(hotsheet_terminals::ProcessProvenance {
                machine_home: f.home.clone(),
                supervisor: "machine-server".into(),
                project_id: "synthetic-project".into(),
                session_id: id.into(),
                binary_revision: None,
            });
            f.state
                .terminals
                .get_or_spawn(crate::routes::terminals::term_key(&f.state, id), spec)
                .unwrap()
        };
        use hotsheet_terminals::TerminalKind::{Ai, Shell};
        let session = spawn("ai-1", Ai, "codex-ai-1");
        let shell = spawn("shell-1", Shell, "terminal-shell-1");
        let capacity = format!("/checkouts/{}/target-capacity", f.checkout.id);
        let (status, snapshot) = call(&router, "GET", &capacity, None).await;
        assert_eq!(status, StatusCode::OK, "{snapshot}");
        // The shell is not an AI session; the AI session is named by id and tool.
        let sessions = snapshot["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1, "{snapshot}");
        assert_eq!(sessions[0]["session_id"], "ai-1");
        assert_eq!(sessions[0]["tool"], "codex");
        assert_eq!(sessions[0]["on_checkout"], true);
        assert_eq!(sessions[0]["resident_mb"], 512);
        assert!(sessions[0]["busy"].is_boolean());
        assert_eq!(sessions[0].as_object().unwrap().len(), 5);
        // The footprint was asked for the journalled process of that session only.
        let journal = hotsheet_process_journal::ProcessJournal::at(&f.home)
            .list()
            .unwrap();
        let pid = journal
            .iter()
            .find(|record| record.session_id.as_deref() == Some("ai-1"))
            .unwrap()
            .pid;
        assert_eq!(
            f.scripted
                .0
                .with_lock(|script| script.resident_asked.clone()),
            [vec![pid]]
        );
        // Nothing that could identify a path, command or user leaves the server.
        let wire = snapshot.to_string();
        for private in [
            f.project.to_str().unwrap(),
            f.home.to_str().unwrap(),
            "cwd",
            "command",
            "pid",
        ] {
            assert!(!wire.contains(private), "{private} in {wire}");
        }
        // One running session counts against the caller's cap.
        let admission = format!("/checkouts/{}/target-admission", f.checkout.id);
        let (_, decision) = call(
            &router,
            "POST",
            &admission,
            Some(json!({"tool":"codex","sessions":2,"limits":{"max_sessions":2}})),
        )
        .await;
        assert_eq!(decision["outcome"], "reduced");
        assert_eq!(decision["admitted"], 1);
        assert_eq!(decision["reason"], "session_cap_reached");
        let _ = session.kill();
        let _ = shell.kill();
    }
}

// ---- HS2-ZY4Z4G: capabilities ------------------------------------------------------------

#[test]
fn hs2_zy4z4g_capabilities_are_detected_once_and_again_after_the_ttl_or_new_toolchains() {
    use hotsheet_ticketing::orchestration::OsFamily;
    let scripted = Scripted::roomy();
    scripted.set(|script| {
        script.capabilities = Some(HostCapabilities::bare(OsFamily::Linux, "x86_64"));
    });
    let capacity = Capacity::with_source(Box::new(scripted.clone()));
    // Nothing is detected before someone asks.
    assert_eq!(capacity.detections(), 0);
    let default: Vec<String> = vec!["cargo".into(), "node".into()];
    let first = capacity.capabilities(&[], &default);
    assert_eq!(first.as_ref().map(|caps| caps.os), Some(OsFamily::Linux));
    // A capacity reading, even an uncached one, does not detect again.
    capacity.set_cache(Duration::ZERO);
    let root = tempfile::tempdir().unwrap();
    observe(&capacity, root.path(), None);
    assert_eq!(capacity.capabilities(&[], &default), first);
    assert_eq!(capacity.detections(), 1);
    // Other declared toolchains are asked about at once.
    let declared = vec!["bazel".to_owned()];
    capacity.capabilities(&[], &declared);
    assert_eq!(capacity.detections(), 2);
    assert_eq!(
        scripted
            .0
            .with_lock(|script| script.toolchains_asked.clone()),
        vec![default, declared.clone()]
    );
    // A source that cannot tell is remembered too: unknown is not asked again per request.
    scripted.set(|script| script.capabilities = None);
    assert!(capacity.capabilities(&[], &declared).is_some());
    assert_eq!(capacity.detections(), 2);
}

// ---- HS2-P2TCQS: re-checking sign-in while held ----------------------------------------

#[test]
fn hs2_p2tcqs_sign_in_is_rechecked_only_for_reads_that_ask_while_a_tool_is_signed_out() {
    use hotsheet_cli::orchestration::SignInRecheck;
    use hotsheet_ticketing::orchestration::{OsFamily, ToolVersion};
    let report = |signed_in: Option<bool>| {
        let mut caps = HostCapabilities::bare(OsFamily::Linux, "x86_64");
        caps.ai_tools = Some(vec![ToolVersion {
            tool: "claude".into(),
            version: None,
            signed_in,
        }]);
        caps
    };
    let signed_in = |caps: Option<HostCapabilities>| {
        caps.and_then(|caps| caps.ai_tools)
            .and_then(|tools| tools.first().and_then(|tool| tool.signed_in))
    };
    let scripted = Scripted::roomy();
    scripted.set(|script| script.capabilities = Some(report(Some(true))));
    let capacity = Capacity::with_source(Box::new(scripted.clone()));
    // Every step is due at once, so only the asking decides.
    capacity.set_sign_in_recheck(Duration::ZERO, Duration::ZERO, Duration::ZERO);
    let toolchains: Vec<String> = Vec::new();
    let read = |recheck| capacity.capabilities_rechecking(&[], &toolchains, recheck);

    // Signed in: a read that asks re-checks nothing.
    assert_eq!(signed_in(read(None)), Some(true));
    for recheck in [Some(SignInRecheck::Held), Some(SignInRecheck::Now), None] {
        assert_eq!(signed_in(read(recheck)), Some(true));
    }
    assert_eq!(capacity.detections(), 1);

    // Signed out, seen at the next detection (a toolchain change here).
    scripted.set(|script| script.capabilities = Some(report(Some(false))));
    assert_eq!(
        signed_in(capacity.capabilities_rechecking(&[], &["go".into()], None)),
        Some(false)
    );
    assert_eq!(capacity.detections(), 2);
    let toolchains = vec!["go".to_owned()];
    let read = |recheck| capacity.capabilities_rechecking(&[], &toolchains, recheck);
    // Nothing held: no read re-checks, however often.
    for _ in 0..3 {
        assert_eq!(signed_in(read(None)), Some(false));
    }
    assert_eq!(capacity.detections(), 2);
    // Held: each due read detects again; a person looking does too.
    assert_eq!(signed_in(read(Some(SignInRecheck::Held))), Some(false));
    assert_eq!(signed_in(read(Some(SignInRecheck::Now))), Some(false));
    assert_eq!(capacity.detections(), 4);
    // The tool signs in: the next held read sees it, and later reads ask no more.
    scripted.set(|script| script.capabilities = Some(report(Some(true))));
    assert_eq!(signed_in(read(Some(SignInRecheck::Held))), Some(true));
    assert_eq!(capacity.detections(), 5);
    for recheck in [Some(SignInRecheck::Held), Some(SignInRecheck::Now)] {
        assert_eq!(signed_in(read(recheck)), Some(true));
    }
    assert_eq!(capacity.detections(), 5);
    // Unknown is not signed out: nothing to re-check.
    scripted.set(|script| script.capabilities = Some(report(None)));
    capacity.set_capabilities_ttl(Duration::ZERO);
    assert_eq!(signed_in(read(None)), None);
    capacity.set_capabilities_ttl(Duration::from_secs(600));
    let before = capacity.detections();
    for recheck in [Some(SignInRecheck::Held), Some(SignInRecheck::Now)] {
        assert_eq!(signed_in(read(recheck)), None);
    }
    assert_eq!(capacity.detections(), before);
}

#[test]
fn hs2_p2tcqs_default_steps_keep_a_fresh_signed_out_reading() {
    use hotsheet_cli::orchestration::SignInRecheck;
    use hotsheet_ticketing::orchestration::{OsFamily, ToolVersion};
    let mut caps = HostCapabilities::bare(OsFamily::Linux, "x86_64");
    caps.ai_tools = Some(vec![ToolVersion {
        tool: "claude".into(),
        version: None,
        signed_in: Some(false),
    }]);
    let scripted = Scripted::roomy();
    scripted.set(|script| script.capabilities = Some(caps));
    let capacity = Capacity::with_source(Box::new(scripted));
    // A minute's first step and a fifteen-second debounce: a reading taken just now
    // answers held and on-demand reads alike.
    for recheck in [None, Some(SignInRecheck::Held), Some(SignInRecheck::Now)] {
        capacity.capabilities_rechecking(&[], &[], recheck);
    }
    assert_eq!(capacity.detections(), 1);
}

// ---- HS2-0VTQFF: catalog models ----------------------------------------------------------

fn descriptor(
    id: &str,
    models: &[&str],
    default: Option<&str>,
) -> hotsheet_plugins::AiToolDescriptor {
    hotsheet_plugins::AiToolDescriptor {
        id: id.into(),
        display_name: id.into(),
        models: models
            .iter()
            .map(|model| hotsheet_plugins::ModelSpec {
                id: (*model).into(),
                label: (*model).into(),
                effort_levels: Vec::new(),
            })
            .collect(),
        default_model: default.map(str::to_owned),
        ..hotsheet_plugins::AiToolDescriptor::default()
    }
}

#[test]
fn hs2_0vtqff_reported_models_put_the_catalog_default_first_then_catalog_order() {
    let reported = reported_models(&[
        descriptor("alpha", &["syn-3", "syn-2", "syn-1"], Some("syn-2")),
        // A default the catalog does not list is not reported.
        descriptor("beta", &["syn-b2", "syn-b1"], Some("syn-gone")),
        descriptor("gamma", &["syn-g1"], None),
        descriptor("delta", &[], Some("syn-d")),
    ]);
    let list = |tool: &str| reported.get(tool).cloned();
    let ids = |ids: &[&str]| Some(ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>());
    assert_eq!(list("alpha"), ids(&["syn-2", "syn-3", "syn-1"]));
    assert_eq!(list("beta"), ids(&["syn-b2", "syn-b1"]));
    assert_eq!(list("gamma"), ids(&["syn-g1"]));
    assert_eq!(list("delta"), ids(&[]));

    // Tools take their own list; one discovery did not report keeps unknown.
    let mut tools = vec![
        ToolAvailability {
            tool: "alpha".into(),
            installed: true,
            configured: true,
            models: None,
        },
        ToolAvailability {
            tool: "omega".into(),
            installed: false,
            configured: true,
            models: Some(vec!["stale".into()]),
        },
    ];
    fill_models(&mut tools, &reported);
    assert_eq!(tools[0].models, list("alpha"));
    assert_eq!(tools[1].models, None);
}
