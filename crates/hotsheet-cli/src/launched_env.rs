//! HS2-KDVHN5: the variables a Hot Sheet-launched terminal exports to name its server, its
//! terminal and its actor. Test runs force them to the empty string (`.cargo/config.toml`
//! cannot unset a variable), so every reader treats an empty or blank value as unset.

/// The launched-session variables a test run clears.
pub const LAUNCHED_SESSION_ENV: &[&str] = &[
    "HOTSHEET_SERVER",
    "HOTSHEET_SECRET",
    "HOTSHEET_TERMINAL_ID",
    "HOTSHEET_PROJECT",
    "HOTSHEET_PERMISSION_PROJECT",
    "HOTSHEET_WORKER_ID",
    "HOTSHEET_ACTOR_ROLE",
    "HOTSHEET_ACTOR_ID",
    "HOTSHEET_AGENT",
];

/// `name`'s value in this process, or `None` when it is unset, empty or blank.
#[must_use]
pub fn value(name: &str) -> Option<String> {
    non_blank(std::env::var(name).ok())
}

/// `value` as an environment reader would see it: `None` for a blank value.
#[must_use]
pub fn non_blank(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_kdvhn5_an_empty_or_blank_value_is_unset() {
        assert_eq!(non_blank(None), None);
        assert_eq!(non_blank(Some(String::new())), None);
        assert_eq!(non_blank(Some("  ".into())), None);
        assert_eq!(non_blank(Some("x".into())), Some("x".into()));
        // The test process itself never sees an inherited launched-session value.
        for name in LAUNCHED_SESSION_ENV {
            assert_eq!(value(name), None, "{name} reached a test process");
        }
        // Every variable listed here is forced empty for cargo's test processes.
        let config = include_str!("../../../.cargo/config.toml");
        for name in LAUNCHED_SESSION_ENV {
            assert!(
                config.contains(&format!("{name} = {{ value = \"\", force = true }}")),
                "{name} is not forced empty in .cargo/config.toml"
            );
        }
    }
}
