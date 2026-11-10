//! Project development and testing policy (HS2-BF79YX).
//!
//! This is deliberately separate from the integration permission list. A preset
//! chooses an allowed integration strategy but cannot grant that permission.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, ensure};
use hotsheet_ticketing::{Scope, Settings};
use serde::{Deserialize, Serialize};

use super::strategy::{IntegrationStrategy, StrategyPolicy, validate_staging_branch};

pub const DEVELOPMENT_POLICY_SETTING: &str = "development_strategy";
pub const DEVELOPMENT_POLICY_VERSION: u32 = 2;
pub const DEFAULT_GREEN_MAIN_GOAL_MINUTES: u32 = 360;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum DevelopmentPreset {
    #[default]
    TicketAssured,
    SteadyMain,
    RiskGuided,
}

impl DevelopmentPreset {
    pub const ALL: [Self; 3] = [Self::TicketAssured, Self::SteadyMain, Self::RiskGuided];

    pub const fn integration(self) -> IntegrationStrategy {
        match self {
            Self::TicketAssured => IntegrationStrategy::Serial,
            Self::SteadyMain | Self::RiskGuided => IntegrationStrategy::CiStaged,
        }
    }

    pub const fn local_gate(self) -> LocalGate {
        match self {
            Self::TicketAssured => LocalGate::FullLocal,
            Self::SteadyMain => LocalGate::Targeted,
            Self::RiskGuided => LocalGate::RiskSelected,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::TicketAssured => "ticket-assured",
            Self::SteadyMain => "steady-main",
            Self::RiskGuided => "risk-guided",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalGate {
    FullLocal,
    Targeted,
    RiskSelected,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskTier {
    T0,
    #[default]
    T1,
    T2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TestPlanTiming {
    BeforeWork,
    BeforeHandoff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RedCiRecovery {
    AttributeAndRetry,
    FallbackToSerial,
}

/// A project-owned profile may select only gates the checked-in gate already knows.
/// No field accepts an executable command or waives an affected mandatory stage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomDevelopmentStrategy {
    pub integration: IntegrationStrategy,
    pub local_gate: LocalGate,
    pub minimum_tier: RiskTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_minutes: Option<u32>,
    #[serde(default)]
    pub browser: bool,
    #[serde(default)]
    pub platform: bool,
    #[serde(default)]
    pub perf: bool,
    pub test_plan_timing: TestPlanTiming,
    /// Name of the connected CI check that will verify the exact main SHA.
    pub ci_gate: String,
    /// Required for ci-staged, matching the allowed integration strategy's branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staging_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub green_main_goal_minutes: Option<u32>,
    pub red_ci_recovery: RedCiRecovery,
}

impl CustomDevelopmentStrategy {
    fn validate(&self, integration_policy: &StrategyPolicy) -> Result<()> {
        ensure!(
            self.ci_gate == "CI",
            "custom development strategy requires the connected CI gate; this checkout supports only CI"
        );
        ensure!(
            self.budget_minutes
                .is_none_or(|minutes| (1..=1_440).contains(&minutes)),
            "custom local budget must be 1-1440 minutes"
        );
        ensure!(
            self.minimum_tier != RiskTier::T2 || self.budget_minutes.is_none(),
            "a T2 minimum cannot have a local time budget"
        );
        ensure!(
            self.local_gate != LocalGate::FullLocal || self.budget_minutes.is_none(),
            "a full local gate cannot have a time budget"
        );
        ensure!(
            self.green_main_goal_minutes
                .is_none_or(|minutes| (1..=10_080).contains(&minutes)),
            "custom green-main goal must be 1-10080 minutes"
        );
        if self.integration == IntegrationStrategy::CiStaged {
            let branch = self.staging_branch.as_deref().ok_or_else(|| {
                anyhow::anyhow!("ci-staged custom strategy requires a staging branch")
            })?;
            validate_staging_branch(branch)?;
            ensure!(
                branch == integration_policy.staging_branch(),
                "custom staging branch must match the allowed integration strategy branch"
            );
        } else {
            ensure!(
                self.staging_branch.is_none(),
                "only ci-staged custom strategies have a staging branch"
            );
        }
        ensure!(
            matches!(
                (self.integration, self.red_ci_recovery),
                (
                    IntegrationStrategy::Serial,
                    RedCiRecovery::AttributeAndRetry
                ) | (
                    IntegrationStrategy::Batch | IntegrationStrategy::CiStaged,
                    RedCiRecovery::FallbackToSerial
                )
            ),
            "custom red-CI recovery must match the integration strategy: serial attributes and retries; batch and ci-staged fall back to serial through their circuit breaker"
        );
        integration_policy.resolve(Some(self.integration))?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentPolicy {
    pub version: u32,
    #[serde(default)]
    pub default: DevelopmentPreset,
    #[serde(default = "default_green_main_goal")]
    pub green_main_goal_minutes: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_custom: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, CustomDevelopmentStrategy>,
    /// This is runtime provenance, never a committed policy field. An absent
    /// shared setting preserves the pre-BF risk-tier gate (HS2-BF79YX).
    #[serde(skip)]
    pub configured: bool,
}

const fn default_green_main_goal() -> u32 {
    DEFAULT_GREEN_MAIN_GOAL_MINUTES
}

impl Default for DevelopmentPolicy {
    fn default() -> Self {
        Self {
            version: 1,
            default: DevelopmentPreset::TicketAssured,
            green_main_goal_minutes: DEFAULT_GREEN_MAIN_GOAL_MINUTES,
            default_custom: None,
            custom: BTreeMap::new(),
            configured: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectionSource {
    UnconfiguredProject,
    Project,
    Session,
    Ticket,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DevelopmentSelection {
    pub preset: String,
    pub source: SelectionSource,
    pub integration: IntegrationStrategy,
    pub local_gate: LocalGate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom: Option<CustomDevelopmentStrategy>,
    /// Every preset retains the verified exact-SHA main promotion boundary.
    pub require_green_main_sha: bool,
    /// CI failure attribution is mandatory before completion for staged presets too.
    pub attribute_red_ci_before_completion: bool,
    pub green_main_goal_minutes: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DevelopmentReport {
    pub policy: DevelopmentPolicy,
    pub effective: DevelopmentSelection,
    pub presets: [DevelopmentPreset; 3],
}

impl DevelopmentPolicy {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 || self.version == DEVELOPMENT_POLICY_VERSION,
            "development strategy version {} is unsupported; expected 1 or {}",
            self.version,
            DEVELOPMENT_POLICY_VERSION
        );
        ensure!(
            (1..=10_080).contains(&self.green_main_goal_minutes),
            "green-main goal must be 1-10080 minutes"
        );
        ensure!(
            self.version != 1 || (self.default_custom.is_none() && self.custom.is_empty()),
            "version 1 development policy cannot contain custom strategies; use version 2"
        );
        ensure!(
            self.custom.len() <= 20,
            "at most 20 custom strategies are supported"
        );
        for name in self.custom.keys() {
            ensure!(
                (1..=40).contains(&name.len())
                    && name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                    && name
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                    && !name.ends_with('-')
                    && !name.contains("--")
                    && !DevelopmentPreset::ALL
                        .iter()
                        .any(|preset| preset.name() == name),
                "custom strategy name {name:?} must be unique lower-case kebab-case, 1-40 characters"
            );
        }
        if let Some(name) = &self.default_custom {
            ensure!(
                self.custom.contains_key(name),
                "unknown default custom strategy {name}"
            );
        }
        Ok(())
    }

    /// A ticket instruction takes precedence over a session instruction and the
    /// committed project default. Neither override can waive a mandatory gate or
    /// select an integration strategy the project has not explicitly authorized.
    pub fn resolve(
        &self,
        integration_policy: &StrategyPolicy,
        session: Option<DevelopmentPreset>,
        ticket: Option<DevelopmentPreset>,
    ) -> Result<DevelopmentSelection> {
        self.resolve_named(
            integration_policy,
            session.map(DevelopmentPreset::name),
            ticket.map(DevelopmentPreset::name),
        )
    }

    pub fn resolve_named(
        &self,
        integration_policy: &StrategyPolicy,
        session: Option<&str>,
        ticket: Option<&str>,
    ) -> Result<DevelopmentSelection> {
        self.validate()?;
        let (name, source) = match (ticket, session) {
            (Some(preset), _) => (preset, SelectionSource::Ticket),
            (None, Some(preset)) => (preset, SelectionSource::Session),
            (None, None) if !self.configured => {
                (self.default.name(), SelectionSource::UnconfiguredProject)
            }
            (None, None) => (
                self.default_custom
                    .as_deref()
                    .unwrap_or(self.default.name()),
                SelectionSource::Project,
            ),
        };
        let builtin = DevelopmentPreset::ALL
            .iter()
            .copied()
            .find(|preset| preset.name() == name);
        let custom = if builtin.is_none() {
            Some(
                self.custom
                    .get(name)
                    .ok_or_else(|| anyhow::anyhow!("unknown development strategy {name}"))?,
            )
        } else {
            None
        };
        if let Some(custom) = custom {
            custom.validate(integration_policy)?;
        }
        let integration = if source == SelectionSource::UnconfiguredProject {
            integration_policy.resolve(None)?
        } else if let Some(custom) = custom {
            custom.integration
        } else {
            integration_policy
                .resolve(Some(builtin.expect("built-in selection").integration()))
                .map_err(|error| {
                    anyhow::anyhow!(
                        "development preset {} conflicts with project integration policy: {error}",
                        name
                    )
                })?
        };
        Ok(DevelopmentSelection {
            preset: name.to_owned(),
            source,
            integration,
            local_gate: if source == SelectionSource::UnconfiguredProject {
                LocalGate::RiskSelected
            } else if let Some(custom) = custom {
                custom.local_gate
            } else {
                builtin.expect("built-in selection").local_gate()
            },
            custom: custom.cloned(),
            require_green_main_sha: true,
            attribute_red_ci_before_completion: true,
            green_main_goal_minutes: custom
                .and_then(|custom| custom.green_main_goal_minutes)
                .or_else(|| {
                    (builtin == Some(DevelopmentPreset::SteadyMain))
                        .then_some(self.green_main_goal_minutes)
                }),
        })
    }

    pub fn report(&self, integration_policy: &StrategyPolicy) -> Result<DevelopmentReport> {
        Ok(DevelopmentReport {
            policy: self.clone(),
            effective: self.resolve(integration_policy, None, None)?,
            presets: DevelopmentPreset::ALL,
        })
    }

    pub fn validate_against(&self, integration_policy: &StrategyPolicy) -> Result<()> {
        self.resolve(integration_policy, None, None)?;
        for custom in self.custom.values() {
            custom.validate(integration_policy)?;
        }
        Ok(())
    }

    /// The checked-in CI adapter supports only the GitHub `CI` workflow on a
    /// project with an origin. The staging branch is scratch and need not exist.
    pub fn validate_project_capabilities(&self, project: &Path) -> Result<()> {
        if self.custom.is_empty() {
            return Ok(());
        }
        let workflow = project.join(".github/workflows/ci.yml");
        let source = std::fs::read_to_string(&workflow).map_err(|_| {
            anyhow::anyhow!("custom development strategy requires a configured .github/workflows/ci.yml CI workflow")
        })?;
        ensure!(
            source.lines().any(|line| line.trim() == "name: CI"),
            "custom development strategy requires a CI workflow named CI"
        );
        let origin = hotsheet_ticketing::git::launch("git")
            .current_dir(project)
            .args(["remote", "get-url", "origin"])
            .output()?;
        ensure!(
            origin.status.success() && !origin.stdout.is_empty(),
            "custom development strategy requires a configured origin remote for CI"
        );
        Ok(())
    }

    /// An unset project retains its existing integration policy and risk-tier gate. A value is
    /// shared with all linked clients; machine-local settings never select policy.
    pub fn read(settings: &Settings) -> Result<Self> {
        let Some(value) = settings.get(DEVELOPMENT_POLICY_SETTING, Scope::Shared)? else {
            return Ok(Self::default());
        };
        let mut policy: Self = serde_json::from_value(value).map_err(|error| {
            anyhow::anyhow!("invalid {DEVELOPMENT_POLICY_SETTING} setting: {error}")
        })?;
        policy.configured = true;
        policy.validate_against(&StrategyPolicy::read(settings)?)?;
        Ok(policy)
    }

    pub fn write(&self, settings: &Settings) -> Result<()> {
        let integration_policy = StrategyPolicy::read(settings)?;
        let mut configured = self.clone();
        configured.configured = true;
        configured.validate_against(&integration_policy)?;
        settings.set(
            DEVELOPMENT_POLICY_SETTING,
            serde_json::to_value(self)?,
            Scope::Shared,
        )?;
        Ok(())
    }
}
