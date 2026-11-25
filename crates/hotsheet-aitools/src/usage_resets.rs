//! HS2-JM0GVB: quota reset times from a tool's own usage report.
//!
//! Antigravity reports a usage limit without a reset time (HS2-86MK1Y), but `agy -p
//! "/usage" --output-format json` answers, without an agent turn or quota spend (observed
//! on agy 1.5.10: zero turns and zero tokens), with
//! `command.data.groups[].buckets[]`, each bucket carrying an `id`, a `window`, a
//! `remaining_fraction` and an RFC 3339 `reset_time`. A plugin names that query in
//! `[drive] usage_query_args`; the host runs it once when a driven turn fails on a typed
//! usage limit, never on a timer. Only the typed members are read: ids, windows,
//! fractions and times. Group and bucket names and descriptions are never kept.

use std::path::Path;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Bounds on what one report may hold; anything beyond is ignored.
const MAX_GROUPS: usize = 32;
const MAX_BUCKETS: usize = 64;
/// The largest report read, in bytes.
pub const MAX_REPORT_BYTES: usize = 256 * 1024;

/// One quota bucket of a usage report.
#[derive(Debug, Clone, PartialEq)]
pub struct QuotaBucket {
    pub id: String,
    pub window: Option<String>,
    /// The share of the bucket left, `0.0` when spent; `None` when not reported.
    pub remaining_fraction: Option<f64>,
    pub resets_at: OffsetDateTime,
}

impl QuotaBucket {
    /// Spent: nothing remains in it.
    pub fn exhausted(&self) -> bool {
        self.remaining_fraction.is_some_and(|left| left <= 0.0)
    }
}

