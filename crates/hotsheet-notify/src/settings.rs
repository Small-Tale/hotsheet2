//! Channel settings: schema validation against a notifier's [`SettingField`]s and
//! delivery-time secret resolution. Stored settings never hold a credential value, only
//! `{"secret": "<key name>"}` references into the machine key registry.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::capability::{SettingField, SettingKind};

/// Stored, unresolved channel settings.
pub type Settings = BTreeMap<String, Value>;

/// Looks a secret up by key name. Implemented for the key registry; tests use a map.
pub trait SecretLookup {
    fn secret(&self, name: &str) -> Result<String, String>;
}

impl<S: hotsheet_ticketing::SecretStore> SecretLookup for hotsheet_ticketing::KeyRegistry<S> {
    fn secret(&self, name: &str) -> Result<String, String> {
        self.get(name).map_err(|error| error.to_string())
    }
}

impl SecretLookup for BTreeMap<String, String> {
    fn secret(&self, name: &str) -> Result<String, String> {
        self.get(name)
            .cloned()
            .ok_or_else(|| format!("no credential named '{name}'"))
    }
}

/// The secret key name a field references, when it is a well-formed reference.
pub fn secret_reference(value: &Value) -> Option<&str> {
    value.get("secret").and_then(Value::as_str)
}

/// Check stored settings against a schema. Secrets must be references, never values.
pub fn validate(fields: &[SettingField], settings: &Settings) -> Result<(), String> {
    for key in settings.keys() {
        if !fields.iter().any(|field| &field.key == key) {
            return Err(format!("unknown setting '{key}'"));
        }
    }
    for field in fields {
        let Some(value) = settings.get(&field.key) else {
            if field.required && field.default.is_none() {
                return Err(format!("setting '{}' is required", field.key));
            }
            continue;
        };
        let ok = match &field.kind {
            SettingKind::String => value
                .as_str()
                .is_some_and(|s| !field.required || !s.trim().is_empty()),
            SettingKind::List => value.as_array().is_some_and(|items| {
                items.iter().all(Value::is_string) && (!field.required || !items.is_empty())
            }),
            SettingKind::Number => value.is_number(),
            SettingKind::Bool => value.is_boolean(),
            SettingKind::Choice { choices } => value
                .as_str()
                .is_some_and(|v| choices.iter().any(|c| c == v)),
            SettingKind::Secret => secret_reference(value).is_some_and(|name| !name.is_empty()),
        };
        if !ok {
            let expected = match &field.kind {
                SettingKind::String => "a string".to_string(),
                SettingKind::List => "a list of strings".to_string(),
                SettingKind::Number => "a number".to_string(),
                SettingKind::Bool => "true or false".to_string(),
                SettingKind::Choice { choices } => format!("one of {}", choices.join(", ")),
                SettingKind::Secret => {
                    "a credential reference {\"secret\": \"<key name>\"}".to_string()
                }
            };
            return Err(format!("setting '{}' must be {expected}", field.key));
        }
    }
    Ok(())
}

/// Settings with defaults applied and secrets resolved, for one delivery only.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Resolved(pub BTreeMap<String, Value>);

impl Resolved {
    pub fn str(&self, key: &str) -> Option<&str> {
        self.0
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    }

    pub fn list(&self, key: &str) -> Vec<String> {
        self.0
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn number(&self, key: &str) -> Option<f64> {
        self.0.get(key).and_then(Value::as_f64)
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        self.0.get(key).and_then(Value::as_bool)
    }
}

pub fn resolve(
    fields: &[SettingField],
    settings: &Settings,
    secrets: &dyn SecretLookup,
) -> Result<Resolved, String> {
    validate(fields, settings)?;
    let mut out = BTreeMap::new();
    for field in fields {
        let value = match settings.get(&field.key) {
            Some(value) => value.clone(),
            None => match &field.default {
                Some(value) => value.clone(),
                None => continue,
            },
        };
        let value = match field.kind {
            SettingKind::Secret => {
                let name = secret_reference(&value).unwrap_or_default();
                Value::String(
                    secrets
                        .secret(name)
                        .map_err(|e| format!("credential for '{}': {e}", field.key))?,
                )
            }
            _ => value,
        };
        out.insert(field.key.clone(), value);
    }
    Ok(Resolved(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fields() -> Vec<SettingField> {
        vec![
            SettingField::new("topic", "Topic", SettingKind::String).required(),
            SettingField::new("to", "To", SettingKind::List),
            SettingField::new(
                "mode",
                "Mode",
                SettingKind::Choice {
                    choices: vec!["a".into(), "b".into()],
                },
            )
            .default_value(json!("a")),
            SettingField::new("token", "Token", SettingKind::Secret),
            SettingField::new("port", "Port", SettingKind::Number),
            SettingField::new("on", "On", SettingKind::Bool),
        ]
    }

    fn settings(value: Value) -> Settings {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn validates_types_required_and_unknown_keys() {
        let f = fields();
        assert!(validate(&f, &settings(json!({"topic":"x"}))).is_ok());
        assert_eq!(
            validate(&f, &settings(json!({}))).unwrap_err(),
            "setting 'topic' is required"
        );
        assert!(validate(&f, &settings(json!({"topic":""}))).is_err());
        assert!(
            validate(&f, &settings(json!({"topic":"x","extra":1})))
                .unwrap_err()
                .contains("unknown setting 'extra'")
        );
        assert!(validate(&f, &settings(json!({"topic":"x","to":[1]}))).is_err());
        assert!(validate(&f, &settings(json!({"topic":"x","mode":"c"}))).is_err());
        assert!(validate(&f, &settings(json!({"topic":"x","port":"25"}))).is_err());
        assert!(validate(&f, &settings(json!({"topic":"x","on":"yes"}))).is_err());
    }

    #[test]
    fn secrets_must_be_references_not_values() {
        let f = fields();
        let err = validate(&f, &settings(json!({"topic":"x","token":"tk_live"}))).unwrap_err();
        assert!(err.contains("credential reference"), "{err}");
        assert!(validate(&f, &settings(json!({"topic":"x","token":{"secret":"k"}}))).is_ok());
    }

    #[test]
    fn resolve_applies_defaults_and_secrets() {
        let mut secrets = BTreeMap::new();
        secrets.insert("k".to_string(), "s3cret".to_string());
        let r = resolve(
            &fields(),
            &settings(
                json!({"topic":"x","token":{"secret":"k"},"to":["a@x.test"],"port":25,"on":true}),
            ),
            &secrets,
        )
        .unwrap();
        assert_eq!(r.str("token"), Some("s3cret"));
        assert_eq!(r.str("mode"), Some("a"));
        assert_eq!(r.list("to"), ["a@x.test"]);
        assert_eq!(r.number("port"), Some(25.0));
        assert_eq!(r.bool("on"), Some(true));
        let err = resolve(
            &fields(),
            &settings(json!({"topic":"x","token":{"secret":"missing"}})),
            &secrets,
        )
        .unwrap_err();
        assert!(err.contains("credential for 'token'"), "{err}");
    }
}
