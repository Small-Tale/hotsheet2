//! HS2-J8PRTV: the admission decision at each boundary, the stricter-wins
//! intersection and the machine-local admission file. Synthetic values only.

use super::*;

const FLOOR: u64 = 2048;
const FOOTPRINT: u64 = DEFAULT_SESSION_FOOTPRINT_MB;

fn tool(name: &str, installed: bool, configured: bool) -> ToolAvailability {
    ToolAvailability {
        tool: name.into(),
        installed,
        configured,
    }
}
fn session(id: &str, tool: Option<&str>, resident_mb: Option<u64>) -> SessionFootprint {
    SessionFootprint {
        session_id: id.into(),
        tool: tool.map(Into::into),
        busy: false,
        on_checkout: Some(true),
        resident_mb,
    }
}
/// A healthy idle machine: 8 GB free, 20% CPU, 50 GB disk, codex usable.
fn healthy() -> CapacitySnapshot {
    CapacitySnapshot::new(
        ulid::Ulid::from(7_u128).to_string(),
        "target-0123456789ab".into(),
        HostObservation {
            capabilities: None,
            sampled_at_ms: Some(1_000),
            memory: MemoryCapacity {
                free_mb: Some(8192),
                total_mb: Some(16_384),
            },
            cpu: CpuCapacity {
                load_percent: Some(20),
                logical_cpus: Some(8),
            },
            disk: DiskCapacity {
                free_mb: Some(50_000),
                total_mb: Some(500_000),
            },
            tools: Some(vec![
                tool("codex", true, true),
                tool("claude", true, false),
                tool("gemini", false, true),
            ]),
            sessions: Some(Vec::new()),
        },
        Vec::new(),
        None,
    )
}
fn ask(tool: &str, sessions: u32) -> AdmissionRequest {
    AdmissionRequest {
        tool: tool.into(),
        model: None,
        sessions,
        limits: Some(MachineLimits::default()),
    }
}

type Change = fn(&mut CapacitySnapshot, &mut AdmissionConfig, &mut AdmissionRequest);
type Expected = (AdmissionOutcome, u32, Option<AdmissionReason>);

