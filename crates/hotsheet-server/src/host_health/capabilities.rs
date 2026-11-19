//! Machine capabilities (HS2-ZY4Z4G, `docs/26-orchestration-projects.md` §26.10): what this
//! machine *is*, for plan steps that require an OS, an architecture, a GPU, an AI tool or
//! a toolchain.
//!
//! Detected when a capacity request first asks, then kept for [`CAPABILITIES_TTL`]; never
//! on a timer and never per request. OS, architecture, CPU model and GPU come from cheap
//! platform reads (`sysctl` on macOS, `/proc`, `/etc/os-release` and `/sys` on Linux, the
//! registry on Windows). Versions come from one bounded `--version` probe per program, run
//! in parallel, and only while `HOTSHEET_AI_TOOL_PROBES` allows probing; the output is cut
//! down to a bare version token, so no free text leaves the machine (HS2-SW5ZK3).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hotsheet_ticketing::orchestration::{
    GpuCapability, HostCapabilities, MAX_MODEL_CHARS, MAX_VERSION_CHARS, OsFamily, ToolVersion,
    ToolchainCapability, normalize_arch,
};

/// How long one detection answers further requests.
pub(crate) const CAPABILITIES_TTL: Duration = Duration::from_secs(10 * 60);
/// The deadline of one version probe.
const VERSION_PROBE: Duration = Duration::from_secs(5);

/// The bare version in a program's `--version` output: the first run of digits and dots
/// that holds a dot, with a `-`/`+` suffix of letters, digits and dots, at most 32
/// characters. `git version 2.39.5 (Apple Git-154)` is `2.39.5`, `go version go1.23.2` is
/// `1.23.2`. `None` when the text holds none.
pub(crate) fn parse_version(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        let preceded_by_digit = start > 0 && bytes[start - 1].is_ascii_digit();
        if !bytes[start].is_ascii_digit() || preceded_by_digit {
            start += 1;
            continue;
        }
        let mut end = start;
        while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
            end += 1;
        }
        let core = text[start..end].trim_end_matches('.');
        if core.contains('.') {
            let mut version = core.to_owned();
            let rest = &text[start + core.len()..];
            if let Some(suffix) = rest
                .strip_prefix('-')
                .or_else(|| rest.strip_prefix('+'))
                .filter(|suffix| {
                    suffix
                        .as_bytes()
                        .first()
                        .is_some_and(u8::is_ascii_alphanumeric)
                })
            {
                let tail: String = suffix
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '.')
                    .collect();
                version.push(rest.as_bytes()[0] as char);
                version.push_str(tail.trim_end_matches('.'));
            }
            version.truncate(MAX_VERSION_CHARS);
            return Some(version);
        }
        start = end.max(start + 1);
    }
    None
}

/// Printable text of at most `max` characters with runs of white space collapsed, or
/// `None` when nothing printable remains.
pub(crate) fn bounded_text(text: &str, max: usize) -> Option<String> {
    let collapsed = text
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let bounded: String = collapsed.chars().take(max).collect();
    (!bounded.is_empty()).then_some(bounded)
}

/// Linux `/etc/os-release`: the distribution `ID` and its `VERSION_ID`, unquoted.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))] // HS2-ZY4Z4G: pure Linux parsers run in every host's unit suite.
pub(crate) fn parse_os_release(text: &str) -> (Option<String>, Option<String>) {
    let value = |key: &str| {
        text.lines().find_map(|line| {
            let raw = line.trim().strip_prefix(key)?.strip_prefix('=')?;
            let unquoted = raw.trim().trim_matches(|c| c == '"' || c == '\'');
            bounded_text(unquoted, MAX_VERSION_CHARS)
        })
    };
    (
        value("ID").map(|id| id.to_ascii_lowercase()),
        value("VERSION_ID"),
    )
}

/// Linux `/proc/cpuinfo`: the first `model name` (x86) or, on Arm, the `Model` line.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))] // HS2-ZY4Z4G: pure Linux parsers run in every host's unit suite.
pub(crate) fn parse_cpuinfo_model(text: &str) -> Option<String> {
    let field = |name: &str| {
        text.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == name)
                .then(|| bounded_text(value, MAX_MODEL_CHARS))
                .flatten()
        })
    };
    field("model name").or_else(|| field("Model"))
}

