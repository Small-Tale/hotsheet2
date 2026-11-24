//! HS2-ZY4Z4G: matching plan-step requirements against a machine's capabilities, for
//! every requirement kind in each of its states (met, unmet, unknown), and the
//! canonical form of a requirement. Synthetic values only.

use super::*;
use crate::orchestration::{
    AdmissionConfig, CpuCapacity, DiskCapacity, HostObservation, MemoryCapacity, ToolAvailability,
    capacity_version,
};

fn tool(name: &str, installed: bool, configured: bool) -> ToolAvailability {
    ToolAvailability {
        tool: name.into(),
        installed,
        configured,
    }
}

fn toolchain(name: &str, installed: bool) -> ToolchainCapability {
    ToolchainCapability {
        name: name.into(),
        installed,
        version: installed.then(|| "1.2.3".into()),
    }
}

/// A Linux arm64 machine with 32 GiB, one NVIDIA GPU, codex usable, claude installed
/// but not enabled for the checkout, and cargo but not docker.
fn capabilities() -> HostCapabilities {
    HostCapabilities {
        os: OsFamily::Linux,
        os_version: Some("24.04".into()),
        os_distribution: Some("ubuntu".into()),
        arch: "arm64".into(),
        cpu_model: Some("Synthetic CPU".into()),
        gpu: Some(GpuCapability::from_vendors(["nvidia".to_owned()])),
        ai_tools: Some(vec![ToolVersion {
            tool: "codex".into(),
            version: Some("0.1.0".into()),
        }]),
        toolchains: Some(vec![toolchain("cargo", true), toolchain("docker", false)]),
    }
}

fn snapshot(capabilities: Option<HostCapabilities>, total_mb: Option<u64>) -> CapacitySnapshot {
    CapacitySnapshot::new(
        "server-1".into(),
        "checkout-1".into(),
        HostObservation {
            sampled_at_ms: Some(1),
            memory: MemoryCapacity {
                free_mb: Some(8192),
                total_mb,
            },
            cpu: CpuCapacity {
                load_percent: Some(10),
                logical_cpus: Some(8),
            },
            disk: DiskCapacity {
                free_mb: Some(1),
                total_mb: Some(2),
            },
            tools: Some(vec![
                tool("codex", true, true),
                tool("claude", true, false),
                tool("gemini", false, true),
            ]),
            sessions: Some(Vec::new()),
            capabilities,
        },
        Vec::new(),
        None,
    )
}

fn machine() -> CapacitySnapshot {
    snapshot(Some(capabilities()), Some(32 * 1024))
}

fn misses(
    requirements: StepRequirements,
    snapshot: &CapacitySnapshot,
) -> Vec<(String, RequirementState)> {
    requirement_misses(&requirements.normalized().unwrap(), snapshot)
        .into_iter()
        .map(|miss| (miss.requirement, miss.state))
        .collect()
}

fn req(specs: &[&str]) -> StepRequirements {
    let mut requirements = StepRequirements::default();
    for spec in specs {
        requirements.add(spec).unwrap();
    }
    requirements
}

use RequirementState::{Unknown, Unmet};

fn one(name: &str, state: RequirementState) -> Vec<(String, RequirementState)> {
    vec![(name.to_owned(), state)]
}

#[test]
fn hs2_zy4z4g_no_requirements_are_met_by_any_machine_even_an_unknown_one() {
    assert!(misses(StepRequirements::default(), &machine()).is_empty());
    assert!(misses(StepRequirements::default(), &snapshot(None, None)).is_empty());
}

#[test]
fn hs2_zy4z4g_os_is_met_by_any_listed_family_and_unknown_without_a_report() {
    assert!(misses(req(&["os=linux"]), &machine()).is_empty());
    assert!(misses(req(&["os=windows", "os=linux"]), &machine()).is_empty());
    assert_eq!(
        misses(req(&["os=windows"]), &machine()),
        one("os:windows", Unmet)
    );
    assert_eq!(
        misses(req(&["os=windows", "os=darwin"]), &machine()),
        one("os:macos|windows", Unmet)
    );
    assert_eq!(
        misses(req(&["os=linux"]), &snapshot(None, Some(1))),
        one("os:linux", Unknown)
    );
}