#[test]
fn hs2_j8prtv_admission_accepts_reduces_and_refuses_at_each_boundary() {
    use AdmissionOutcome::{Accepted, Reduced, Refused};
    use AdmissionReason as R;
    // Policy defaults: 2 sessions, 2048 MB RAM floor, 90% CPU, 5120 MB disk floor.
    let table: &[(&str, Change, Expected)] = &[
        ("healthy", |_, _, _| {}, (Accepted, 1, None)),
        (
            "all the cap allows",
            |_, _, r| r.sessions = 2,
            (Accepted, 2, None),
        ),
        (
            "more than the cap",
            |_, _, r| r.sessions = 3,
            (Reduced, 2, Some(R::SessionCapReached)),
        ),
        (
            "one already running",
            |s, _, r| {
                s.sessions = Some(vec![session("a", Some("claude"), None)]);
                r.sessions = 2;
            },
            (Reduced, 1, Some(R::SessionCapReached)),
        ),
        (
            "cap reached",
            |s, _, _| {
                s.sessions = Some(vec![
                    session("a", Some("codex"), None),
                    session("b", None, None),
                ]);
            },
            (Refused, 0, Some(R::SessionCapReached)),
        ),
        (
            "exactly one footprint above the floor",
            |s, _, _| s.memory.free_mb = Some(FLOOR + FOOTPRINT),
            (Accepted, 1, None),
        ),
        (
            "one megabyte short of a footprint",
            |s, _, _| s.memory.free_mb = Some(FLOOR + FOOTPRINT - 1),
            (Refused, 0, Some(R::BelowRamFloor)),
        ),
        (
            "below the floor already",
            |s, _, _| s.memory.free_mb = Some(FLOOR - 1),
            (Refused, 0, Some(R::BelowRamFloor)),
        ),
        (
            "room for one of two",
            |s, _, r| {
                s.memory.free_mb = Some(FLOOR + 2 * FOOTPRINT - 1);
                r.sessions = 2;
            },
            (Reduced, 1, Some(R::BelowRamFloor)),
        ),
        (
            "CPU at the ceiling",
            |s, _, _| s.cpu.load_percent = Some(90),
            (Accepted, 1, None),
        ),
        (
            "CPU above the ceiling",
            |s, _, _| s.cpu.load_percent = Some(91),
            (Refused, 0, Some(R::AboveCpuCeiling)),
        ),
        (
            "disk at the floor",
            |s, _, _| s.disk.free_mb = Some(5120),
            (Accepted, 1, None),
        ),
        (
            "disk below the floor",
            |s, _, _| s.disk.free_mb = Some(5119),
            (Refused, 0, Some(R::BelowDiskFloor)),
        ),
        (
            "tool the host does not know",
            |_, _, r| r.tool = "absent".into(),
            (Refused, 0, Some(R::ToolNotInstalled)),
        ),
        (
            "tool known but not installed",
            |_, _, r| r.tool = "gemini".into(),
            (Refused, 0, Some(R::ToolNotInstalled)),
        ),
        (
            "tool the checkout opted out of",
            |_, _, r| r.tool = "claude".into(),
            (Refused, 0, Some(R::ToolNotConfigured)),
        ),
        (
            "unknown memory fails closed",
            |s, _, _| s.memory.free_mb = None,
            (Refused, 0, Some(R::CapacityUnknown)),
        ),
        (
            "unknown CPU fails closed",
            |s, _, _| s.cpu.load_percent = None,
            (Refused, 0, Some(R::CapacityUnknown)),
        ),
        (
            "unknown disk fails closed",
            |s, _, _| s.disk.free_mb = None,
            (Refused, 0, Some(R::CapacityUnknown)),
        ),
        (
            "unknown tools fail closed",
            |s, _, _| s.tools = None,
            (Refused, 0, Some(R::CapacityUnknown)),
        ),
        (
            "unknown sessions fail closed",
            |s, _, _| s.sessions = None,
            (Refused, 0, Some(R::CapacityUnknown)),
        ),
        (
            "the owner allowed an unknown",
            |s, c, _| {
                s.memory.free_mb = None;
                c.allow_unknown = true;
            },
            (Accepted, 1, None),
        ),
        (
            "a broken known limit outranks an unknown",
            |s, _, _| {
                s.memory.free_mb = None;
                s.cpu.load_percent = Some(95);
            },
            (Refused, 0, Some(R::AboveCpuCeiling)),
        ),
        (
            "allowing unknowns never excuses a known limit",
            |s, c, _| {
                s.sessions = None;
                s.disk.free_mb = Some(1);
                c.allow_unknown = true;
            },
            (Refused, 0, Some(R::BelowDiskFloor)),
        ),
        (
            "unknown sessions allowed: RAM still bounds the count",
            |s, c, r| {
                s.sessions = None;
                s.memory.free_mb = Some(FLOOR + FOOTPRINT);
                c.allow_unknown = true;
                r.sessions = 4;
            },
            (Reduced, 1, Some(R::BelowRamFloor)),
        ),
        (
            "a smaller configured footprint fits more",
            |s, c, r| {
                s.memory.free_mb = Some(FLOOR + FOOTPRINT);
                c.tool_footprint_mb.insert("codex".into(), 512);
                r.sessions = 2;
            },
            (Accepted, 2, None),
        ),
        (
            "an observed footprint above the estimate is used",
            |s, _, _| {
                s.sessions = Some(vec![session("a", Some("codex"), Some(3000))]);
                s.memory.free_mb = Some(FLOOR + 2999);
            },
            (Refused, 0, Some(R::BelowRamFloor)),
        ),
        (
            "the owner's stricter cap wins",
            |_, c, r| {
                c.max_sessions = Some(1);
                r.sessions = 2;
            },
            (Reduced, 1, Some(R::SessionCapReached)),
        ),
        (
            "the owner refuses every session",
            |_, c, _| c.max_sessions = Some(0),
            (Refused, 0, Some(R::SessionCapReached)),
        ),
    ];
    for (name, change, expected) in table {
        let (mut snapshot, mut config, mut request) =
            (healthy(), AdmissionConfig::default(), ask("codex", 1));
        change(&mut snapshot, &mut config, &mut request);
        let decision = decide_admission(&snapshot, &config, &request);
        assert_eq!(
            (decision.outcome, decision.admitted, decision.reason),
            *expected,
            "{name}"
        );
        assert_eq!(decision.requested, request.sessions, "{name}");
        assert_eq!(decision.sampled_at_ms, Some(1_000), "{name}");
        // An accepted answer admits all that was asked; a reduced one fewer, never zero.
        match decision.outcome {
            Accepted => assert_eq!(decision.admitted, decision.requested, "{name}"),
            Reduced => assert!(
                (1..decision.requested).contains(&decision.admitted),
                "{name}"
            ),
            Refused => assert_eq!(decision.admitted, 0, "{name}"),
        }
    }
}

