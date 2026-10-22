//! Read-only freshness of a standalone Rulesync lock, separate from Hot Sheet pins.

use serde::Serialize;
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const VALIDATED_VERSION: &str = "38.0.0";
const OUTPUT_LIMIT: u64 = 1_048_576;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LockState {
    Current,
    Outdated,
    Unlocked,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RulesyncSourceUpdate {
    pub source: String,
    pub state: LockState,
    pub requested_ref: Option<String>,
    pub locked_ref: Option<String>,
    pub latest_ref: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RulesyncUpdateReport {
    pub state: LockState,
    pub executable_version: Option<String>,
    pub sources: Vec<RulesyncSourceUpdate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

impl RulesyncUpdateReport {
    fn unknown(version: Option<String>, reason: &'static str) -> Self {
        Self {
            state: LockState::Unknown,
            executable_version: version,
            sources: Vec::new(),
            reason: Some(reason),
        }
    }
}

/// `None` means this project does not use standalone Rulesync sources.
/// This never installs Rulesync, changes a lock, or generates instruction files.
pub fn check_project(project: &Path) -> Option<RulesyncUpdateReport> {
    let executable = find_executable();
    check_project_with_executable(project, executable.as_deref())
}

fn check_project_with_executable(
    project: &Path,
    executable: Option<&Path>,
) -> Option<RulesyncUpdateReport> {
    if !project.join("rulesync.jsonc").is_file()
        || project.join(crate::guidance_sources::CONFIG_PATH).exists()
    {
        return None;
    }
    let Some(executable) = executable else {
        return Some(RulesyncUpdateReport::unknown(
            None,
            "Rulesync 38.0.0 is not installed",
        ));
    };
    let version_output = run(executable, project, &["--version"], Duration::from_secs(5));
    let Ok((status, bytes, _)) = version_output else {
        return Some(RulesyncUpdateReport::unknown(
            None,
            "Rulesync version could not be checked",
        ));
    };
    let version = String::from_utf8(bytes)
        .ok()
        .map(|text| text.trim().to_owned());
    if !status.success() || version.as_deref() != Some(VALIDATED_VERSION) {
        return Some(RulesyncUpdateReport::unknown(
            version,
            "Rulesync 38.0.0 is required for this check",
        ));
    }
    let Ok((status, stdout, stderr)) = run(
        executable,
        project,
        &["--json", "install", "--outdated"],
        Duration::from_secs(30),
    ) else {
        return Some(RulesyncUpdateReport::unknown(
            version,
            "Rulesync update check could not finish",
        ));
    };
    Some(parse_outdated(status.code(), &stdout, &stderr, version))
}

fn find_executable() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "rulesync.exe"
    } else {
        "rulesync"
    };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|directory| {
        let candidate = directory.join(name);
        candidate
            .is_file()
            .then(|| candidate.canonicalize().ok())
            .flatten()
    })
}

fn run(
    executable: &Path,
    project: &Path,
    args: &[&str],
    timeout: Duration,
) -> std::io::Result<(ExitStatus, Vec<u8>, Vec<u8>)> {
    // Files avoid pipes filling while Rulesync checks projects with many sources.
    // Rulesync 38 prints JSON failures (including outdated sources) on stderr.
    let stdout = tempfile::tempfile()?;
    let stderr = tempfile::tempfile()?;
    let mut child = Command::new(executable)
        .args(args)
        .current_dir(project)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(Stdio::from(stdout.try_clone()?))
        .stderr(Stdio::from(stderr.try_clone()?))
        .spawn()?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Rulesync timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    Ok((status, read_output(stdout)?, read_output(stderr)?))
}

fn read_output(mut file: File) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.seek(SeekFrom::Start(0))?;
    file.take(OUTPUT_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > OUTPUT_LIMIT {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Rulesync output too large",
        ));
    }
    Ok(bytes)
}