#[test]
fn hs2_zy4z4g_arch_is_normalized_on_both_sides() {
    assert!(misses(req(&["arch=aarch64"]), &machine()).is_empty());
    let mut peer = capabilities();
    peer.arch = "AArch64".into();
    assert!(misses(req(&["arch=arm64"]), &snapshot(Some(peer), Some(1))).is_empty());
    assert_eq!(
        misses(req(&["arch=amd64"]), &machine()),
        one("arch:x86_64", Unmet)
    );
    assert_eq!(
        misses(req(&["arch=x64", "arch=arm64"]), &machine()),
        Vec::<(String, RequirementState)>::new()
    );
    assert_eq!(
        misses(req(&["arch=arm64"]), &snapshot(None, Some(1))),
        one("arch:arm64", Unknown)
    );
}

#[test]
fn hs2_zy4z4g_min_memory_allows_what_the_os_reserves_and_no_more() {
    // 32 GiB installed, as an OS reports it after its reservations.
    for total_mb in [32 * 1024, 31_836, 30_720] {
        assert!(
            misses(
                req(&["min_memory_gb=32"]),
                &snapshot(Some(capabilities()), Some(total_mb))
            )
            .is_empty(),
            "{total_mb} MiB meets 32 GB"
        );
    }
    assert_eq!(
        misses(
            req(&["min_memory_gb=32"]),
            &snapshot(Some(capabilities()), Some(30_719))
        ),
        one("min_memory_gb:32", Unmet)
    );
    assert_eq!(
        misses(req(&["min_memory_gb=64"]), &machine()),
        one("min_memory_gb:64", Unmet)
    );
    // Total memory is part of the capacity snapshot, not the capabilities report.
    assert!(misses(req(&["min_memory_gb=16"]), &snapshot(None, Some(16 * 1024))).is_empty());
    assert_eq!(
        misses(
            req(&["min_memory_gb=16"]),
            &snapshot(Some(capabilities()), None)
        ),
        one("min_memory_gb:16", Unknown)
    );
}

#[test]
fn hs2_zy4z4g_gpu_any_or_a_vendor() {
    assert!(misses(req(&["gpu=any"]), &machine()).is_empty());
    assert!(misses(req(&["gpu=NVIDIA"]), &machine()).is_empty());
    assert_eq!(misses(req(&["gpu=amd"]), &machine()), one("gpu:amd", Unmet));
    let mut none = capabilities();
    none.gpu = Some(GpuCapability::from_vendors(Vec::new()));
    assert!(!none.gpu.as_ref().unwrap().present);
    assert_eq!(
        misses(req(&["gpu=any"]), &snapshot(Some(none), Some(1))),
        one("gpu:any", Unmet)
    );
    let mut unread = capabilities();
    unread.gpu = None;
    assert_eq!(
        misses(req(&["gpu=any"]), &snapshot(Some(unread), Some(1))),
        one("gpu:any", Unknown)
    );
    assert_eq!(
        misses(req(&["gpu=intel"]), &snapshot(None, Some(1))),
        one("gpu:intel", Unknown)
    );
}