#[test]
fn hs2_j8prtv_a_decision_names_every_unknown_and_its_footprint_source() {
    let mut snapshot = healthy();
    snapshot.memory.free_mb = None;
    snapshot.cpu.load_percent = None;
    snapshot.disk.free_mb = None;
    snapshot.tools = None;
    snapshot.sessions = None;
    snapshot.sampled_at_ms = None;
    let closed = decide_admission(&snapshot, &AdmissionConfig::default(), &ask("codex", 3));
    assert_eq!(closed.reason, Some(AdmissionReason::CapacityUnknown));
    assert_eq!(
        closed.unknown,
        [
            CapacityField::Tools,
            CapacityField::Sessions,
            CapacityField::Cpu,
            CapacityField::Disk,
            CapacityField::Memory
        ]
    );
    assert_eq!(closed.sampled_at_ms, None);
    assert_eq!(
        (closed.footprint_mb, closed.footprint_source),
        (1024, FootprintSource::Default)
    );
    // Nothing is known and the owner allowed that: the answer is theirs to own.
    let open = decide_admission(
        &snapshot,
        &AdmissionConfig {
            allow_unknown: true,
            default_footprint_mb: Some(2000),
            ..AdmissionConfig::default()
        },
        &ask("codex", 3),
    );
    assert_eq!(
        (open.outcome, open.admitted),
        (AdmissionOutcome::Accepted, 3)
    );
    assert_eq!(open.unknown.len(), 5);
    assert_eq!(
        (open.footprint_mb, open.footprint_source),
        (2000, FootprintSource::Configured)
    );
    // A tool's own estimate beats the default; an observation only raises it, and
    // only for sessions of that same tool.
    let config = AdmissionConfig {
        default_footprint_mb: Some(2000),
        tool_footprint_mb: [("codex".to_owned(), 700)].into(),
        ..AdmissionConfig::default()
    };
    let running = [
        session("a", Some("codex"), Some(650)),
        session("b", Some("claude"), Some(9000)),
        session("c", Some("codex"), None),
    ];
    assert_eq!(
        estimated_footprint(&config, Some(&running), "codex"),
        (700, FootprintSource::Configured)
    );
    assert_eq!(
        estimated_footprint(&config, Some(&running), "claude"),
        (9000, FootprintSource::Observed)
    );
    assert_eq!(
        estimated_footprint(&config, None, "other"),
        (2000, FootprintSource::Configured)
    );
    // A request without policy limits is held to the policy defaults.
    let bare = AdmissionRequest {
        limits: None,
        ..ask("codex", 1)
    };
    assert_eq!(
        decide_admission(&healthy(), &AdmissionConfig::default(), &bare).limits,
        MachineLimits::default()
    );
}

#[test]
fn hs2_j8prtv_the_machine_owner_can_only_make_policy_limits_stricter() {
    let policy = MachineLimits {
        max_sessions: 4,
        min_free_ram_mb: 2000,
        max_cpu_percent: 80,
        min_free_disk_mb: 5000,
    };
    let local = |max_sessions, min_free_ram_mb, max_cpu_percent, min_free_disk_mb| {
        effective_limits(
            &policy,
            &AdmissionConfig {
                max_sessions,
                min_free_ram_mb,
                max_cpu_percent,
                min_free_disk_mb,
                ..AdmissionConfig::default()
            },
        )
    };
    assert_eq!(local(None, None, None, None), policy);
    // Stricter on every field: the owner wins.
    assert_eq!(
        local(Some(1), Some(4000), Some(50), Some(9000)),
        MachineLimits {
            max_sessions: 1,
            min_free_ram_mb: 4000,
            max_cpu_percent: 50,
            min_free_disk_mb: 9000,
        }
    );
    // Looser on every field: the policy stands.
    assert_eq!(local(Some(64), Some(1), Some(100), Some(1)), policy);
    // Equal values and a mixed file.
    assert_eq!(local(Some(4), Some(2000), Some(80), Some(5000)), policy);
    assert_eq!(
        local(Some(2), Some(1), None, Some(6000)),
        MachineLimits {
            max_sessions: 2,
            min_free_disk_mb: 6000,
            ..policy
        }
    );
}

