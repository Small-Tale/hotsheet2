//! Terminal **memory limits** (HS2-Y5DYC0): how much history a client keeps, how much output the
//! server replays to a reattaching viewer, and how many kept-alive terminals stay warm out of the
//! page.
//!
//! Each limit is a machine-wide setting (`${HOTSHEET_HOME}/settings.json`, set from App Settings →
//! Terminal memory or `hotsheet-cli settings set <key> <value> --scope global`). An unset limit
//! takes a default derived from the machine's physical memory, and the replay buffer is sized from
//! the history so a reconnect restores about what the client would have kept. An explicit setting
//! always wins over a derived default.
//!
//! These are pure setting definitions, kept in `ticketing` beside the other setting keys so the
//! CLI can validate them without linking the terminal crate (`docs/12` §12.2.1). The server reads
//! the physical memory (`hotsheet_terminals::physical_memory_bytes`) and applies the result.

use serde::{Deserialize, Serialize};

/// Setting key for the history (scrollback lines) a dedicated terminal keeps.
pub const HISTORY_LINES_SETTING: &str = "terminal.history_lines";
/// Setting key for the per-terminal server replay buffer, in bytes.
pub const REPLAY_BYTES_SETTING: &str = "terminal.replay_bytes";
/// Setting key for how many kept-alive terminals stay warm out of the page.
pub const WARM_TERMINALS_SETTING: &str = "terminal.warm_terminals";

/// Accepted history range, in lines.
pub const HISTORY_LINES_RANGE: (u32, u32) = (100, 100_000);
/// Accepted replay-buffer range, in bytes (64 KiB to 64 MiB).
pub const REPLAY_BYTES_RANGE: (u64, u64) = (64 * 1024, 64 * 1024 * 1024);
/// Accepted warm-terminal range.
pub const WARM_TERMINALS_RANGE: (u32, u32) = (0, 64);

/// Bytes of PTY output a typical history line costs, escapes included. A redraw-heavy TUI (Claude
/// Code, Codex) spends far more per line than plain shell output, so the estimate is generous.
pub const REPLAY_BYTES_PER_LINE: u64 = 192;
/// Bounds for the replay size derived from the history. The floor is the historical 256 KiB.
pub const DERIVED_REPLAY_BYTES_RANGE: (u64, u64) = (256 * 1024, 8 * 1024 * 1024);

const GIB: u64 = 1024 * 1024 * 1024;

/// Defaults a limit falls back to when it is not set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalLimitValues {
    pub history_lines: u32,
    pub replay_bytes: u64,
    pub warm_terminals: u32,
}

/// The explicit machine-wide settings; `None` means "use the derived default".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfiguredTerminalLimits {
    #[serde(default)]
    pub history_lines: Option<u32>,
    #[serde(default)]
    pub replay_bytes: Option<u64>,
    #[serde(default)]
    pub warm_terminals: Option<u32>,
}

/// The replay buffer that holds about `history_lines` of output, clamped.
pub fn replay_bytes_for_history(history_lines: u32) -> u64 {
    (u64::from(history_lines) * REPLAY_BYTES_PER_LINE)
        .clamp(DERIVED_REPLAY_BYTES_RANGE.0, DERIVED_REPLAY_BYTES_RANGE.1)
}

/// Defaults for a machine with `physical_memory` bytes. An unknown size keeps the historical
/// values (5,000 lines, 12 warm terminals). An xterm buffer costs roughly 12 bytes per cell, so
/// 5,000 lines at 120 columns is about 7 MB per live terminal; the tiers keep the warm set to a
/// small fraction of memory.
pub fn default_terminal_limits(physical_memory: Option<u64>) -> TerminalLimitValues {
    let (history_lines, warm_terminals) = match physical_memory {
        None => (5_000, 12),
        Some(bytes) if bytes < 8 * GIB => (2_500, 6),
        Some(bytes) if bytes < 16 * GIB => (5_000, 12),
        Some(bytes) if bytes < 32 * GIB => (10_000, 16),
        Some(_) => (20_000, 24),
    };
    TerminalLimitValues {
        history_lines,
        replay_bytes: replay_bytes_for_history(history_lines),
        warm_terminals,
    }
}

/// The effective limits: each explicit setting wins; an unset replay size follows the effective
/// history (configured or derived), so the two stay sized together.
pub fn effective_terminal_limits(
    defaults: TerminalLimitValues,
    configured: ConfiguredTerminalLimits,
) -> TerminalLimitValues {
    let history_lines = configured.history_lines.unwrap_or(defaults.history_lines);
    TerminalLimitValues {
        history_lines,
        replay_bytes: configured
            .replay_bytes
            .unwrap_or_else(|| replay_bytes_for_history(history_lines)),
        warm_terminals: configured.warm_terminals.unwrap_or(defaults.warm_terminals),
    }
}