#[test]
fn hs2_zy4z4g_an_ai_tool_must_be_installed_and_enabled_for_the_checkout() {
    assert!(misses(req(&["tool=codex"]), &machine()).is_empty());
    // Installed, but the checkout opted out of it.
    assert_eq!(
        misses(req(&["tool=claude"]), &machine()),
        one("tool:claude", Unmet)
    );
    // Enabled, but its program is missing.
    assert_eq!(
        misses(req(&["tool=gemini"]), &machine()),
        one("tool:gemini", Unmet)
    );
    // Not a tool the host knows.
    assert_eq!(
        misses(req(&["tool=other"]), &machine()),
        one("tool:other", Unmet)
    );
    let mut unread = machine();
    unread.tools = None;
    assert_eq!(
        misses(req(&["tool=codex"]), &unread),
        one("tool:codex", Unknown)
    );
    // Every listed tool is needed.
    assert_eq!(
        misses(req(&["tool=codex", "tool=claude"]), &machine()),
        one("tool:claude", Unmet)
    );
}

#[test]
fn hs2_zy4z4g_a_toolchain_the_owner_did_not_declare_is_unknown_not_missing() {
    assert!(misses(req(&["toolchain=cargo"]), &machine()).is_empty());
    assert_eq!(
        misses(req(&["toolchain=docker"]), &machine()),
        one("toolchain:docker", Unmet)
    );
    assert_eq!(
        misses(req(&["toolchain=xcode"]), &machine()),
        one("toolchain:xcode", Unknown)
    );
    let mut unread = capabilities();
    unread.toolchains = None;
    assert_eq!(
        misses(req(&["toolchain=cargo"]), &snapshot(Some(unread), Some(1))),
        one("toolchain:cargo", Unknown)
    );
    assert_eq!(
        misses(req(&["toolchain=cargo"]), &snapshot(None, Some(1))),
        one("toolchain:cargo", Unknown)
    );
}

#[test]
fn hs2_zy4z4g_every_miss_is_named_in_a_fixed_order() {
    let all = req(&[
        "toolchain=xcode",
        "tool=claude",
        "gpu=amd",
        "min_memory_gb=64",
        "arch=x86_64",
        "os=macos",
    ]);
    assert_eq!(
        misses(all.clone(), &machine()),
        vec![
            ("os:macos".to_owned(), Unmet),
            ("arch:x86_64".to_owned(), Unmet),
            ("min_memory_gb:64".to_owned(), Unmet),
            ("gpu:amd".to_owned(), Unmet),
            ("tool:claude".to_owned(), Unmet),
            ("toolchain:xcode".to_owned(), Unknown),
        ]
    );
    // An older peer: only what the snapshot itself says can be judged.
    assert_eq!(
        misses(all, &snapshot(None, Some(64 * 1024))),
        vec![
            ("os:macos".to_owned(), Unknown),
            ("arch:x86_64".to_owned(), Unknown),
            ("gpu:amd".to_owned(), Unknown),
            ("tool:claude".to_owned(), Unmet),
            ("toolchain:xcode".to_owned(), Unknown),
        ]
    );
}

#[test]
fn hs2_zy4z4g_requirements_have_one_canonical_form() {
    let spelled = req(&[
        "os=Windows",
        "os=win",
        "arch=AMD64",
        "arch=x86_64",
        "tool=codex",
        "tool=claude",
        "tool=codex",
        "toolchain=node",
        "toolchain=cargo",
        "gpu=Any",
    ])
    .normalized()
    .unwrap();
    assert_eq!(
        spelled,
        StepRequirements {
            os: vec![OsFamily::Windows],
            arch: vec!["x86_64".into()],
            tools: vec!["claude".into(), "codex".into()],
            toolchains: vec!["cargo".into(), "node".into()],
            min_memory_gb: None,
            gpu: Some("any".into()),
        }
    );
    assert_eq!(spelled.clone().normalized().unwrap(), spelled);
    assert!(StepRequirements::default().is_empty());
    assert!(!spelled.is_empty());
}

