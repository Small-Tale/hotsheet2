//! Versioned project development policy and override precedence (HS2-BF79YX).

use super::development_strategy::*;
use super::strategy::{IntegrationStrategy, StrategyPolicy};
use hotsheet_ticketing::{Scope, Settings};

fn staged_allowed() -> StrategyPolicy {
    let mut policy = StrategyPolicy::default();
    policy
        .allow(IntegrationStrategy::CiStaged, "green exact-SHA promotion")
        .unwrap();
    policy
}

#[test]
fn hs2_bf79yx_unset_project_preserves_legacy_serial_and_migration_default() {
    let root = tempfile::tempdir().unwrap();
    let settings = Settings::for_project(root.path());
    let policy = DevelopmentPolicy::read(&settings).unwrap();
    assert_eq!(policy, DevelopmentPolicy::default());
    let selection = policy
        .resolve(&StrategyPolicy::default(), None, None)
        .unwrap();
    assert_eq!(selection.preset, DevelopmentPreset::TicketAssured);
    assert_eq!(selection.source, SelectionSource::UnconfiguredProject);
    assert_eq!(selection.integration, IntegrationStrategy::Serial);
    assert_eq!(selection.local_gate, LocalGate::RiskSelected);
    assert!(selection.require_green_main_sha);
    assert!(selection.attribute_red_ci_before_completion);
    assert_eq!(selection.green_main_goal_minutes, None);
    let explicit_ticket = policy
        .resolve(
            &StrategyPolicy::default(),
            None,
            Some(DevelopmentPreset::TicketAssured),
        )
        .unwrap();
    assert_eq!(explicit_ticket.source, SelectionSource::Ticket);
    assert_eq!(explicit_ticket.local_gate, LocalGate::FullLocal);
    assert!(
        settings
            .get(DEVELOPMENT_POLICY_SETTING, Scope::Shared)
            .unwrap()
            .is_none()
    );
}

#[test]
fn hs2_bf79yx_unset_project_reports_existing_batch_default() {
    let root = tempfile::tempdir().unwrap();
    let settings = Settings::for_project(root.path());
    let mut integration = StrategyPolicy::default();
    integration
        .allow(IntegrationStrategy::Batch, "reviewed existing batch policy")
        .unwrap();
    integration.default = IntegrationStrategy::Batch;
    integration.write(&settings).unwrap();

    let policy = DevelopmentPolicy::read(&settings).unwrap();
    let effective = policy.resolve(&integration, None, None).unwrap();
    assert_eq!(effective.source, SelectionSource::UnconfiguredProject);
    assert_eq!(effective.integration, IntegrationStrategy::Batch);
    assert_eq!(effective.local_gate, LocalGate::RiskSelected);
    assert_eq!(policy.report(&integration).unwrap().effective, effective);

    let explicit_ticket = policy
        .resolve(&integration, None, Some(DevelopmentPreset::TicketAssured))
        .unwrap();
    assert_eq!(explicit_ticket.integration, IntegrationStrategy::Serial);
    assert_eq!(explicit_ticket.local_gate, LocalGate::FullLocal);
}

#[test]
fn hs2_bf79yx_every_preset_round_trips_in_shared_settings() {
    let root = tempfile::tempdir().unwrap();
    let settings = Settings::for_project(root.path());
    let integration = staged_allowed();
    integration.write(&settings).unwrap();
    for preset in DevelopmentPreset::ALL {
        let policy = DevelopmentPolicy {
            version: DEVELOPMENT_POLICY_VERSION,
            default: preset,
            green_main_goal_minutes: 420,
            configured: true,
        };
        policy.write(&settings).unwrap();
        assert_eq!(DevelopmentPolicy::read(&settings).unwrap(), policy);
        assert!(
            serde_json::to_value(&policy)
                .unwrap()
                .get("configured")
                .is_none()
        );
        let effective = policy.resolve(&integration, None, None).unwrap();
        assert_eq!(effective.preset, preset);
        assert_eq!(effective.integration, preset.integration());
        assert_eq!(effective.local_gate, preset.local_gate());
        assert_eq!(effective.source, SelectionSource::Project);
        assert!(effective.require_green_main_sha);
        assert!(effective.attribute_red_ci_before_completion);
        assert_eq!(
            effective.green_main_goal_minutes,
            (preset == DevelopmentPreset::SteadyMain).then_some(420)
        );
    }
    let refused = StrategyPolicy::default()
        .write(&settings)
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("conflicts with project integration policy"),
        "{refused}"
    );
    settings
        .set(
            DEVELOPMENT_POLICY_SETTING,
            serde_json::json!({"version": 1, "default": "ticket-assured"}),
            Scope::Local,
        )
        .unwrap();
    assert_eq!(
        DevelopmentPolicy::read(&settings).unwrap().default,
        DevelopmentPreset::RiskGuided
    );
}

#[test]
fn hs2_bf79yx_ticket_then_session_then_project_precedence_keeps_gates() {
    let policy = DevelopmentPolicy {
        default: DevelopmentPreset::SteadyMain,
        configured: true,
        ..DevelopmentPolicy::default()
    };
    let allowed = staged_allowed();
    let session = policy
        .resolve(&allowed, Some(DevelopmentPreset::RiskGuided), None)
        .unwrap();
    assert_eq!(session.source, SelectionSource::Session);
    assert_eq!(session.local_gate, LocalGate::RiskSelected);
    let ticket = policy
        .resolve(
            &allowed,
            Some(DevelopmentPreset::RiskGuided),
            Some(DevelopmentPreset::TicketAssured),
        )
        .unwrap();
    assert_eq!(ticket.source, SelectionSource::Ticket);
    assert_eq!(ticket.integration, IntegrationStrategy::Serial);
    assert!(ticket.require_green_main_sha);
    assert!(ticket.attribute_red_ci_before_completion);
    let project = policy.resolve(&allowed, None, None).unwrap();
    assert_eq!(project.source, SelectionSource::Project);
    assert_eq!(project.green_main_goal_minutes, Some(360));
}

#[test]
fn hs2_bf79yx_rejects_invalid_policy_and_unauthorized_staging() {
    let root = tempfile::tempdir().unwrap();
    let settings = Settings::for_project(root.path());
    let mut policy = DevelopmentPolicy {
        default: DevelopmentPreset::SteadyMain,
        ..DevelopmentPolicy::default()
    };
    assert!(
        policy
            .write(&settings)
            .unwrap_err()
            .to_string()
            .contains("conflicts with project integration policy")
    );
    assert!(
        policy
            .resolve(
                &StrategyPolicy::default(),
                None,
                Some(DevelopmentPreset::RiskGuided)
            )
            .is_err()
    );
    assert!(
        settings
            .get(DEVELOPMENT_POLICY_SETTING, Scope::Shared)
            .unwrap()
            .is_none()
    );
    policy.version = 2;
    assert!(policy.validate().is_err());
    policy.version = DEVELOPMENT_POLICY_VERSION;
    policy.green_main_goal_minutes = 0;
    assert!(policy.validate().is_err());
    policy.green_main_goal_minutes = 10_081;
    assert!(policy.validate().is_err());
    settings
        .set(
            DEVELOPMENT_POLICY_SETTING,
            serde_json::json!({"version": 1, "default": "risk-guided", "unknown": true}),
            Scope::Shared,
        )
        .unwrap();
    assert!(DevelopmentPolicy::read(&settings).is_err());
}