/// Why a limit value was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{key} must be a whole number from {min} to {max}")]
pub struct LimitRangeError {
    pub key: &'static str,
    pub min: u64,
    pub max: u64,
}

fn check(key: &'static str, value: u64, (min, max): (u64, u64)) -> Result<(), LimitRangeError> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(LimitRangeError { key, min, max })
    }
}

/// Validate explicit limits against their accepted ranges.
pub fn validate_terminal_limits(limits: &ConfiguredTerminalLimits) -> Result<(), LimitRangeError> {
    let wide = |(min, max): (u32, u32)| (u64::from(min), u64::from(max));
    if let Some(value) = limits.history_lines {
        check(
            HISTORY_LINES_SETTING,
            u64::from(value),
            wide(HISTORY_LINES_RANGE),
        )?;
    }
    if let Some(value) = limits.replay_bytes {
        check(REPLAY_BYTES_SETTING, value, REPLAY_BYTES_RANGE)?;
    }
    if let Some(value) = limits.warm_terminals {
        check(
            WARM_TERMINALS_SETTING,
            u64::from(value),
            wide(WARM_TERMINALS_RANGE),
        )?;
    }
    Ok(())
}

/// Whether `key` is one of the machine-wide terminal limits.
pub fn is_terminal_limit_key(key: &str) -> bool {
    matches!(
        key,
        HISTORY_LINES_SETTING | REPLAY_BYTES_SETTING | WARM_TERMINALS_SETTING
    )
}

fn limit_range(key: &str) -> Option<(&'static str, (u64, u64))> {
    match key {
        HISTORY_LINES_SETTING => Some((
            HISTORY_LINES_SETTING,
            (
                u64::from(HISTORY_LINES_RANGE.0),
                u64::from(HISTORY_LINES_RANGE.1),
            ),
        )),
        REPLAY_BYTES_SETTING => Some((REPLAY_BYTES_SETTING, REPLAY_BYTES_RANGE)),
        WARM_TERMINALS_SETTING => Some((
            WARM_TERMINALS_SETTING,
            (
                u64::from(WARM_TERMINALS_RANGE.0),
                u64::from(WARM_TERMINALS_RANGE.1),
            ),
        )),
        _ => None,
    }
}

/// Parse a limit typed on the command line: a whole number within the key's range.
pub fn parse_terminal_limit(key: &str, raw: &str) -> Result<u64, LimitRangeError> {
    let (key, (min, max)) = limit_range(key).ok_or(LimitRangeError {
        key: "terminal limit",
        min: 0,
        max: 0,
    })?;
    raw.trim()
        .parse::<u64>()
        .ok()
        .filter(|value| (min..=max).contains(value))
        .ok_or(LimitRangeError { key, min, max })
}