#[test]
fn hs2_zy4z4g_malformed_requirements_are_refused() {
    let bad = [
        StepRequirements {
            os: vec![OsFamily::Other],
            ..Default::default()
        },
        StepRequirements {
            arch: vec!["Not An Arch".into()],
            ..Default::default()
        },
        StepRequirements {
            tools: vec!["has space".into()],
            ..Default::default()
        },
        StepRequirements {
            toolchains: vec!["Cargo".into()],
            ..Default::default()
        },
        StepRequirements {
            toolchains: (0..9).map(|n| format!("t{n}")).collect(),
            ..Default::default()
        },
        StepRequirements {
            min_memory_gb: Some(0),
            ..Default::default()
        },
        StepRequirements {
            min_memory_gb: Some(MAX_MIN_MEMORY_GB + 1),
            ..Default::default()
        },
        StepRequirements {
            gpu: Some("tpu".into()),
            ..Default::default()
        },
    ];
    for requirements in bad {
        assert!(
            requirements.clone().normalized().is_err(),
            "{requirements:?} refused"
        );
    }
    let mut requirements = StepRequirements::default();
    for spec in ["os", "os=", "os=beos", "cpu=fast", "min_memory_gb=lots"] {
        assert!(requirements.add(spec).is_err(), "{spec} refused");
    }
    assert!(requirements.is_empty());
}

#[test]
fn hs2_zy4z4g_requirements_and_capabilities_travel_as_json() {
    let requirements: StepRequirements = serde_json::from_value(serde_json::json!({
        "os": ["windows"], "arch": ["arm64"], "tools": ["codex"],
        "toolchains": ["cargo"], "min_memory_gb": 16, "gpu": "any"
    }))
    .unwrap();
    assert_eq!(
        requirements,
        req(&[
            "os=windows",
            "arch=arm64",
            "tool=codex",
            "toolchain=cargo",
            "min_memory_gb=16",
            "gpu=any"
        ])
    );
    assert!(
        serde_json::from_value::<StepRequirements>(serde_json::json!({"cpu": "fast"})).is_err()
    );
    assert_eq!(
        serde_json::to_value(StepRequirements::default()).unwrap(),
        serde_json::json!({})
    );
    // An older peer's snapshot has no capabilities and still reads; one without them
    // serializes without the member.
    let mut older = serde_json::to_value(machine()).unwrap();
    older.as_object_mut().unwrap().remove("capabilities");
    let read: CapacitySnapshot = serde_json::from_value(older).unwrap();
    assert_eq!(read.capabilities, None);
    assert!(
        serde_json::to_value(&read)
            .unwrap()
            .get("capabilities")
            .is_none()
    );
    // A newer peer may add members to the report.
    let mut newer = serde_json::to_value(capabilities()).unwrap();
    newer["future_member"] = serde_json::json!(true);
    assert_eq!(
        serde_json::from_value::<HostCapabilities>(newer).unwrap(),
        capabilities()
    );
}

#[test]
fn hs2_zy4z4g_the_capacity_version_follows_capabilities_only_when_reported() {
    let bare = snapshot(None, Some(1));
    let reported = snapshot(Some(capabilities()), Some(1));
    assert_ne!(capacity_version(&bare), capacity_version(&reported));
    let mut upgraded = capabilities();
    upgraded.toolchains = Some(vec![toolchain("cargo", true), toolchain("docker", true)]);
    assert_ne!(
        capacity_version(&reported),
        capacity_version(&snapshot(Some(upgraded), Some(1)))
    );
    assert_eq!(
        capacity_version(&reported),
        capacity_version(&snapshot(Some(capabilities()), Some(1)))
    );
}

#[test]
fn hs2_zy4z4g_os_and_arch_names() {
    for family in [OsFamily::Macos, OsFamily::Linux, OsFamily::Windows] {
        assert_eq!(OsFamily::parse(family.code()), Some(family));
    }
    assert_eq!(OsFamily::parse("other"), None);
    assert_eq!(OsFamily::Other.code(), "other");
    assert_ne!(OsFamily::current(), OsFamily::Other);
    for (raw, normal) in [
        ("x86_64", "x86_64"),
        ("amd64", "x86_64"),
        ("X64", "x86_64"),
        ("aarch64", "arm64"),
        ("arm64", "arm64"),
        ("riscv64", "riscv64"),
    ] {
        assert_eq!(normalize_arch(raw).as_deref(), Some(normal));
    }
    assert_eq!(normalize_arch(""), None);
    assert_eq!(normalize_arch("has space"), None);
    assert_eq!(HostCapabilities::bare(OsFamily::Linux, "").arch, "unknown");
}

