//! HS2-ZY4Z4G: what a target machine *is*, and what a plan step needs it to be.
//!
//! A [`HostCapabilities`] report rides in the capacity snapshot (§26.10) beside the
//! room the machine has: its OS family and version, architecture, CPU model, GPU
//! presence, the AI tools installed with their versions, and the toolchains its owner
//! declared. Anything the host could not tell is `None`; nothing is guessed. Names and
//! versions only: no path, command line or free text (HS2-SW5ZK3).
//!
//! A plan step may declare [`StepRequirements`]. [`requirement_misses`] is the pure
//! match of one step's requirements against one snapshot, and names every requirement
//! the machine does not meet or did not report.

use serde::{Deserialize, Serialize};

use super::OrchestrationError;
use super::capacity::CapacitySnapshot;

/// The toolchains a machine reports when its owner declares none (§26.10).
pub const DEFAULT_TOOLCHAINS: &[&str] = &[
    "node", "cargo", "git", "python", "docker", "xcode", "go", "java", "dotnet",
];
/// Toolchains one machine may declare.
pub const MAX_TOOLCHAINS: usize = 16;
/// Values one requirement list (`os`, `arch`, `tools`, `toolchains`) may hold.
pub const MAX_REQUIREMENT_VALUES: usize = 8;
/// The longest version, CPU model or name a report keeps.
pub const MAX_VERSION_CHARS: usize = 32;
pub const MAX_MODEL_CHARS: usize = 96;
/// The largest `min_memory_gb` a step may ask for.
pub const MAX_MIN_MEMORY_GB: u32 = 64 * 1024;
/// A machine meets `min_memory_gb` when its physical memory reaches this share of it
/// (fifteen sixteenths), because every OS reserves part of the installed memory.
pub const MEMORY_TOLERANCE_SIXTEENTHS: u64 = 15;

/// GPU vendors a report names. Virtual display adapters are not GPUs.
pub const GPU_VENDORS: &[&str] = &["amd", "apple", "intel", "nvidia", "other", "qualcomm"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OsFamily {
    // The spellings `OsFamily::parse` accepts are accepted on the wire too.
    #[serde(alias = "mac", alias = "darwin", alias = "osx")]
    Macos,
    Linux,
    #[serde(alias = "win", alias = "win32")]
    Windows,
    /// Any other platform. A requirement cannot name it.
    Other,
}

impl OsFamily {
    pub fn code(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::Other => "other",
        }
    }

    /// The family a requirement names, with the common spellings. `other` is not one.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "macos" | "mac" | "darwin" | "osx" => Some(Self::Macos),
            "linux" => Some(Self::Linux),
            "windows" | "win" | "win32" => Some(Self::Windows),
            _ => None,
        }
    }

    /// The family this build runs on.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Other
        }
    }
}

/// One architecture spelling, normalized: `x86_64` and `arm64` for the two the
/// platforms ship, lowercase otherwise. `None` for anything that is not a bounded name.
pub fn normalize_arch(name: &str) -> Option<String> {
    let lower = name.trim().to_ascii_lowercase();
    match lower.as_str() {
        "x86_64" | "amd64" | "x64" | "x86-64" => Some("x86_64".into()),
        "arm64" | "aarch64" | "armv8" => Some("arm64".into()),
        other if valid_name(other) => Some(other.into()),
        _ => None,
    }
}

/// A lowercase name of letters, digits, `.`, `_` and `-`, at most 32 characters.
fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_VERSION_CHARS
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

/// HS2-ZY4Z4G: a toolchain name as the machine owner declares it and a step requires it.
pub fn valid_toolchain(name: &str) -> Result<(), OrchestrationError> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(OrchestrationError::Invalid(
            "a toolchain is a lowercase name of at most 32 characters",
        ))
    }
}

