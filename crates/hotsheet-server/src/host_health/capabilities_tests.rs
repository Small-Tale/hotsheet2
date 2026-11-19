//! HS2-ZY4Z4G: the pure capability parsers over captured platform text, the platform
//! readers of this host, and one real detection. Synthetic values only.

use super::*;

#[test]
fn hs2_zy4z4g_versions_are_cut_down_to_a_bare_token() {
    let cases = [
        ("git version 2.39.5 (Apple Git-154)", Some("2.39.5")),
        ("v22.11.0\n", Some("22.11.0")),
        ("cargo 1.97.0 (a1b2c3d4e 2026-09-01)", Some("1.97.0")),
        ("Docker version 27.3.1, build ce12230", Some("27.3.1")),
        ("Xcode 16.1\nBuild version 16B40", Some("16.1")),
        ("go version go1.23.2 darwin/arm64", Some("1.23.2")),
        ("Python 3.12.4", Some("3.12.4")),
        (
            "openjdk version \"21.0.2\" 2024-01-16\nOpenJDK Runtime",
            Some("21.0.2"),
        ),
        ("8.0.403", Some("8.0.403")),
        ("2.0.14 (Claude Code)", Some("2.0.14")),
        ("codex-cli 0.46.0-alpha.3", Some("0.46.0-alpha.3")),
        ("tool 1.0.0+build.7.", Some("1.0.0+build.7")),
        ("tool 3.1.", Some("3.1")),
        ("x86_64 2024-01-16 no dots here", None),
        ("", None),
        ("v12", None),
    ];
    for (text, expected) in cases {
        assert_eq!(parse_version(text).as_deref(), expected, "{text:?}");
    }
    let long = format!("1.{}", "2".repeat(80));
    assert_eq!(parse_version(&long).map(|v| v.len()), Some(32));
    // A digit run that is part of a longer word is not a version start.
    assert_eq!(parse_version("abc12x 4.5").as_deref(), Some("4.5"));
}

#[test]
fn hs2_zy4z4g_text_is_bounded_and_printable() {
    assert_eq!(
        bounded_text("  Apple\tM4   Pro \n", 96).as_deref(),
        Some("Apple M4 Pro")
    );
    assert_eq!(bounded_text("a\u{7}b", 96).as_deref(), Some("a b"));
    assert_eq!(bounded_text(" \n ", 96), None);
    assert_eq!(bounded_text("abcdef", 3).as_deref(), Some("abc"));
}

#[test]
fn hs2_zy4z4g_linux_os_release_and_cpuinfo_parse() {
    let release = "NAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nID=ubuntu\nID_LIKE=debian\n";
    assert_eq!(
        parse_os_release(release),
        (Some("ubuntu".into()), Some("24.04".into()))
    );
    assert_eq!(
        parse_os_release("ID='Fedora'\nVERSION_ID=41"),
        (Some("fedora".into()), Some("41".into()))
    );
    // A rolling distribution has no VERSION_ID.
    assert_eq!(parse_os_release("ID=arch\n"), (Some("arch".into()), None));
    assert_eq!(parse_os_release(""), (None, None));
    let x86 = "processor\t: 0\nmodel name\t: Synthetic(R) CPU @ 3.00GHz\nprocessor\t: 1\nmodel name\t: Synthetic(R) CPU @ 3.00GHz\n";
    assert_eq!(
        parse_cpuinfo_model(x86).as_deref(),
        Some("Synthetic(R) CPU @ 3.00GHz")
    );
    let arm = "processor\t: 0\nBogoMIPS\t: 108.00\nCPU part\t: 0xd0c\n\nModel\t\t: Synthetic Board Rev 1.0\n";
    assert_eq!(
        parse_cpuinfo_model(arm).as_deref(),
        Some("Synthetic Board Rev 1.0")
    );
    assert_eq!(parse_cpuinfo_model("processor : 0\n"), None);
}

#[test]
fn hs2_zy4z4g_gpu_vendors_from_pci_ids_and_windows_adapters() {
    for (id, expected) in [
        ("0x10de\n", Some(Some("nvidia"))),
        ("0x1002", Some(Some("amd"))),
        ("0x8086", Some(Some("intel"))),
        ("0x5143", Some(Some("qualcomm"))),
        ("0x106b", Some(Some("apple"))),
        ("0x1a03", Some(Some("other"))),
        ("0x1af4", Some(None)),
        ("0x15ad", Some(None)),
        ("0x1234", Some(None)),
        ("0x1414", Some(None)),
        ("not hex", None),
    ] {
        assert_eq!(pci_gpu_vendor(id), expected, "{id}");
    }
    for (text, expected) in [
        ("NVIDIA NVIDIA GeForce RTX 4090", Some("nvidia")),
        (
            "Advanced Micro Devices, Inc. AMD Radeon(TM) Graphics",
            Some("amd"),
        ),
        ("Intel Corporation Intel(R) UHD Graphics 770", Some("intel")),
        (
            "Qualcomm Technologies Inc. Qualcomm(R) Adreno(TM) X1-85 GPU",
            Some("qualcomm"),
        ),
        ("Microsoft Microsoft Basic Display Adapter", None),
        ("Parallels Parallels Display Adapter (WDDM)", None),
        ("Microsoft Hyper-V Video", None),
        ("Some Vendor Display", Some("other")),
    ] {
        assert_eq!(display_adapter_vendor(text), expected, "{text}");
    }
}