/// What a PCI display device's vendor id says: `Some(Some(vendor))` for a GPU,
/// `Some(None)` for a virtual display adapter (not a GPU), `None` when the id is not one.
pub(crate) fn pci_gpu_vendor(id: &str) -> Option<Option<&'static str>> {
    let hex = id.trim().trim_start_matches("0x").trim_start_matches("0X");
    let code = u16::from_str_radix(hex, 16).ok()?;
    Some(match code {
        0x10de => Some("nvidia"),
        0x1002 | 0x1022 => Some("amd"),
        0x8086 => Some("intel"),
        0x5143 => Some("qualcomm"),
        0x106b => Some("apple"),
        // virtio, VMware, QEMU/Bochs, Red Hat QXL, VirtualBox, Hyper-V, Parallels.
        0x1af4 | 0x15ad | 0x1234 | 0x1b36 | 0x80ee | 0x1414 | 0x1ab8 => None,
        _ => Some("other"),
    })
}

/// What a Windows display adapter's provider or description says, as for
/// [`pci_gpu_vendor`]: `None` for a virtual or basic display adapter.
#[cfg_attr(not(windows), allow(dead_code))] // HS2-ZY4Z4G: pure Windows parsers run in every host's unit suite.
pub(crate) fn display_adapter_vendor(text: &str) -> Option<&'static str> {
    let lower = text.to_ascii_lowercase();
    let virtual_adapter = [
        "basic display",
        "basic render",
        "hyper-v",
        "vmware",
        "virtualbox",
        "parallels",
        "red hat",
        "qxl",
        "virtio",
        "remote display",
        "indirect display",
    ];
    if virtual_adapter.iter().any(|name| lower.contains(name)) {
        return None;
    }
    Some(if lower.contains("nvidia") {
        "nvidia"
    } else if lower.contains("advanced micro devices")
        || lower.contains("amd")
        || lower.contains("radeon")
    {
        "amd"
    } else if lower.contains("intel") {
        "intel"
    } else if lower.contains("qualcomm") || lower.contains("adreno") {
        "qualcomm"
    } else {
        "other"
    })
}

/// Windows: `major.minor.build` from the registry's current-version numbers.
#[cfg_attr(not(windows), allow(dead_code))] // HS2-ZY4Z4G: pure Windows parsers run in every host's unit suite.
pub(crate) fn windows_version(major: u32, minor: u32, build: &str) -> Option<String> {
    let build = build.trim();
    (!build.is_empty() && build.bytes().all(|b| b.is_ascii_digit()))
        .then(|| format!("{major}.{minor}.{build}"))
}

/// The program and arguments that report a toolchain's version. A name the table does not
/// know is its own program, asked with `--version`.
pub(crate) fn toolchain_command(name: &str, os: OsFamily) -> (String, Vec<&'static str>) {
    let (program, args): (&str, &[&'static str]) = match name {
        "python" if os == OsFamily::Windows => ("python", &["--version"]),
        "python" => ("python3", &["--version"]),
        "xcode" => ("xcodebuild", &["-version"]),
        "go" => ("go", &["version"]),
        "java" => ("java", &["-version"]),
        other => (other, &["--version"]),
    };
    (program.to_owned(), args.to_vec())
}

/// The first executable named `name` on `path`, trying each of `extensions` (the
/// Windows `PATHEXT` entries; an empty list off Windows) after the bare name.
pub(crate) fn find_program(name: &str, path: &OsStr, extensions: &[String]) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .find_map(|dir| {
            std::iter::once(String::new())
                .chain(extensions.iter().cloned())
                .map(|extension| dir.join(format!("{name}{extension}")))
                .find(|candidate| is_program(candidate))
        })
}

#[cfg(unix)]
fn is_program(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_program(path: &Path) -> bool {
    path.is_file()
}

fn path_extensions() -> Vec<String> {
    if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(|extension| extension.to_ascii_lowercase())
            .collect()
    } else {
        Vec::new()
    }
}

/// One program's presence and version. Without probing, a program found on `PATH` is
/// installed with no version. With probing, it is installed when its version command
/// succeeds, so a launcher stub that only says the tool is missing does not count.
fn program_version(program: &str, args: &[&str], probes: bool) -> (bool, Option<String>) {
    let Some(path) = std::env::var_os("PATH") else {
        return (false, None);
    };
    let Some(found) = find_program(program, &path, &path_extensions()) else {
        return (false, None);
    };
    if !probes {
        return (true, None);
    }
    let mut command = hotsheet_ticketing::git::launch(&found);
    command.args(args);
    match hotsheet_aitools::probe::run_probe_within(&mut command, VERSION_PROBE) {
        Ok(output) if output.status.success() => {
            let version = parse_version(&String::from_utf8_lossy(&output.stdout))
                .or_else(|| parse_version(&String::from_utf8_lossy(&output.stderr)));
            (true, version)
        }
        _ => (false, None),
    }
}