/// The owner's declared toolchains: each name valid, at most [`MAX_TOOLCHAINS`].
pub fn validate_toolchains(names: &[String]) -> Result<(), OrchestrationError> {
    if names.len() > MAX_TOOLCHAINS {
        return Err(OrchestrationError::Invalid(
            "at most 16 toolchains may be declared",
        ));
    }
    names.iter().try_for_each(|name| valid_toolchain(name))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuCapability {
    /// A physical GPU (or an integrated one) is present. A virtual display adapter is
    /// not counted.
    pub present: bool,
    /// The vendors present, sorted: `amd`, `apple`, `intel`, `nvidia`, `qualcomm`, or
    /// `other`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vendors: Vec<String>,
}

impl GpuCapability {
    /// A report from the vendors found; none found is no GPU.
    pub fn from_vendors(vendors: impl IntoIterator<Item = String>) -> Self {
        let mut vendors: Vec<String> = vendors.into_iter().collect();
        vendors.sort();
        vendors.dedup();
        Self {
            present: !vendors.is_empty(),
            vendors,
        }
    }
}

/// One installed AI tool and the version its program reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolVersion {
    pub tool: String,
    /// `None` when the version was not asked for or could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// One toolchain the machine's owner declared, and whether it is there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolchainCapability {
    pub name: String,
    pub installed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// What the machine is, as its own server tells it. Detected on demand and kept for a
/// while (§26.10); a member the host could not tell is `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// HS2-T2KS8G: read from a peer, so a newer peer's added fields are accepted.
pub struct HostCapabilities {
    pub os: OsFamily,
    /// The OS release: `15.1` on macOS, `10.0.26100` on Windows, `VERSION_ID` on Linux.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    /// Linux only: the distribution's `ID`, such as `ubuntu`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_distribution: Option<String>,
    /// The machine's native architecture, normalized (`x86_64`, `arm64`).
    pub arch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<GpuCapability>,
    /// Every installed drivable AI tool with its version, in registry order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_tools: Option<Vec<ToolVersion>>,
    /// The toolchains the machine owner declared (or the default list), in that order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolchains: Option<Vec<ToolchainCapability>>,
}

impl HostCapabilities {
    /// The bare report of a machine nothing more is known about.
    pub fn bare(os: OsFamily, arch: &str) -> Self {
        Self {
            os,
            os_version: None,
            os_distribution: None,
            arch: normalize_arch(arch).unwrap_or_else(|| "unknown".into()),
            cpu_model: None,
            gpu: None,
            ai_tools: None,
            toolchains: None,
        }
    }
}

/// HS2-ZY4Z4G: what a plan step needs its target's machine to be. Every list is
/// optional. `os` and `arch` are met by any one value; `tools` and `toolchains` by all.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepRequirements {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub os: Vec<OsFamily>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arch: Vec<String>,
    /// AI tools that must be installed and enabled for the checkout, by plugin id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// Toolchains the machine must report installed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub toolchains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_memory_gb: Option<u32>,
    /// `any` for any GPU, or one vendor of [`GPU_VENDORS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,
}

impl StepRequirements {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The canonical form: checked, normalized, sorted and without repeats, so two
    /// spellings of one requirement are one definition.
    pub fn normalized(mut self) -> Result<Self, OrchestrationError> {
        let too_many = [
            self.os.len(),
            self.arch.len(),
            self.tools.len(),
            self.toolchains.len(),
        ]
        .into_iter()
        .any(|count| count > MAX_REQUIREMENT_VALUES);
        if too_many {
            return Err(OrchestrationError::Invalid(
                "a requirement lists at most 8 values",
            ));
        }
        if self.os.contains(&OsFamily::Other) {
            return Err(OrchestrationError::Invalid(
                "an OS requirement is macos, linux or windows",
            ));
        }
        self.arch = self
            .arch
            .iter()
            .map(|arch| {
                normalize_arch(arch).ok_or(OrchestrationError::Invalid(
                    "an architecture is a lowercase name",
                ))
            })
            .collect::<Result<_, _>>()?;
        for tool in &self.tools {
            super::contract::valid_identifier(tool)?;
        }
        validate_toolchains(&self.toolchains)?;
        if self
            .min_memory_gb
            .is_some_and(|gb| gb == 0 || gb > MAX_MIN_MEMORY_GB)
        {
            return Err(OrchestrationError::Invalid("min_memory_gb is 1 to 65536"));
        }
        if let Some(gpu) = &mut self.gpu {
            *gpu = gpu.to_ascii_lowercase();
            if gpu != "any" && !GPU_VENDORS.contains(&gpu.as_str()) {
                return Err(OrchestrationError::Invalid(
                    "gpu is any, amd, apple, intel, nvidia, qualcomm or other",
                ));
            }
        }
        for list in [&mut self.arch, &mut self.tools, &mut self.toolchains] {
            list.sort();
            list.dedup();
        }
        self.os.sort();
        self.os.dedup();
        Ok(self)
    }

