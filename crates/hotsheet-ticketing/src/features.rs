//! Feature areas (HS2-3GJZGG, `docs/06-clients.md` §6.12): the non-core parts of Hot Sheet a
//! person can turn on or off in App Settings › Features.
//!
//! Each choice is a machine-wide setting (`${HOTSHEET_HOME}/settings.json`, key
//! `features.<id>`), read and written with `GET`/`PUT /feature-settings` or
//! `hotsheet-cli settings set features.<id> on|off|default`, and applied to every client of the
//! host. An unset value takes the feature's default, so a new default reaches every install on
//! upgrade without a migration. A feature that is off hides its client entry points; its data is
//! kept and the CLI and MCP are unaffected. Automatic ticket grouping is the one feature the server
//! itself consults: off, it spends no AI turn.
//!
//! This list is the server half of the registry. The web client's registry
//! (`clients/web/src/feature-areas.ts`) carries labels, hints and entry points, and a policy test
//! pins that both list the same ids, groups and defaults.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The settings-key prefix every feature choice is stored under.
pub const SETTING_PREFIX: &str = "features.";

/// Where a feature is listed on the Features page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureGroup {
    /// Off by default: early areas that are still changing.
    Experimental,
    /// On by default; listed under Workspace.
    Workspace,
    /// On by default; listed under Tickets.
    Tickets,
    /// On by default; listed under Ambient.
    Ambient,
}

/// One registered feature area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FeatureDefinition {
    pub id: &'static str,
    pub group: FeatureGroup,
    pub default_on: bool,
}

const fn feature(id: &'static str, group: FeatureGroup, default_on: bool) -> FeatureDefinition {
    FeatureDefinition {
        id,
        group,
        default_on,
    }
}

/// Automatic ticket grouping: the server spends an AI turn only while this is on.
pub const AUTO_GROUPING: &str = "auto-grouping";

/// Every feature area, in the order the Features page lists them.
pub const FEATURES: &[FeatureDefinition] = &[
    feature("ai-chat", FeatureGroup::Experimental, false),
    feature("drive", FeatureGroup::Experimental, false),
    feature("project-stats", FeatureGroup::Experimental, false),
    feature("cross-project-stats", FeatureGroup::Experimental, false),
    feature(AUTO_GROUPING, FeatureGroup::Experimental, false),
    feature("note-repair", FeatureGroup::Experimental, false),
    feature("ai-recommendations", FeatureGroup::Experimental, false),
    feature("code-review", FeatureGroup::Experimental, false),
    feature("workspace-grid", FeatureGroup::Workspace, true),
    feature("workers-view", FeatureGroup::Workspace, true),
    feature("commands", FeatureGroup::Workspace, true),
    feature("repository-status", FeatureGroup::Workspace, true),
    feature("live-activity", FeatureGroup::Workspace, true),
    feature("ticket-groups", FeatureGroup::Tickets, true),
    feature("ticket-timeline", FeatureGroup::Tickets, true),
    feature("confidence", FeatureGroup::Tickets, true),
    feature("annotations", FeatureGroup::Tickets, true),
    feature("not-working", FeatureGroup::Tickets, true),
    feature("announcer", FeatureGroup::Ambient, true),
    feature("standby", FeatureGroup::Ambient, true),
];

/// The registered definition of `id`.
pub fn definition(id: &str) -> Option<&'static FeatureDefinition> {
    FEATURES.iter().find(|feature| feature.id == id)
}

/// The settings key a feature's choice is stored under.
pub fn setting_key(id: &str) -> String {
    format!("{SETTING_PREFIX}{id}")
}

/// Whether `key` is a feature setting (so it is machine-wide and validated).
pub fn is_feature_key(key: &str) -> bool {
    key.starts_with(SETTING_PREFIX)
}

/// Every registered id, `|`-separated, for error messages.
fn known_ids() -> String {
    FEATURES
        .iter()
        .map(|feature| feature.id)
        .collect::<Vec<_>>()
        .join("|")
}

/// Why a feature choice was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FeatureSettingError {
    #[error("unknown feature '{0}' ({ids})", ids = known_ids())]
    Unknown(String),
    #[error("{0} must be on, off, or default")]
    Value(String),
}

/// A command-line choice: `Some(bool)` to store, `None` to clear back to the default.
pub fn parse_setting(key: &str, raw: &str) -> Result<Option<bool>, FeatureSettingError> {
    let id = key.strip_prefix(SETTING_PREFIX).unwrap_or(key);
    if definition(id).is_none() {
        return Err(FeatureSettingError::Unknown(id.to_owned()));
    }
    match raw.trim() {
        "on" | "true" => Ok(Some(true)),
        "off" | "false" => Ok(Some(false)),
        "default" => Ok(None),
        _ => Err(FeatureSettingError::Value(key.to_owned())),
    }
}