#[test]
fn hs2_zy4z4g_the_owner_declares_the_toolchains_a_machine_reports() {
    let mut config = AdmissionConfig::default();
    assert_eq!(
        config.declared_toolchains(),
        DEFAULT_TOOLCHAINS
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>()
    );
    config.toolchains = Some(vec!["bazel".into()]);
    assert!(config.validate().is_ok());
    assert_eq!(config.declared_toolchains(), ["bazel"]);
    config.toolchains = Some(Vec::new());
    assert!(config.declared_toolchains().is_empty());
    config.toolchains = Some(vec!["Bad Name".into()]);
    assert!(config.validate().is_err());
    config.toolchains = Some((0..=MAX_TOOLCHAINS).map(|n| format!("t{n}")).collect());
    assert!(config.validate().is_err());
}

/// One minimum-version case: the requirement, the reported codex and node versions,
/// and the misses expected.
type VersionCase<'a> = (
    &'a str,
    Option<&'a str>,
    Option<&'a str>,
    Vec<(String, RequirementState)>,
);

/// A machine reporting `codex` at `codex_version` and `node` installed at
/// `node_version` (each `None` when it did not say).
fn versioned(codex_version: Option<&str>, node_version: Option<&str>) -> CapacitySnapshot {
    let mut caps = capabilities();
    caps.ai_tools = Some(vec![ToolVersion {
        tool: "codex".into(),
        version: codex_version.map(Into::into),
    }]);
    caps.toolchains = Some(vec![ToolchainCapability {
        name: "node".into(),
        installed: true,
        version: node_version.map(Into::into),
    }]);
    snapshot(Some(caps), Some(32 * 1024))
}

#[test]
fn hs2_zh9c6w_a_minimum_version_is_met_unmet_or_unknown() {
    // (requirement, codex version, node version, expected misses)
    let cases: Vec<VersionCase<'_>> = vec![
        // Toolchains: equal, higher in a later part, higher in an earlier part.
        ("toolchain=node>=20", None, Some("20.0.0"), vec![]),
        ("toolchain=node>=20", None, Some("v20.11.1"), vec![]),
        ("toolchain=node>=20", None, Some("22"), vec![]),
        (
            "toolchain=node>=20.11",
            None,
            Some("20.9.0"),
            one("toolchain:node>=20.11", Unmet),
        ),
        (
            "toolchain=node>=20",
            None,
            Some("v18.19.0"),
            one("toolchain:node>=20", Unmet),
        ),
        // No version reported, or one without a number: unknown, never a guess.
        (
            "toolchain=node>=20",
            None,
            None,
            one("toolchain:node>=20", Unknown),
        ),
        (
            "toolchain=node>=20",
            None,
            Some("unknown"),
            one("toolchain:node>=20", Unknown),
        ),
        // A bare name ignores the version.
        ("toolchain=node", None, None, vec![]),
        // AI tools: the reported version with its prefix and suffix.
        ("tool=codex>=0.40", Some("codex-cli 0.41.0"), None, vec![]),
        ("tool=codex>=0.40", Some("0.40"), None, vec![]),
        (
            "tool=codex>=0.40",
            Some("0.39.9"),
            None,
            one("tool:codex>=0.40", Unmet),
        ),
        (
            "tool=codex>=0.40",
            Some("0.4"),
            None,
            one("tool:codex>=0.40", Unmet),
        ),
        (
            "tool=codex>=0.40",
            None,
            None,
            one("tool:codex>=0.40", Unknown),
        ),
        // A tool that is not usable is unmet whatever its version.
        (
            "tool=claude>=1",
            Some("9"),
            None,
            one("tool:claude>=1", Unmet),
        ),
    ];
    for (spec, codex, node, expected) in cases {
        assert_eq!(
            misses(req(&[spec]), &versioned(codex, node)),
            expected,
            "{spec} with codex {codex:?}, node {node:?}"
        );
    }
    // A machine that did not report its AI tool versions at all.
    let mut caps = capabilities();
    caps.ai_tools = None;
    assert_eq!(
        misses(req(&["tool=codex>=0.40"]), &snapshot(Some(caps), Some(1))),
        one("tool:codex>=0.40", Unknown)
    );
    // No capabilities at all: a versioned toolchain is unknown, a versioned tool too.
    assert_eq!(
        misses(
            req(&["tool=codex>=0.40", "toolchain=node>=20"]),
            &snapshot(None, Some(1))
        ),
        [
            ("tool:codex>=0.40".to_owned(), Unknown),
            ("toolchain:node>=20".to_owned(), Unknown)
        ]
    );
}