/// Read one stored limit. A missing, non-numeric or out-of-range value (a hand-edited settings
/// file) is ignored so the derived default applies instead of a broken limit.
pub fn stored_limit(key: &str, value: Option<&serde_json::Value>) -> Option<u64> {
    let number = value?.as_u64()?;
    let (_, (min, max)) = limit_range(key)?;
    (min..=max).contains(&number).then_some(number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_memory_keeps_the_historical_defaults() {
        let defaults = default_terminal_limits(None);
        assert_eq!(defaults.history_lines, 5_000);
        assert_eq!(defaults.warm_terminals, 12);
        assert_eq!(defaults.replay_bytes, 5_000 * REPLAY_BYTES_PER_LINE);
    }

    #[test]
    fn defaults_grow_with_physical_memory_tiers() {
        let tier = |gib: u64| {
            let value = default_terminal_limits(Some(gib * GIB));
            (value.history_lines, value.warm_terminals)
        };
        assert_eq!(tier(4), (2_500, 6));
        assert_eq!(tier(8), (5_000, 12));
        assert_eq!(tier(16), (10_000, 16));
        assert_eq!(tier(31), (10_000, 16));
        assert_eq!(tier(32), (20_000, 24));
        assert_eq!(tier(128), (20_000, 24));
        // Exactly at a boundary moves up; just under it stays.
        assert_eq!(
            default_terminal_limits(Some(8 * GIB - 1)).history_lines,
            2_500
        );
    }

    #[test]
    fn replay_follows_history_within_its_clamp() {
        assert_eq!(replay_bytes_for_history(100), DERIVED_REPLAY_BYTES_RANGE.0);
        assert_eq!(
            replay_bytes_for_history(10_000),
            10_000 * REPLAY_BYTES_PER_LINE
        );
        assert_eq!(
            replay_bytes_for_history(100_000),
            DERIVED_REPLAY_BYTES_RANGE.1
        );
        // Every derived default stays within the clamp, including the largest tier.
        let large = default_terminal_limits(Some(64 * GIB));
        assert!(large.replay_bytes <= DERIVED_REPLAY_BYTES_RANGE.1);
    }

    #[test]
    fn an_explicit_setting_wins_and_an_unset_replay_follows_the_effective_history() {
        let defaults = default_terminal_limits(Some(64 * GIB));
        let none = effective_terminal_limits(defaults, ConfiguredTerminalLimits::default());
        assert_eq!(none, defaults);

        let history = effective_terminal_limits(
            defaults,
            ConfiguredTerminalLimits {
                history_lines: Some(2_000),
                ..Default::default()
            },
        );
        assert_eq!(history.history_lines, 2_000);
        assert_eq!(history.replay_bytes, replay_bytes_for_history(2_000));
        assert_eq!(history.warm_terminals, defaults.warm_terminals);

        let all = effective_terminal_limits(
            defaults,
            ConfiguredTerminalLimits {
                history_lines: Some(2_000),
                replay_bytes: Some(128 * 1024),
                warm_terminals: Some(0),
            },
        );
        assert_eq!(
            all,
            TerminalLimitValues {
                history_lines: 2_000,
                replay_bytes: 128 * 1024,
                warm_terminals: 0
            }
        );
    }

    #[test]
    fn validation_rejects_out_of_range_values() {
        assert!(validate_terminal_limits(&ConfiguredTerminalLimits::default()).is_ok());
        let error = validate_terminal_limits(&ConfiguredTerminalLimits {
            history_lines: Some(99),
            ..Default::default()
        })
        .unwrap_err();
        assert_eq!(error.key, HISTORY_LINES_SETTING);
        assert!(
            validate_terminal_limits(&ConfiguredTerminalLimits {
                replay_bytes: Some(REPLAY_BYTES_RANGE.1 + 1),
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            validate_terminal_limits(&ConfiguredTerminalLimits {
                warm_terminals: Some(65),
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            validate_terminal_limits(&ConfiguredTerminalLimits {
                history_lines: Some(100_000),
                replay_bytes: Some(64 * 1024),
                warm_terminals: Some(0),
            })
            .is_ok()
        );
    }

    #[test]
    fn command_line_limits_are_whole_numbers_in_range() {
        assert!(is_terminal_limit_key(HISTORY_LINES_SETTING));
        assert!(!is_terminal_limit_key(
            "terminal.inherit_global_shell_history"
        ));
        assert_eq!(
            parse_terminal_limit(HISTORY_LINES_SETTING, " 8000 "),
            Ok(8_000)
        );
        assert_eq!(parse_terminal_limit(WARM_TERMINALS_SETTING, "0"), Ok(0));
        let error = parse_terminal_limit(REPLAY_BYTES_SETTING, "1k").unwrap_err();
        assert_eq!(error.key, REPLAY_BYTES_SETTING);
        assert!(error.to_string().contains("65536 to 67108864"));
        assert!(parse_terminal_limit(WARM_TERMINALS_SETTING, "65").is_err());
        assert!(parse_terminal_limit("editor", "1").is_err());
    }

    #[test]
    fn stored_limits_ignore_broken_values() {
        let value = |json: &str| serde_json::from_str::<serde_json::Value>(json).unwrap();
        assert_eq!(
            stored_limit(HISTORY_LINES_SETTING, Some(&value("8000"))),
            Some(8_000)
        );
        assert_eq!(
            stored_limit(HISTORY_LINES_SETTING, Some(&value("50"))),
            None
        );
        assert_eq!(
            stored_limit(HISTORY_LINES_SETTING, Some(&value("\"8000\""))),
            None
        );
        assert_eq!(
            stored_limit(WARM_TERMINALS_SETTING, Some(&value("-1"))),
            None
        );
        assert_eq!(
            stored_limit(WARM_TERMINALS_SETTING, Some(&value("0"))),
            Some(0)
        );
        assert_eq!(stored_limit(REPLAY_BYTES_SETTING, None), None);
        assert_eq!(stored_limit("terminal.other", Some(&value("1"))), None);
    }
}