/// Explicit choices by feature id; `None` (or a missing id) means "use the default".
pub type ConfiguredFeatures = BTreeMap<String, Option<bool>>;

/// Reject a REST body that names an unregistered feature.
pub fn validate_configured(configured: &ConfiguredFeatures) -> Result<(), FeatureSettingError> {
    match configured.keys().find(|id| definition(id).is_none()) {
        Some(id) => Err(FeatureSettingError::Unknown(id.clone())),
        None => Ok(()),
    }
}

/// The feature settings view: defaults, explicit values (`null` = default), and effective.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSettingsView {
    pub defaults: BTreeMap<String, bool>,
    pub configured: BTreeMap<String, Option<bool>>,
    pub effective: BTreeMap<String, bool>,
}

/// Build the view from each registered feature's stored value. A stored value that is not a
/// boolean (a hand-edited settings file) is ignored so the default applies.
pub fn view(mut stored: impl FnMut(&str) -> Option<serde_json::Value>) -> FeatureSettingsView {
    let mut view = FeatureSettingsView {
        defaults: BTreeMap::new(),
        configured: BTreeMap::new(),
        effective: BTreeMap::new(),
    };
    for feature in FEATURES {
        let configured = stored(&setting_key(feature.id)).and_then(|value| value.as_bool());
        view.defaults.insert(feature.id.into(), feature.default_on);
        view.configured.insert(feature.id.into(), configured);
        view.effective
            .insert(feature.id.into(), configured.unwrap_or(feature.default_on));
    }
    view
}

/// Whether `id` is on, given its stored value. An unregistered id is on.
pub fn is_on(id: &str, stored: Option<&serde_json::Value>) -> bool {
    stored
        .and_then(serde_json::Value::as_bool)
        .unwrap_or_else(|| definition(id).is_none_or(|feature| feature.default_on))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn registry_ids_are_unique_and_experimental_areas_default_off() {
        let mut ids: Vec<_> = FEATURES.iter().map(|feature| feature.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), FEATURES.len());
        for feature in FEATURES {
            assert_eq!(
                feature.default_on,
                feature.group != FeatureGroup::Experimental,
                "{}",
                feature.id
            );
        }
        // The four areas the ticket named are experimental.
        for id in ["ai-chat", "drive", "project-stats", "cross-project-stats"] {
            assert_eq!(definition(id).unwrap().group, FeatureGroup::Experimental);
        }
    }

    #[test]
    fn cli_values_parse_and_unknown_features_are_rejected() {
        assert_eq!(parse_setting("features.drive", "on"), Ok(Some(true)));
        assert_eq!(parse_setting("features.drive", " off "), Ok(Some(false)));
        assert_eq!(parse_setting("features.drive", "true"), Ok(Some(true)));
        assert_eq!(parse_setting("features.drive", "default"), Ok(None));
        assert_eq!(
            parse_setting("features.drive", "maybe"),
            Err(FeatureSettingError::Value("features.drive".into()))
        );
        assert_eq!(
            parse_setting("features.nope", "on"),
            Err(FeatureSettingError::Unknown("nope".into()))
        );
        assert!(is_feature_key("features.drive"));
        assert!(!is_feature_key("standby.enabled"));
        assert_eq!(setting_key("drive"), "features.drive");
    }

    #[test]
    fn view_applies_defaults_and_ignores_broken_values() {
        let view = view(|key| match key {
            "features.drive" => Some(json!(true)),
            "features.standby" => Some(json!(false)),
            "features.ai-chat" => Some(json!("yes")),
            _ => None,
        });
        assert_eq!(view.effective["drive"], true);
        assert_eq!(view.configured["drive"], Some(true));
        assert_eq!(view.effective["standby"], false);
        assert_eq!(view.effective["ai-chat"], false);
        assert_eq!(view.configured["ai-chat"], None);
        assert_eq!(view.effective["announcer"], true);
        assert_eq!(view.defaults.len(), FEATURES.len());
        assert!(is_on(AUTO_GROUPING, Some(&json!(true))));
        assert!(!is_on(AUTO_GROUPING, None));
        assert!(is_on("unregistered", None));
    }

    #[test]
    fn rest_bodies_reject_unknown_features() {
        let mut configured = ConfiguredFeatures::new();
        configured.insert("drive".into(), Some(true));
        configured.insert("standby".into(), None);
        assert!(validate_configured(&configured).is_ok());
        configured.insert("teleport".into(), Some(true));
        assert_eq!(
            validate_configured(&configured),
            Err(FeatureSettingError::Unknown("teleport".into()))
        );
    }
}