fn parse_outdated(
    exit: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
    version: Option<String>,
) -> RulesyncUpdateReport {
    let unknown =
        || RulesyncUpdateReport::unknown(version.clone(), "Rulesync update response was invalid");
    if !matches!(exit, Some(0..=2)) {
        return unknown();
    }
    let Ok(body) = serde_json::from_slice::<Value>(stdout)
        .or_else(|_| serde_json::from_slice::<Value>(stderr))
    else {
        return unknown();
    };
    if body.get("command").and_then(Value::as_str) != Some("install")
        || body.get("version").and_then(Value::as_str) != Some(VALIDATED_VERSION)
        || body.get("success").and_then(Value::as_bool) != Some(exit == Some(0))
    {
        return unknown();
    }
    let source_values = if exit == Some(0) {
        body.pointer("/data/sources")
    } else {
        body.pointer("/error/details/sources")
    };
    let Some(source_values) = source_values.and_then(Value::as_array) else {
        return unknown();
    };
    let mut sources = Vec::with_capacity(source_values.len());
    for row in source_values {
        let Some(source) = row.get("source").and_then(Value::as_str) else {
            return unknown();
        };
        let state = match row.get("status").and_then(Value::as_str) {
            Some("up-to-date") => LockState::Current,
            Some("outdated") => LockState::Outdated,
            Some("not-locked") => LockState::Unlocked,
            Some("failed") => LockState::Unknown,
            _ => return unknown(),
        };
        sources.push(RulesyncSourceUpdate {
            source: source.to_owned(),
            state,
            requested_ref: row
                .get("requestedRef")
                .and_then(Value::as_str)
                .map(str::to_owned),
            locked_ref: row
                .get("lockedRef")
                .and_then(Value::as_str)
                .map(str::to_owned),
            latest_ref: row
                .get("latestRef")
                .and_then(Value::as_str)
                .map(str::to_owned),
        });
    }
    if exit == Some(0)
        && sources
            .iter()
            .any(|source| source.state != LockState::Current)
    {
        return unknown();
    }
    if exit == Some(2)
        && !sources
            .iter()
            .any(|source| source.state == LockState::Unknown)
    {
        return unknown();
    }
    if exit == Some(1)
        && !sources
            .iter()
            .any(|source| matches!(source.state, LockState::Outdated | LockState::Unlocked))
    {
        return unknown();
    }
    let state = if sources
        .iter()
        .any(|source| source.state == LockState::Unknown)
    {
        LockState::Unknown
    } else if sources
        .iter()
        .any(|source| source.state == LockState::Unlocked)
    {
        LockState::Unlocked
    } else if sources
        .iter()
        .any(|source| source.state == LockState::Outdated)
    {
        LockState::Outdated
    } else {
        LockState::Current
    };
    RulesyncUpdateReport {
        state,
        executable_version: version,
        sources,
        reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rulesync_38_current_outdated_unlocked_and_unknown() {
        let version = Some(VALIDATED_VERSION.to_owned());
        let row = |status: &str| {
            serde_json::json!({
                "source": "https://example.test/rules.git", "status": status,
                "requestedRef": "main", "lockedRef": "first", "latestRef": "second"
            })
        };
        for (exit, status, expected) in [
            (0, "up-to-date", LockState::Current),
            (1, "outdated", LockState::Outdated),
            (1, "not-locked", LockState::Unlocked),
            (2, "failed", LockState::Unknown),
        ] {
            let body = if exit == 0 {
                serde_json::json!({"success":true,"command":"install","version":VALIDATED_VERSION,"data":{"sources":[row(status)]}})
            } else {
                serde_json::json!({"success":false,"command":"install","version":VALIDATED_VERSION,"error":{"details":{"sources":[row(status)]}}})
            };
            let json = body.to_string();
            let (stdout, stderr) = if exit == 0 {
                (json.as_bytes(), &b""[..])
            } else {
                (&b""[..], json.as_bytes())
            };
            let parsed = parse_outdated(Some(exit), stdout, stderr, version.clone());
            assert_eq!(parsed.state, expected);
            assert_eq!(parsed.sources[0].state, expected);
            assert_eq!(
                parsed.executable_version.as_deref(),
                Some(VALIDATED_VERSION)
            );
        }
        assert_eq!(
            parse_outdated(Some(1), b"not json", b"", version).state,
            LockState::Unknown
        );
    }

    #[test]
    fn mixed_sources_and_inconsistent_exit_codes_never_claim_a_fresh_lock() {
        let row = |source: &str, status: &str| serde_json::json!({"source":source,"status":status});
        let mixed = serde_json::json!({
            "success": false, "command": "install", "version": VALIDATED_VERSION,
            "error": {"details": {"sources": [row("a", "outdated"), row("b", "not-locked")]}}
        });
        let json = mixed.to_string();
        let report = parse_outdated(
            Some(1),
            b"",
            json.as_bytes(),
            Some(VALIDATED_VERSION.into()),
        );
        assert_eq!(report.state, LockState::Unlocked);
        assert_eq!(report.sources[0].state, LockState::Outdated);
        assert_eq!(report.sources[1].state, LockState::Unlocked);

        let inconsistent = serde_json::json!({
            "success": true, "command": "install", "version": VALIDATED_VERSION,
            "data": {"sources": [row("a", "outdated")]}
        });
        let report = parse_outdated(Some(0), inconsistent.to_string().as_bytes(), b"", None);
        assert_eq!(report.state, LockState::Unknown);
        assert!(report.sources.is_empty());
    }

    #[test]
    fn skips_hot_sheet_pins_and_missing_rulesync_config() {
        let project = tempfile::tempdir().unwrap();
        assert!(check_project_with_executable(project.path(), None).is_none());
        std::fs::write(project.path().join("rulesync.jsonc"), "{}").unwrap();
        assert_eq!(
            check_project_with_executable(project.path(), None)
                .unwrap()
                .state,
            LockState::Unknown
        );
        std::fs::create_dir(project.path().join(".hotsheet2")).unwrap();
        std::fs::write(
            project.path().join(crate::guidance_sources::CONFIG_PATH),
            "{}",
        )
        .unwrap();
        assert!(check_project_with_executable(project.path(), None).is_none());
    }
}
