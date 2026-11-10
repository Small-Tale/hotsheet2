//! Project development and testing policy (HS2-BF79YX).
//!
//! This is deliberately separate from the integration permission list. A preset
//! chooses an allowed integration strategy but cannot grant that permission.

use anyhow::{Result, ensure};
use hotsheet_ticketing::{Scope, Settings};
use serde::{Deserialize, Serialize};

use super::strategy::{IntegrationStrategy, StrategyPolicy};

pub const DEVELOPMENT_POLICY_SETTING: &str = "development_strategy";
pub const DEVELOPMENT_POLICY_VERSION: u32 = 1;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalGate {
    FullLocal,
    Targeted,
    RiskSelected,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentPolicy {
    pub version: u32,
    #[serde(default)]
    pub default: DevelopmentPreset,
    #[serde(default = "default_green_main_goal")]
    pub green_main_goal_minutes: u32,
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
            version: DEVELOPMENT_POLICY_VERSION,
            default: DevelopmentPreset::TicketAssured,
            green_main_goal_minutes: DEFAULT_GREEN_MAIN_GOAL_MINUTES,
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
    pub preset: DevelopmentPreset,
    pub source: SelectionSource,
    pub integration: IntegrationStrategy,
    pub local_gate: LocalGate,
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
            self.version == DEVELOPMENT_POLICY_VERSION,
            "development strategy version {} is unsupported; expected {}",
            self.version,
            DEVELOPMENT_POLICY_VERSION
        );
        ensure!(
            (1..=10_080).contains(&self.green_main_goal_minutes),
            "green-main goal must be 1-10080 minutes"
        );
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
        self.validate()?;
        let (preset, source) = match (ticket, session) {
            (Some(preset), _) => (preset, SelectionSource::Ticket),
            (None, Some(preset)) => (preset, SelectionSource::Session),
            (None, None) if !self.configured => {
                (self.default, SelectionSource::UnconfiguredProject)
            }
            (None, None) => (self.default, SelectionSource::Project),
        };
        let integration = preset.integration();
        integration_policy
            .resolve(Some(integration))
            .map_err(|error| {
                anyhow::anyhow!(
                    "development preset {} conflicts with project integration policy: {error}",
                    preset.name()
                )
            })?;
        Ok(DevelopmentSelection {
            preset,
            source,
            integration,
            local_gate: if source == SelectionSource::UnconfiguredProject {
                LocalGate::RiskSelected
            } else {
                preset.local_gate()
            },
            require_green_main_sha: true,
            attribute_red_ci_before_completion: true,
            green_main_goal_minutes: (preset == DevelopmentPreset::SteadyMain)
                .then_some(self.green_main_goal_minutes),
        })
    }

    pub fn report(&self, integration_policy: &StrategyPolicy) -> Result<DevelopmentReport> {
        Ok(DevelopmentReport {
            policy: self.clone(),
            effective: self.resolve(integration_policy, None, None)?,
            presets: DevelopmentPreset::ALL,
        })
    }

    /// An unset project retains the legacy serial, per-ticket behavior. A value is
    /// shared with all linked clients; machine-local settings never select policy.
    pub fn read(settings: &Settings) -> Result<Self> {
        let Some(value) = settings.get(DEVELOPMENT_POLICY_SETTING, Scope::Shared)? else {
            return Ok(Self::default());
        };
        let mut policy: Self = serde_json::from_value(value).map_err(|error| {
            anyhow::anyhow!("invalid {DEVELOPMENT_POLICY_SETTING} setting: {error}")
        })?;
        policy.configured = true;
        policy.validate()?;
        Ok(policy)
    }

    pub fn write(&self, settings: &Settings) -> Result<()> {
        let integration_policy = StrategyPolicy::read(settings)?;
        let mut configured = self.clone();
        configured.configured = true;
        configured.resolve(&integration_policy, None, None)?;
        settings.set(
            DEVELOPMENT_POLICY_SETTING,
            serde_json::to_value(self)?,
            Scope::Shared,
        )?;
        Ok(())
    }
}
