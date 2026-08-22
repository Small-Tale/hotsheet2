//! **Environment scrubbing** (`docs/05` §5.4, HS1 §22.13.1). A PTY child shell must not
//! inherit Hot Sheet's own tool-marker variables (`TSX_*`, `npm_*`, `NODE_*`, and our own
//! `HOTSHEET_*` launch markers), which otherwise leak into the AI tool and its subprocesses
//! and confuse tool detection. [`scrub_env`] drops those from a base environment.

/// Prefixes of variables that shouldn't reach a spawned tool shell.
const SCRUB_PREFIXES: &[&str] = &["TSX_", "npm_", "NODE_", "HOTSHEET_"];

/// Filter a base environment down to what a child terminal should inherit — dropping any
/// variable whose name starts with a scrubbed prefix.
pub fn scrub_env<I, K, V>(base: I) -> Vec<(String, String)>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    base.into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .filter(|(k, _)| !SCRUB_PREFIXES.iter().any(|p| k.starts_with(p)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_tool_markers_keeps_the_rest() {
        let base = [
            ("PATH", "/usr/bin"),
            ("TSX_something", "x"),
            ("npm_config_foo", "y"),
            ("NODE_OPTIONS", "z"),
            ("HOTSHEET_HOME", "~/.hotsheet2"),
            ("HOME", "/home/me"),
        ];
        let out: Vec<(String, String)> = scrub_env(base);
        let names: Vec<&str> = out.iter().map(|(k, _)| k.as_str()).collect();
        assert!(names.contains(&"PATH") && names.contains(&"HOME"));
        assert!(!names.iter().any(|k| k.starts_with("TSX_")
            || k.starts_with("npm_")
            || k.starts_with("NODE_")
            || k.starts_with("HOTSHEET_")));
    }
}