/// The platform facts that need no subprocess.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PlatformFacts {
    pub(crate) os_version: Option<String>,
    pub(crate) os_distribution: Option<String>,
    /// The native architecture, when the platform says it apart from this build's.
    pub(crate) arch: Option<String>,
    pub(crate) cpu_model: Option<String>,
    pub(crate) gpu: Option<GpuCapability>,
}

/// Every drivable AI tool installed here, with its version, and the declared toolchains.
/// The platform facts come from [`platform_facts`].
pub(crate) fn detect(
    plugin_dirs: &[PathBuf],
    toolchains: &[String],
    probes: bool,
) -> HostCapabilities {
    let os = OsFamily::current();
    let facts = platform_facts();
    let tools: Vec<(String, String)> = hotsheet_plugins::detected_drivable_plugins(plugin_dirs)
        .iter()
        .filter_map(|plugin| {
            let binary = plugin.manifest.detection.binaries.first()?;
            Some((plugin.id().to_string(), binary.clone()))
        })
        .collect();
    let (ai_tools, toolchains) = std::thread::scope(|scope| {
        let tool_probes: Vec<_> = tools
            .iter()
            .map(|(tool, binary)| {
                scope.spawn(move || ToolVersion {
                    tool: tool.clone(),
                    version: program_version(binary, &["--version"], probes).1,
                })
            })
            .collect();
        let chain_probes: Vec<_> = toolchains
            .iter()
            .map(|name| {
                scope.spawn(move || {
                    let (program, args) = toolchain_command(name, os);
                    let (installed, version) = program_version(&program, &args, probes);
                    ToolchainCapability {
                        name: name.clone(),
                        installed,
                        version,
                    }
                })
            })
            .collect();
        (
            tool_probes
                .into_iter()
                .filter_map(|handle| handle.join().ok())
                .collect::<Vec<_>>(),
            chain_probes
                .into_iter()
                .filter_map(|handle| handle.join().ok())
                .collect::<Vec<_>>(),
        )
    });
    HostCapabilities {
        os,
        os_version: facts.os_version,
        os_distribution: facts.os_distribution,
        arch: facts
            .arch
            .and_then(|arch| normalize_arch(&arch))
            .or_else(|| normalize_arch(std::env::consts::ARCH))
            .unwrap_or_else(|| "unknown".into()),
        cpu_model: facts.cpu_model,
        gpu: facts.gpu,
        ai_tools: Some(ai_tools),
        toolchains: Some(toolchains),
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn platform_facts() -> PlatformFacts {
    // Apple silicon reports `hw.optional.arm64` even to a translated (Rosetta) process.
    let arm64 = macos::sysctl_u32("hw.optional.arm64") == Some(1);
    PlatformFacts {
        os_version: macos::sysctl_string("kern.osproductversion")
            .and_then(|version| bounded_text(&version, MAX_VERSION_CHARS)),
        os_distribution: None,
        arch: Some(
            if arm64 {
                "arm64"
            } else {
                std::env::consts::ARCH
            }
            .to_owned(),
        ),
        cpu_model: macos::sysctl_string("machdep.cpu.brand_string")
            .and_then(|model| bounded_text(&model, MAX_MODEL_CHARS)),
        // Every Apple silicon Mac has its integrated GPU. An Intel Mac's GPUs need the
        // I/O registry, which is not read: unknown.
        gpu: arm64.then(|| GpuCapability::from_vendors(["apple".to_owned()])),
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::CString;

    fn sysctl_bytes(name: &str) -> Option<Vec<u8>> {
        let name = CString::new(name).ok()?;
        let mut size: libc::size_t = 0;
        // SAFETY: a null buffer asks only for the value's size.
        let sized = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if sized != 0 || size == 0 || size > 4096 {
            return None;
        }
        let mut buffer = vec![0u8; size];
        // SAFETY: `buffer` holds `size` writable bytes, as `size` says.
        let read = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                buffer.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        (read == 0).then(|| {
            buffer.truncate(size);
            buffer
        })
    }

    pub(super) fn sysctl_string(name: &str) -> Option<String> {
        let bytes = sysctl_bytes(name)?;
        let text = bytes.split(|byte| *byte == 0).next()?;
        String::from_utf8(text.to_vec()).ok()
    }

    pub(super) fn sysctl_u32(name: &str) -> Option<u32> {
        let bytes: [u8; 4] = sysctl_bytes(name)?.get(..4)?.try_into().ok()?;
        Some(u32::from_ne_bytes(bytes))
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn platform_facts() -> PlatformFacts {
    let (distribution, version) = std::fs::read_to_string("/etc/os-release")
        .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
        .map(|text| parse_os_release(&text))
        .unwrap_or_default();
    PlatformFacts {
        os_version: version,
        os_distribution: distribution,
        arch: None,
        cpu_model: std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|text| parse_cpuinfo_model(&text)),
        gpu: linux_gpu(Path::new("/sys/class/drm"), Path::new("/dev/nvidia0")),
    }
}

/// Linux: the vendors of the DRM cards under `drm` (`card<N>/device/vendor`), plus NVIDIA
/// when its device node exists. `None` when there is no DRM directory to read.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))] // HS2-ZY4Z4G: pure Linux readers run in every host's unit suite.
pub(crate) fn linux_gpu(drm: &Path, nvidia_node: &Path) -> Option<GpuCapability> {
    let entries = std::fs::read_dir(drm).ok()?;
    let mut vendors = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // `card0`, not a connector such as `card0-HDMI-A-1`.
        if !name
            .strip_prefix("card")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        if let Some(Some(vendor)) = std::fs::read_to_string(entry.path().join("device/vendor"))
            .ok()
            .and_then(|id| pci_gpu_vendor(&id))
        {
            vendors.push(vendor.to_owned());
        }
    }
    if nvidia_node.exists() {
        vendors.push("nvidia".into());
    }
    Some(GpuCapability::from_vendors(vendors))
}

#[cfg(windows)]
pub(crate) fn platform_facts() -> PlatformFacts {
    use windows_registry as registry;
    const CURRENT: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let os_version = match (
        registry::dword(CURRENT, "CurrentMajorVersionNumber"),
        registry::dword(CURRENT, "CurrentMinorVersionNumber"),
        registry::string(CURRENT, "CurrentBuildNumber"),
    ) {
        (Some(major), Some(minor), Some(build)) => windows_version(major, minor, &build),
        _ => None,
    };
    let cpu_model = registry::string(
        r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
        "ProcessorNameString",
    )
    .and_then(|model| bounded_text(&model, MAX_MODEL_CHARS));
    // Display adapters: the device class {4d36e968-...}, one numbered key per adapter.
    const DISPLAY: &str =
        r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let mut read_any = false;
    let mut vendors = Vec::new();
    for index in 0..16 {
        let key = format!(r"{DISPLAY}\{index:04}");
        let text = [
            registry::string(&key, "ProviderName"),
            registry::string(&key, "DriverDesc"),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
        if text.is_empty() {
            continue;
        }
        read_any = true;
        if let Some(vendor) = display_adapter_vendor(&text) {
            vendors.push(vendor.to_owned());
        }
    }
    PlatformFacts {
        os_version,
        os_distribution: None,
        arch: windows_registry::native_arch().map(str::to_owned),
        cpu_model,
        gpu: read_any.then(|| GpuCapability::from_vendors(vendors)),
    }
}

#[cfg(windows)]
mod windows_registry {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
    };

    fn wide(text: &str) -> Vec<u16> {
        std::ffi::OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub(super) fn string(key: &str, value: &str) -> Option<String> {
        let (key, value) = (wide(key), wide(value));
        let mut buffer = vec![0u16; 512];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: both names are NUL-terminated, and `buffer` holds `size` writable bytes.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != 0 {
            return None;
        }
        let chars = (size as usize / 2).min(buffer.len());
        let text = String::from_utf16_lossy(&buffer[..chars]);
        let text = text.trim_end_matches('\0').trim().to_owned();
        (!text.is_empty()).then_some(text)
    }

    pub(super) fn dword(key: &str, value: &str) -> Option<u32> {
        let (key, value) = (wide(key), wide(value));
        let mut data = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: both names are NUL-terminated, and `data` is a writable DWORD.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut data as *mut u32).cast(),
                &mut size,
            )
        };
        (status == 0).then_some(data)
    }

    /// The machine's own architecture, also when this build runs emulated (an x64
    /// server on Windows on Arm).
    pub(super) fn native_arch() -> Option<&'static str> {
        use windows_sys::Win32::System::SystemInformation::{
            IMAGE_FILE_MACHINE_AMD64, IMAGE_FILE_MACHINE_ARM64, IMAGE_FILE_MACHINE_UNKNOWN,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};
        let (mut process, mut native) = (IMAGE_FILE_MACHINE_UNKNOWN, IMAGE_FILE_MACHINE_UNKNOWN);
        // SAFETY: the pseudo-handle of this process needs no closing; both outputs are
        // writable.
        if unsafe { IsWow64Process2(GetCurrentProcess(), &mut process, &mut native) } == 0 {
            return None;
        }
        match native {
            IMAGE_FILE_MACHINE_ARM64 => Some("arm64"),
            IMAGE_FILE_MACHINE_AMD64 => Some("x86_64"),
            _ => None,
        }
    }
}

/// No platform reader here: only the family and this build's architecture are known.
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub(crate) fn platform_facts() -> PlatformFacts {
    PlatformFacts::default()
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;