#[test]
fn hs2_zh9c6w_minimum_versions_have_one_canonical_form() {
    let spelled = req(&[
        "toolchain=node",
        "toolchain=node>=18",
        "toolchain=node>= 20.0",
        "toolchain=cargo>=1.079.0",
        "tool=codex>=0.40.0",
        "tool=codex",
        "tool=claude",
    ])
    .normalized()
    .unwrap();
    assert_eq!(spelled.toolchains, ["cargo>=1.79", "node>=20"]);
    assert_eq!(spelled.tools, ["claude", "codex>=0.40"]);
    assert_eq!(spelled.clone().normalized().unwrap(), spelled);
    // The order given does not matter: the highest minimum stays.
    let other = req(&["toolchain=node>=20", "toolchain=node>=18", "toolchain=node"])
        .normalized()
        .unwrap();
    assert_eq!(other.toolchains, ["node>=20"]);
    // A zero version is the version 0.
    assert_eq!(
        req(&["toolchain=node>=0.0"])
            .normalized()
            .unwrap()
            .toolchains,
        ["node>=0"]
    );
    // Malformed versions and names are refused.
    for spec in [
        "toolchain=node>=",
        "toolchain=node>=20.x",
        "toolchain=node>=1.2.3.4.5",
        "toolchain=node>=-1",
        "toolchain=node>=1..2",
        "toolchain=node>=1234567890",
        "toolchain=Node>=20",
        "tool=has space>=1",
        "toolchain=>=20",
    ] {
        assert!(req(&[spec]).normalized().is_err(), "{spec} refused");
    }
    // It travels as the same JSON strings.
    assert_eq!(
        serde_json::to_value(&spelled).unwrap()["toolchains"],
        serde_json::json!(["cargo>=1.79", "node>=20"])
    );
}

#[test]
fn hs2_zh9c6w_versions_compare_part_by_part() {
    assert_eq!(version_parts("v20.11.1"), Some(vec![20, 11, 1]));
    assert_eq!(version_parts("codex-cli 0.41.0"), Some(vec![0, 41, 0]));
    assert_eq!(version_parts("1.79.0 (stable)"), Some(vec![1, 79, 0]));
    assert_eq!(version_parts("none"), None);
    assert!(version_at_least(&[20], &[20, 0, 0]));
    assert!(version_at_least(&[20, 0, 1], &[20]));
    assert!(!version_at_least(&[19, 99], &[20]));
    assert!(version_at_least(&[1, 10], &[1, 9]));
    assert!(!version_at_least(&[1, 9], &[1, 10]));
    assert_eq!(split_minimum("node>=20"), ("node", Some("20")));
    assert_eq!(split_minimum("node"), ("node", None));
}