#[test]
fn hs2_j8prtv_requests_and_configurations_are_validated_at_the_boundary() {
    assert!(ask("codex", 1).validate().is_ok());
    assert!(ask("codex", 64).validate().is_ok());
    for bad in [
        ask("codex", 0),
        ask("codex", 65),
        ask("", 1),
        ask("has space", 1),
        AdmissionRequest {
            model: Some("two words".into()),
            ..ask("codex", 1)
        },
        AdmissionRequest {
            limits: Some(MachineLimits {
                max_cpu_percent: 0,
                ..MachineLimits::default()
            }),
            ..ask("codex", 1)
        },
    ] {
        assert!(
            matches!(bad.validate(), Err(OrchestrationError::Invalid(_))),
            "{bad:?}"
        );
    }
    // The wire body: sessions defaults to one, unknown keys are refused.
    let parsed: AdmissionRequest = serde_json::from_str(r#"{"tool":"codex"}"#).unwrap();
    assert_eq!((parsed.sessions, parsed.limits), (1, None));
    assert!(serde_json::from_str::<AdmissionRequest>(r#"{"tool":"codex","path":"x"}"#).is_err());

    let valid = AdmissionConfig {
        max_sessions: Some(0),
        max_cpu_percent: Some(100),
        default_footprint_mb: Some(1),
        ..AdmissionConfig::default()
    };
    assert!(valid.validate().is_ok());
    let invalid = [
        AdmissionConfig {
            max_sessions: Some(65),
            ..AdmissionConfig::default()
        },
        AdmissionConfig {
            max_cpu_percent: Some(0),
            ..AdmissionConfig::default()
        },
        AdmissionConfig {
            max_cpu_percent: Some(101),
            ..AdmissionConfig::default()
        },
        AdmissionConfig {
            min_free_ram_mb: Some(1024 * 1024 + 1),
            ..AdmissionConfig::default()
        },
        AdmissionConfig {
            default_footprint_mb: Some(0),
            ..AdmissionConfig::default()
        },
        AdmissionConfig {
            tool_footprint_mb: [("codex".to_owned(), MAX_SESSION_FOOTPRINT_MB + 1)].into(),
            ..AdmissionConfig::default()
        },
        AdmissionConfig {
            tool_footprint_mb: [("bad tool".to_owned(), 10)].into(),
            ..AdmissionConfig::default()
        },
        AdmissionConfig {
            tool_footprint_mb: (0..17).map(|n| (format!("tool-{n}"), 10)).collect(),
            ..AdmissionConfig::default()
        },
    ];
    for config in invalid {
        assert!(config.validate().is_err(), "{config:?}");
    }
}

#[test]
fn hs2_j8prtv_reason_codes_and_the_snapshot_wire_shape_are_stable() {
    use AdmissionReason as R;
    let reasons = [
        (R::ToolNotInstalled, "tool_not_installed"),
        (R::ToolNotConfigured, "tool_not_configured"),
        (R::SessionCapReached, "session_cap_reached"),
        (R::AboveCpuCeiling, "above_cpu_ceiling"),
        (R::BelowDiskFloor, "below_disk_floor"),
        (R::BelowRamFloor, "below_ram_floor"),
        (R::CapacityUnknown, "capacity_unknown"),
    ];
    for (reason, code) in reasons {
        assert_eq!(reason.code(), code);
        assert_eq!(serde_json::to_value(reason).unwrap(), code);
    }
    // An unmeasured value is an explicit null, never an absent key or a zero.
    let unknown = CapacitySnapshot::new(
        ulid::Ulid::from(7_u128).to_string(),
        "target-0123456789ab".into(),
        HostObservation::default(),
        vec!["builds".into()],
        None,
    );
    let wire = serde_json::to_value(&unknown).unwrap();
    for pointer in [
        "/sampled_at_ms",
        "/memory/free_mb",
        "/memory/total_mb",
        "/cpu/load_percent",
        "/cpu/logical_cpus",
        "/disk/free_mb",
        "/tools",
        "/sessions",
        "/claims",
    ] {
        assert_eq!(wire.pointer(pointer), Some(&serde_json::Value::Null));
    }
    assert_eq!(wire["schema_version"], 1);
    assert_eq!(wire["claim_namespaces"], serde_json::json!(["builds"]));
    // A session is an id, a tool name and metrics: nothing that could hold a path.
    let mut snapshot = healthy();
    snapshot.sessions = Some(vec![session("term-1", Some("codex"), None)]);
    let wire = serde_json::to_value(&snapshot).unwrap();
    let mut keys: Vec<_> = wire["sessions"][0].as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["busy", "on_checkout", "resident_mb", "session_id", "tool"]
    );
    assert_eq!(wire["sessions"][0]["resident_mb"], serde_json::Value::Null);
    assert_eq!(
        serde_json::from_value::<CapacitySnapshot>(wire).unwrap(),
        snapshot
    );
    assert_eq!(snapshot.clone().into_host().sessions, snapshot.sessions);
    let decision = decide_admission(&healthy(), &AdmissionConfig::default(), &ask("codex", 1));
    let wire = serde_json::to_value(&decision).unwrap();
    assert_eq!(wire["outcome"], "accepted");
    assert_eq!(wire["reason"], serde_json::Value::Null);
    assert_eq!(wire["footprint_source"], "default");
    assert_eq!(
        serde_json::from_value::<AdmissionDecision>(wire).unwrap(),
        decision
    );
}

#[test]
fn hs2_j8prtv_the_admission_file_is_idempotent_private_and_fails_closed() {
    let home = tempfile::tempdir().unwrap();
    let store = AdmissionStore::new(home.path());
    let path = home.path().join("orchestration-admission.json");
    // Reading an absent file is the default configuration and creates nothing.
    let absent = store.read().unwrap();
    assert!(!absent.configured);
    assert_eq!(absent.admission, AdmissionConfig::default());
    assert_eq!(absent.default_footprint_mb, 1024);
    assert!(!path.exists());

    let set = store
        .update(|config| {
            config.max_sessions = Some(1);
            config.tool_footprint_mb.insert("codex".into(), 1500);
        })
        .unwrap();
    assert!(set.configured);
    assert_eq!(set.admission.max_sessions, Some(1));
    let written = std::fs::read(&path).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&written).unwrap();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["admission"]["tool_footprint_mb"]["codex"], 1500);
    assert_eq!(document["admission"]["allow_unknown"], false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // The same change again writes nothing; a further edit and a reset both hold.
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let again = store
        .update(|config| config.max_sessions = Some(1))
        .unwrap();
    assert_eq!(again, set);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(std::fs::read(&path).unwrap(), written);
    store.update(|config| config.allow_unknown = true).unwrap();
    assert!(store.read().unwrap().admission.allow_unknown);
    let cleared = store
        .update(|config| *config = AdmissionConfig::default())
        .unwrap();
    assert!(cleared.configured);
    assert_eq!(cleared.admission, AdmissionConfig::default());
    // An invalid change is refused and leaves the file as it was.
    let before = std::fs::read(&path).unwrap();
    assert!(matches!(
        store.update(|config| config.max_cpu_percent = Some(0)),
        Err(OrchestrationError::Invalid(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // Damaged, unknown-key, out-of-range and newer files are errors, never defaults.
    for (bytes, upgrade) in [
        (&b"{not json"[..], false),
        (
            br#"{"schema_version":1,"admission":{"allow_everything":true}}"#,
            false,
        ),
        (
            br#"{"schema_version":1,"admission":{"max_cpu_percent":0}}"#,
            false,
        ),
        (br#"{"schema_version":2,"admission":{}}"#, true),
    ] {
        std::fs::write(&path, bytes).unwrap();
        let error = store.read().unwrap_err();
        assert_eq!(
            matches!(error, OrchestrationError::UnsupportedVersion { found: 2 }),
            upgrade
        );
        assert!(upgrade || matches!(error, OrchestrationError::Corrupt));
        assert!(store.update(|config| config.allow_unknown = true).is_err());
    }
}

/// HS2-P5GTSW: the capacity version changes exactly when room changes.
#[test]
fn hs2_p5gtsw_the_capacity_version_changes_with_room_not_with_jitter() {
    let base = healthy().with_version();
    let version = base.version.clone().unwrap();
    assert_eq!(version, capacity_version(&healthy()));
    assert_eq!(version.len(), 16);
    // Stamping twice is the same; the stamp itself is not part of the digest.
    assert_eq!(base.clone().with_version().version, Some(version.clone()));
    type Change = fn(&mut CapacitySnapshot);
    let same: [(&str, Change); 6] = [
        ("sampling time", |s| s.sampled_at_ms = Some(9_999)),
        ("memory within a step", |s| {
            s.memory.free_mb = Some(8192 + 1000)
        }),
        ("load within a step", |s| s.cpu.load_percent = Some(39)),
        ("disk within a step", |s| {
            s.disk.free_mb = Some(50_000 + 1000)
        }),
        ("tools reordered", |s| s.tools.as_mut().unwrap().reverse()),
        ("a session going busy", |s| {
            s.sessions = Some(vec![session("a", Some("codex"), Some(10))]);
        }),
    ];
    let busy_base = {
        let mut s = healthy();
        s.sessions = Some(vec![session("a", Some("codex"), Some(10))]);
        capacity_version(&s)
    };
    for (name, change) in same {
        let mut snapshot = healthy();
        change(&mut snapshot);
        let expected = if name == "a session going busy" {
            // Compared against the same session idle: busy and footprint do not count.
            let mut busy = snapshot.clone();
            busy.sessions.as_mut().unwrap()[0].busy = true;
            busy.sessions.as_mut().unwrap()[0].resident_mb = Some(999);
            assert_eq!(capacity_version(&busy), busy_base, "{name}");
            busy_base.clone()
        } else {
            version.clone()
        };
        assert_eq!(capacity_version(&snapshot), expected, "{name}");
    }
    let differs: [(&str, Change); 12] = [
        ("memory a step lower", |s| {
            s.memory.free_mb = Some(8192 - 1024)
        }),
        ("memory unknown", |s| s.memory.free_mb = None),
        ("load a step higher", |s| s.cpu.load_percent = Some(40)),
        ("load unknown", |s| s.cpu.load_percent = None),
        ("disk a step lower", |s| {
            s.disk.free_mb = Some(50_000 - 4096)
        }),
        ("a tool installed", |s| {
            s.tools.as_mut().unwrap()[2].installed = true
        }),
        ("tools unknown", |s| s.tools = None),
        ("a session started", |s| {
            s.sessions = Some(vec![session("a", Some("codex"), None)]);
        }),
        ("sessions unknown", |s| s.sessions = None),
        ("a claim held", |s| {
            s.claims = Some(vec![HeldClaim {
                namespace: "vm".into(),
                resource: "macos-vm".into(),
                expires_at_ms: 5,
            }]);
        }),
        ("claims offline", |s| {
            s.claims_offline = Some(OfflinePolicy::Provisional)
        }),
        ("speculation opted in", |s| {
            s.speculation = Some(TargetSpeculation {
                enabled: true,
                max_depth: 1,
                integration: Default::default(),
            });
        }),
    ];
    let mut seen = std::collections::BTreeSet::from([version.clone()]);
    for (name, change) in differs {
        let mut snapshot = healthy();
        change(&mut snapshot);
        let changed = capacity_version(&snapshot);
        assert_ne!(changed, version, "{name}");
        assert!(seen.insert(changed), "{name} collides with another change");
    }
    // A claim renewed (a new expiry) is the same claim.
    let held = |expires_at_ms| {
        let mut s = healthy();
        s.claims = Some(vec![HeldClaim {
            namespace: "vm".into(),
            resource: "macos-vm".into(),
            expires_at_ms,
        }]);
        capacity_version(&s)
    };
    assert_eq!(held(5), held(500));
    // The wire form carries it, and an older peer's snapshot without it still reads.
    let wire = serde_json::to_value(&base).unwrap();
    assert_eq!(wire["version"], version.as_str());
    let mut older = wire.clone();
    older.as_object_mut().unwrap().remove("version");
    let read: CapacitySnapshot = serde_json::from_value(older).unwrap();
    assert_eq!(read.version, None);
}

/// HS2-AWB7BX: the machine owner's own admission limits are part of the version.
#[test]
fn hs2_awb7bx_admission_limits_change_the_capacity_version() {
    let plain = healthy().with_version().version;
    assert_eq!(
        healthy()
            .with_admission_version(&AdmissionConfig::default())
            .version,
        plain
    );
    let closed = AdmissionConfig {
        max_sessions: Some(0),
        ..AdmissionConfig::default()
    };
    let open = AdmissionConfig {
        max_sessions: Some(4),
        ..AdmissionConfig::default()
    };
    let with = |config: &AdmissionConfig| healthy().with_admission_version(config).version;
    assert_ne!(with(&closed), plain);
    assert_ne!(with(&closed), with(&open));
    assert_eq!(with(&closed), with(&closed));
}