/// Every well-formed bucket in a usage report, in report order. A malformed report,
/// group or bucket contributes nothing; nothing here fails.
pub fn parse_buckets(report: &[u8]) -> Vec<QuotaBucket> {
    if report.len() > MAX_REPORT_BYTES {
        return Vec::new();
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(report) else {
        return Vec::new();
    };
    let Some(groups) = value
        .pointer("/command/data/groups")
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    groups
        .iter()
        .take(MAX_GROUPS)
        .filter_map(|group| group.get("buckets").and_then(serde_json::Value::as_array))
        .flatten()
        .take(MAX_BUCKETS)
        .filter_map(|bucket| {
            let id = bucket.get("id")?.as_str()?.trim();
            if id.is_empty() || id.len() > 160 {
                return None;
            }
            let resets_at =
                OffsetDateTime::parse(bucket.get("reset_time")?.as_str()?, &Rfc3339).ok()?;
            Some(QuotaBucket {
                id: id.to_owned(),
                window: bucket
                    .get("window")
                    .and_then(serde_json::Value::as_str)
                    .filter(|window| !window.is_empty() && window.len() <= 64)
                    .map(str::to_owned),
                remaining_fraction: bucket
                    .get("remaining_fraction")
                    .and_then(serde_json::Value::as_f64)
                    .filter(|fraction| fraction.is_finite()),
                resets_at,
            })
        })
        .collect()
}

/// When the limit hit at `limited_at` ends, by the report: the latest reset of a spent
/// bucket that is still ahead. The report does not say which bucket the failed turn
/// drew on, so the latest is the one that cannot end the limit early. `None` when no
/// bucket is spent, which leaves the registry's own backoff in force.
pub fn exhausted_reset(
    buckets: &[QuotaBucket],
    limited_at: OffsetDateTime,
) -> Option<OffsetDateTime> {
    buckets
        .iter()
        .filter(|bucket| bucket.exhausted() && bucket.resets_at > limited_at)
        .map(|bucket| bucket.resets_at)
        .max()
}

/// Run the plugin's usage query once, bounded, and read the reset of the limit hit at
/// `limited_at`. Any failure (the program is missing or untrusted, it fails, times out
/// or answers something else) is `None`.
pub fn query_reset(
    program: &str,
    args: &[String],
    identity_policy: Option<&str>,
    cwd: &Path,
    limited_at: OffsetDateTime,
) -> Option<OffsetDateTime> {
    let program = crate::model_catalog::trusted_program(program, identity_policy).ok()?;
    let mut command = hotsheet_ticketing::git::launch_resolved(
        &program,
        std::env::var_os("PATH").as_deref(),
        Some(cwd),
    );
    command.args(args).current_dir(cwd);
    let output = crate::probe::run_probe(&mut command).ok()?;
    if !output.status.success() {
        return None;
    }
    exhausted_reset(&parse_buckets(&output.stdout), limited_at)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape observed from agy 1.5.10's `/usage`, with synthetic names and values.
    fn report(buckets: serde_json::Value) -> Vec<u8> {
        serde_json::json!({
            "conversation_id": "synthetic",
            "status": "SUCCESS",
            "response": "",
            "duration_seconds": 0,
            "num_turns": 0,
            "usage": {"input_tokens": 0, "output_tokens": 0, "thinking_tokens": 0,
                "cache_read_tokens": 0, "total_tokens": 0},
            "command": {"name": "usage", "data": {"description": "Synthetic", "groups": [
                {"name": "Group A", "description": "Synthetic", "buckets": buckets},
                {"name": "Group B", "description": "Synthetic", "buckets": [
                    {"id": "b-weekly", "name": "B", "description": "Synthetic",
                     "window": "weekly", "remaining_fraction": 1, "reset_time": "2026-12-02T00:08:51Z"}
                ]}
            ]}}
        })
        .to_string()
        .into_bytes()
    }

    fn at(text: &str) -> OffsetDateTime {
        OffsetDateTime::parse(text, &Rfc3339).unwrap()
    }

    /// HS2-JM0GVB: the observed shape parses to typed buckets, and only a spent bucket
    /// still ahead of the limit names its end.
    #[test]
    fn hs2_jm0gvb_a_spent_bucket_names_when_the_limit_ends() {
        let limited = at("2026-11-24T12:00:00Z");
        let spent = report(serde_json::json!([
            {"id": "a-weekly", "name": "A", "description": "Synthetic", "window": "weekly",
             "remaining_fraction": 0, "reset_time": "2026-12-01T05:28:13Z"},
            {"id": "a-daily", "name": "A", "description": "Synthetic", "window": "daily",
             "remaining_fraction": 0.0, "reset_time": "2026-11-25T00:00:00Z"}
        ]));
        let buckets = parse_buckets(&spent);
        assert_eq!(
            buckets
                .iter()
                .map(|bucket| (bucket.id.as_str(), bucket.exhausted()))
                .collect::<Vec<_>>(),
            [("a-weekly", true), ("a-daily", true), ("b-weekly", false)]
        );
        assert_eq!(buckets[0].window.as_deref(), Some("weekly"));
        // Two spent buckets: the later reset, which cannot end the limit early.
        assert_eq!(
            exhausted_reset(&buckets, limited),
            Some(at("2026-12-01T05:28:13Z"))
        );
        // Nothing spent (a rate limit, or a report that disagrees): no reset.
        let partial = parse_buckets(&report(serde_json::json!([
            {"id": "a-weekly", "window": "weekly", "remaining_fraction": 0.02,
             "reset_time": "2026-12-01T05:28:13Z"}
        ])));
        assert_eq!(exhausted_reset(&partial, limited), None);
        // A spent bucket whose reset already passed says nothing about this limit.
        let past = parse_buckets(&report(serde_json::json!([
            {"id": "a-weekly", "remaining_fraction": 0, "reset_time": "2026-11-24T11:00:00Z"}
        ])));
        assert_eq!(exhausted_reset(&past, limited), None);
        // No fraction reported: not known to be spent.
        let unknown = parse_buckets(&report(serde_json::json!([
            {"id": "a-weekly", "reset_time": "2026-12-01T05:28:13Z"}
        ])));
        assert!(!unknown[0].exhausted());
    }

    /// HS2-JM0GVB: malformed reports, groups and buckets contribute nothing.
    #[test]
    fn hs2_jm0gvb_malformed_reports_yield_nothing() {
        for bad in [
            &b""[..],
            b"not json",
            b"{}",
            br#"{"command":{"data":{"groups":"x"}}}"#,
            br#"{"command":{"data":{"groups":[{"buckets":{}}]}}}"#,
        ] {
            assert!(parse_buckets(bad).is_empty());
        }
        let buckets = parse_buckets(&report(serde_json::json!([
            {"remaining_fraction": 0, "reset_time": "2026-12-01T05:28:13Z"},
            {"id": "", "remaining_fraction": 0, "reset_time": "2026-12-01T05:28:13Z"},
            {"id": "a", "remaining_fraction": 0, "reset_time": "next week"},
            {"id": "a", "remaining_fraction": 0},
            {"id": "ok", "remaining_fraction": "0", "reset_time": "2026-12-01T05:28:13Z"}
        ])));
        // Only the well-formed bucket and the second group's remain; a string
        // fraction is not a reported fraction.
        assert_eq!(
            buckets
                .iter()
                .map(|bucket| (bucket.id.as_str(), bucket.remaining_fraction))
                .collect::<Vec<_>>(),
            [("ok", None), ("b-weekly", Some(1.0))]
        );
        assert!(parse_buckets(&vec![b' '; MAX_REPORT_BYTES + 1]).is_empty());
    }

    /// HS2-JM0GVB: the query runs the declared program once and reads its answer; a
    /// failing or missing program yields nothing.
    #[cfg(unix)]
    #[test]
    fn hs2_jm0gvb_the_query_runs_once_and_fails_closed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let report_path = dir.path().join("report.json");
        std::fs::write(
            &report_path,
            report(serde_json::json!([
                {"id": "a-weekly", "remaining_fraction": 0, "reset_time": "2026-12-01T05:28:13Z"}
            ])),
        )
        .unwrap();
        let script = dir.path().join("fake-agy");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho \"$@\" >> '{}'\nexec cat '{}'\n",
                dir.path().join("calls").display(),
                report_path.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let args = ["-p", "/usage", "--output-format", "json"].map(String::from);
        let limited = at("2026-11-24T12:00:00Z");
        assert_eq!(
            query_reset(script.to_str().unwrap(), &args, None, dir.path(), limited),
            Some(at("2026-12-01T05:28:13Z"))
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("calls")).unwrap(),
            "-p /usage --output-format json\n"
        );
        let failing = dir.path().join("failing");
        std::fs::write(&failing, "#!/bin/sh\nexit 3\n").unwrap();
        std::fs::set_permissions(&failing, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            query_reset(failing.to_str().unwrap(), &args, None, dir.path(), limited),
            None
        );
        assert_eq!(
            query_reset("/nonexistent/agy", &args, None, dir.path(), limited),
            None
        );
        assert_eq!(
            query_reset(
                script.to_str().unwrap(),
                &args,
                Some("unknown-policy"),
                dir.path(),
                limited
            ),
            None,
            "an untrusted identity is never run"
        );
    }
}