    /// Parse one `key=value` the CLI takes (`--require os=windows`) into these
    /// requirements. List keys add a value; the others replace theirs.
    pub fn add(&mut self, spec: &str) -> Result<(), OrchestrationError> {
        let invalid = OrchestrationError::Invalid(
            "a requirement is os=, arch=, tool=, toolchain=, min_memory_gb= or gpu=",
        );
        let Some((key, value)) = spec.split_once('=') else {
            return Err(invalid);
        };
        let value = value.trim();
        if value.is_empty() {
            return Err(invalid);
        }
        match key.trim() {
            "os" => self
                .os
                .push(OsFamily::parse(value).ok_or(OrchestrationError::Invalid(
                    "an OS requirement is macos, linux or windows",
                ))?),
            "arch" => self.arch.push(value.into()),
            "tool" | "tools" => self.tools.push(value.into()),
            "toolchain" | "toolchains" => self.toolchains.push(value.into()),
            "min_memory_gb" => {
                self.min_memory_gb =
                    Some(value.parse().map_err(|_| {
                        OrchestrationError::Invalid("min_memory_gb is a whole number")
                    })?);
            }
            "gpu" => self.gpu = Some(value.into()),
            _ => return Err(invalid),
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementState {
    /// The machine reported what it is, and it is not what the step needs.
    Unmet,
    /// The machine did not report it (an older peer, or a value it could not read).
    Unknown,
}

/// One requirement a machine misses, named as `<kind>:<value>`, for example
/// `os:windows`, `arch:arm64|x86_64`, `toolchain:cargo`, `min_memory_gb:32` or
/// `gpu:nvidia`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementMiss {
    pub requirement: String,
    pub state: RequirementState,
}

fn miss(misses: &mut Vec<RequirementMiss>, requirement: String, state: RequirementState) {
    misses.push(RequirementMiss { requirement, state });
}

/// HS2-ZY4Z4G: every requirement of `requirements` that the machine `snapshot`
/// describes does not meet (`unmet`) or did not report (`unknown`), in a fixed order:
/// OS, architecture, memory, GPU, AI tools, toolchains. Empty when the machine
/// qualifies. Pure.
pub fn requirement_misses(
    requirements: &StepRequirements,
    snapshot: &CapacitySnapshot,
) -> Vec<RequirementMiss> {
    use RequirementState::{Unknown, Unmet};
    let mut misses = Vec::new();
    let capabilities = snapshot.capabilities.as_ref();
    if !requirements.os.is_empty() {
        let name = format!(
            "os:{}",
            requirements
                .os
                .iter()
                .map(|os| os.code())
                .collect::<Vec<_>>()
                .join("|")
        );
        match capabilities {
            None => miss(&mut misses, name, Unknown),
            Some(caps) if !requirements.os.contains(&caps.os) => miss(&mut misses, name, Unmet),
            Some(_) => {}
        }
    }
    if !requirements.arch.is_empty() {
        let name = format!("arch:{}", requirements.arch.join("|"));
        match capabilities {
            None => miss(&mut misses, name, Unknown),
            Some(caps) => {
                let arch = normalize_arch(&caps.arch).unwrap_or_default();
                if !requirements.arch.contains(&arch) {
                    miss(&mut misses, name, Unmet);
                }
            }
        }
    }
    if let Some(gb) = requirements.min_memory_gb {
        let name = format!("min_memory_gb:{gb}");
        match snapshot.memory.total_mb {
            None => miss(&mut misses, name, Unknown),
            Some(total_mb) => {
                let needed = u64::from(gb) * 1024 * MEMORY_TOLERANCE_SIXTEENTHS;
                if total_mb.saturating_mul(16) < needed {
                    miss(&mut misses, name, Unmet);
                }
            }
        }
    }
    if let Some(gpu) = &requirements.gpu {
        let name = format!("gpu:{gpu}");
        match capabilities.and_then(|caps| caps.gpu.as_ref()) {
            None => miss(&mut misses, name, Unknown),
            Some(found) => {
                let met = if gpu == "any" {
                    found.present
                } else {
                    found.vendors.iter().any(|vendor| vendor == gpu)
                };
                if !met {
                    miss(&mut misses, name, Unmet);
                }
            }
        }
    }
    for tool in &requirements.tools {
        let name = format!("tool:{tool}");
        match &snapshot.tools {
            None => miss(&mut misses, name, Unknown),
            Some(tools) => {
                let usable = tools
                    .iter()
                    .any(|known| &known.tool == tool && known.installed && known.configured);
                if !usable {
                    miss(&mut misses, name, Unmet);
                }
            }
        }
    }
    for toolchain in &requirements.toolchains {
        let name = format!("toolchain:{toolchain}");
        let reported = capabilities
            .and_then(|caps| caps.toolchains.as_ref())
            .and_then(|all| all.iter().find(|known| &known.name == toolchain));
        match reported {
            // Not declared on that machine: its owner did not say whether it is there.
            None => miss(&mut misses, name, Unknown),
            Some(known) if !known.installed => miss(&mut misses, name, Unmet),
            Some(_) => {}
        }
    }
    misses
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;