#[test]
fn hs2_zy4z4g_linux_gpu_reads_drm_cards_and_the_nvidia_node() {
    let root = tempfile::tempdir().unwrap();
    let drm = root.path().join("drm");
    // No DRM directory: unknown, not absent.
    assert_eq!(linux_gpu(&drm, &root.path().join("nvidia0")), None);
    std::fs::create_dir_all(&drm).unwrap();
    assert_eq!(
        linux_gpu(&drm, &root.path().join("nvidia0")),
        Some(GpuCapability::default())
    );
    for (card, vendor) in [
        ("card0", "0x1af4"),
        ("card1", "0x8086"),
        ("card1-HDMI-A-1", "0x10de"),
    ] {
        let device = drm.join(card).join("device");
        std::fs::create_dir_all(&device).unwrap();
        std::fs::write(device.join("vendor"), format!("{vendor}\n")).unwrap();
    }
    let node = root.path().join("nvidia0");
    assert_eq!(
        linux_gpu(&drm, &node),
        Some(GpuCapability::from_vendors(["intel".to_owned()]))
    );
    std::fs::write(&node, "").unwrap();
    let found = linux_gpu(&drm, &node).unwrap();
    assert!(found.present);
    assert_eq!(found.vendors, ["intel", "nvidia"]);
}

#[test]
fn hs2_zy4z4g_windows_versions_and_toolchain_commands() {
    assert_eq!(
        windows_version(10, 0, "26100").as_deref(),
        Some("10.0.26100")
    );
    assert_eq!(windows_version(10, 0, ""), None);
    assert_eq!(windows_version(10, 0, "26100a"), None);
    let command = |name, os| {
        let (program, args) = toolchain_command(name, os);
        (program, args.join(" "))
    };
    assert_eq!(
        command("python", OsFamily::Linux),
        ("python3".into(), "--version".into())
    );
    assert_eq!(
        command("python", OsFamily::Windows),
        ("python".into(), "--version".into())
    );
    assert_eq!(
        command("xcode", OsFamily::Macos),
        ("xcodebuild".into(), "-version".into())
    );
    assert_eq!(
        command("go", OsFamily::Linux),
        ("go".into(), "version".into())
    );
    assert_eq!(
        command("java", OsFamily::Linux),
        ("java".into(), "-version".into())
    );
    assert_eq!(
        command("bazel", OsFamily::Linux),
        ("bazel".into(), "--version".into())
    );
}

#[test]
fn hs2_zy4z4g_programs_are_found_on_path_with_windows_extensions() {
    let root = tempfile::tempdir().unwrap();
    let (first, second) = (root.path().join("a"), root.path().join("b"));
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    let make = |path: &Path| {
        std::fs::write(path, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    };
    make(&second.join("tool"));
    make(&second.join("shim.cmd"));
    let path = std::env::join_paths([&first, &second]).unwrap();
    assert_eq!(find_program("tool", &path, &[]), Some(second.join("tool")));
    assert_eq!(
        find_program("shim", &path, &[".exe".into(), ".cmd".into()]),
        Some(second.join("shim.cmd"))
    );
    assert_eq!(find_program("shim", &path, &[]), None);
    assert_eq!(find_program("missing", &path, &[".exe".into()]), None);
    // A file that cannot be run is not a program.
    #[cfg(unix)]
    {
        std::fs::write(first.join("plain"), "").unwrap();
        assert_eq!(find_program("plain", &path, &[]), None);
    }
}

/// This host's own readers answer something for its platform, and a real detection
/// without probes reports presence only, never a version.
#[test]
fn hs2_zy4z4g_this_host_reports_what_it_is() {
    let facts = platform_facts();
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        assert!(facts.os_version.is_some(), "{facts:?}");
        assert!(facts.arch.is_some(), "{facts:?}");
    }
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        assert_eq!(
            facts.gpu,
            Some(GpuCapability::from_vendors(["apple".to_owned()]))
        );
    }
    let report = detect(
        &[],
        &["git".to_owned(), "no-such-toolchain-zy4z4g".to_owned()],
        false,
    );
    assert_eq!(report.os, OsFamily::current());
    assert!(
        report.arch == "arm64" || report.arch == "x86_64",
        "{}",
        report.arch
    );
    let toolchains = report.toolchains.unwrap();
    assert_eq!(toolchains.len(), 2);
    assert!(
        toolchains
            .iter()
            .all(|toolchain| toolchain.version.is_none())
    );
    assert!(!toolchains[1].installed);
    assert!(
        report
            .ai_tools
            .unwrap()
            .iter()
            .all(|tool| tool.version.is_none())
    );
}
