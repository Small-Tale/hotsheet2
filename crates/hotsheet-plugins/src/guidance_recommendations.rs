//! Read-only project evidence and explicit review of pinned guidance candidates.

use crate::guidance_sources::{self as sources, GuidanceSourceError};
use hotsheet_ticketing::{Scope, Settings, SettingsError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::Path;

const SETTINGS_KEY: &str = "guidance_recommendations_v1";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum RecommendationError {
    #[error(transparent)]
    Source(#[from] GuidanceSourceError),
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error("invalid recommendation review: {0}")]
    Invalid(String),
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ProjectSignal {
    pub key: String,
    pub evidence: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Recommendation {
    pub source: String,
    pub kind: String,
    pub name: String,
    pub version: u64,
    pub description: String,
    pub pinned_commit: String,
    pub evidence: Vec<String>,
    pub fingerprint: String,
    pub selected: bool,
    pub installed_version: Option<u64>,
    /// `new`, `accepted`, `dismissed`, or `selected`.
    pub state: String,
    #[serde(skip)]
    digest: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct RecommendationReport {
    pub fingerprint: String,
    pub signals: Vec<ProjectSignal>,
    pub candidates: Vec<Recommendation>,
    pub changes: Vec<RecommendationChange>,
    pub source_registration_suggested: bool,
    /// First explicit selection changes what a later `guidance apply` installs.
    pub first_accept_replaces_bundled_default: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct RecommendationChange {
    pub source: String,
    pub kind: String,
    pub name: String,
    /// `new`, `changed`, or `withdrawn` relative to the last decision for this item.
    pub change: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct DecisionRecord {
    source: String,
    kind: String,
    name: String,
    fingerprint: String,
    decision: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct ReviewState {
    #[serde(default)]
    decisions: Vec<DecisionRecord>,
}

impl ReviewState {
    fn validate(&self) -> Result<(), RecommendationError> {
        let mut seen = std::collections::HashSet::new();
        if self.decisions.iter().any(|record| {
            !matches!(record.decision.as_str(), "accepted" | "dismissed")
                || !seen.insert((&record.source, &record.kind, &record.name))
        }) {
            return Err(RecommendationError::Invalid(
                "invalid stored recommendation decisions".into(),
            ));
        }
        Ok(())
    }
}

fn hash(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn file_signal(project: &Path, relative: &str, key: &str, signals: &mut Vec<ProjectSignal>) {
    let path = project.join(relative);
    if fs::symlink_metadata(&path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= MAX_MANIFEST_BYTES)
    {
        signals.push(ProjectSignal {
            key: key.into(),
            evidence: relative.into(),
        });
    }
}

fn project_signals(project: &Path) -> Vec<ProjectSignal> {
    let mut signals = Vec::new();
    for (file, key) in [
        ("Cargo.toml", "rust"),
        ("package.json", "javascript"),
        ("tsconfig.json", "typescript"),
        ("pyproject.toml", "python"),
        ("go.mod", "go"),
        ("Package.swift", "swift"),
        ("Gemfile", "ruby"),
    ] {
        file_signal(project, file, key, &mut signals);
    }
    let package = project.join("package.json");
    if fs::symlink_metadata(&package)
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= MAX_MANIFEST_BYTES)
    {
        let mut bytes = Vec::new();
        if fs::File::open(&package)
            .and_then(|file| file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes))
            .is_ok()
            && bytes.len() as u64 <= MAX_MANIFEST_BYTES
        {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                for (dependency, key) in [
                    ("react", "react"),
                    ("vue", "vue"),
                    ("svelte", "svelte"),
                    ("next", "nextjs"),
                    ("@kerfjs/ui", "kerf"),
                ] {
                    if ["dependencies", "devDependencies"]
                        .iter()
                        .any(|section| value[*section].get(dependency).is_some())
                    {
                        signals.push(ProjectSignal {
                            key: key.into(),
                            evidence: format!("package.json: {dependency}"),
                        });
                    }
                }
            }
        }
    }
    signals.sort_by(|a, b| a.key.cmp(&b.key).then(a.evidence.cmp(&b.evidence)));
    signals.dedup();
    signals
}

fn settings(project: &Path) -> Settings {
    Settings::for_project(project)
}

fn read_state(project: &Path) -> Result<ReviewState, RecommendationError> {
    let state: ReviewState = settings(project)
        .get(SETTINGS_KEY, Scope::Shared)?
        .map(|value| {
            serde_json::from_value(value)
                .map_err(|error| RecommendationError::Invalid(error.to_string()))
        })
        .transpose()
        .map(|state| state.unwrap_or_default())?;
    state.validate()?;
    Ok(state)
}

/// Inspect bounded local evidence and exact cached source snapshots without writing.
pub fn scan(project: &Path) -> Result<RecommendationReport, RecommendationError> {
    let config = sources::load(project)?;
    let state = read_state(project)?;
    let signals = project_signals(project);
    let mut candidates = Vec::new();
    for source in &config.sources {
        for rule in &source.rules {
            let evidence = signals
                .iter()
                .filter(|signal| {
                    rule.tags
                        .iter()
                        .any(|tag| tag.eq_ignore_ascii_case(&signal.key))
                })
                .map(|signal| signal.evidence.clone())
                .collect::<Vec<_>>();
            if evidence.is_empty() {
                continue;
            }
            candidates.push(candidate(
                &state,
                &source.id,
                &source.commit,
                "rule",
                &rule.name,
                rule.version,
                &rule.sha256,
                &rule.description,
                evidence,
                source.selected_rules.contains(&rule.name),
                sources::installed_version(project, &rule.name),
            ));
        }
        for skill in &source.skills {
            let evidence = signals
                .iter()
                .filter(|signal| {
                    skill.name == signal.key || skill.name.starts_with(&format!("{}-", signal.key))
                })
                .map(|signal| signal.evidence.clone())
                .collect::<Vec<_>>();
            if evidence.is_empty() {
                continue;
            }
            candidates.push(candidate(
                &state,
                &source.id,
                &source.commit,
                "skill",
                &skill.name,
                0,
                &sources::skill_digest(skill),
                &format!("{} skill from {}", skill.name, source.id),
                evidence,
                source.selected_skills.contains(&skill.name),
                None,
            ));
        }
    }
    candidates.sort_by(|a, b| (&a.source, &a.kind, &a.name).cmp(&(&b.source, &b.kind, &b.name)));
    let mut changes = candidates
        .iter()
        .filter_map(|candidate| {
            if candidate.selected {
                return None;
            }
            let previous = state.decisions.iter().find(|record| {
                record.source == candidate.source
                    && record.kind == candidate.kind
                    && record.name == candidate.name
            });
            let change = match previous {
                None => "new",
                Some(record) if record.fingerprint != candidate.fingerprint => "changed",
                Some(_) => return None,
            };
            Some(RecommendationChange {
                source: candidate.source.clone(),
                kind: candidate.kind.clone(),
                name: candidate.name.clone(),
                change: change.into(),
            })
        })
        .collect::<Vec<_>>();
    changes.extend(
        state
            .decisions
            .iter()
            .filter(|record| {
                !candidates.iter().any(|candidate| {
                    candidate.source == record.source
                        && candidate.kind == record.kind
                        && candidate.name == record.name
                })
            })
            .map(|record| RecommendationChange {
                source: record.source.clone(),
                kind: record.kind.clone(),
                name: record.name.clone(),
                change: "withdrawn".into(),
            }),
    );
    changes.sort_by(|a, b| (&a.source, &a.kind, &a.name).cmp(&(&b.source, &b.kind, &b.name)));
    let mut fingerprint_parts = signals
        .iter()
        .map(|signal| format!("{}:{}", signal.key, signal.evidence))
        .collect::<Vec<_>>();
    fingerprint_parts.extend(
        config
            .sources
            .iter()
            .map(|source| format!("source:{}:{}", source.id, source.commit)),
    );
    fingerprint_parts.extend(
        candidates
            .iter()
            .map(|candidate| candidate.fingerprint.clone()),
    );
    let report_fingerprint = hash(
        &fingerprint_parts
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    Ok(RecommendationReport {
        fingerprint: report_fingerprint,
        signals,
        candidates,
        changes,
        source_registration_suggested: config.sources.is_empty(),
        first_accept_replaces_bundled_default: !config.explicit_selection
            && !config.sources.is_empty(),
    })
}

#[allow(clippy::too_many_arguments)]
fn candidate(
    state: &ReviewState,
    source: &str,
    commit: &str,
    kind: &str,
    name: &str,
    version: u64,
    digest: &str,
    description: &str,
    evidence: Vec<String>,
    selected: bool,
    installed_version: Option<u64>,
) -> Recommendation {
    let version_text = version.to_string();
    let fingerprint = hash(&[
        source,
        kind,
        name,
        &version_text,
        digest,
        &evidence.join("\n"),
    ]);
    let state_name = if selected {
        "selected"
    } else {
        state
            .decisions
            .iter()
            .find(|decision| {
                decision.source == source
                    && decision.kind == kind
                    && decision.name == name
                    && decision.fingerprint == fingerprint
            })
            .map_or("new", |decision| decision.decision.as_str())
    };
    Recommendation {
        source: source.into(),
        kind: kind.into(),
        name: name.into(),
        version,
        description: description.into(),
        pinned_commit: commit.into(),
        evidence,
        fingerprint,
        selected,
        installed_version,
        state: state_name.into(),
        digest: digest.into(),
    }
}

/// Accept or dismiss one exact reviewed candidate. Accept selects it but never applies.
pub fn decide(
    project: &Path,
    source: &str,
    kind: &str,
    name: &str,
    fingerprint: &str,
    decision: &str,
) -> Result<RecommendationReport, RecommendationError> {
    if !matches!(decision, "accepted" | "dismissed") {
        return Err(RecommendationError::Invalid("unknown decision".into()));
    }
    let report = scan(project)?;
    let candidate = report
        .candidates
        .iter()
        .find(|candidate| {
            candidate.source == source
                && candidate.kind == kind
                && candidate.name == name
                && candidate.fingerprint == fingerprint
        })
        .ok_or_else(|| {
            RecommendationError::Invalid("recommendation changed since review".into())
        })?;
    if decision == "dismissed" && candidate.selected {
        return Err(RecommendationError::Invalid(
            "selected guidance cannot be dismissed without deselecting it".into(),
        ));
    }
    if decision == "accepted" {
        sources::select_item(
            project,
            source,
            kind,
            name,
            candidate.version,
            &candidate.digest,
        )?;
    }
    let settings = settings(project);
    for _ in 0..8 {
        if settings.get(SETTINGS_KEY, Scope::Local)?.is_some() {
            return Err(RecommendationError::Invalid(
                "local recommendation state shadows project decisions".into(),
            ));
        }
        let previous = settings.get_effective(SETTINGS_KEY)?;
        let mut state: ReviewState = previous
            .clone()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| RecommendationError::Invalid(error.to_string()))?
            .unwrap_or_default();
        state.validate()?;
        state.decisions.retain(|record| {
            !(record.source == source && record.kind == kind && record.name == name)
        });
        state.decisions.push(DecisionRecord {
            source: source.into(),
            kind: kind.into(),
            name: name.into(),
            fingerprint: fingerprint.into(),
            decision: decision.into(),
        });
        state
            .decisions
            .sort_by(|a, b| (&a.source, &a.kind, &a.name).cmp(&(&b.source, &b.kind, &b.name)));
        let value = serde_json::to_value(state)
            .map_err(|error| RecommendationError::Invalid(error.to_string()))?;
        if settings.set_if(SETTINGS_KEY, value, Scope::Shared, |current| {
            current == previous.as_ref()
        })? {
            return scan(project);
        }
    }
    Err(RecommendationError::Invalid(
        "recommendation decisions changed concurrently; retry".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guidance_sources::{
        GuidanceConfig, GuidanceSource, SourceRule, SourceSkill, SourceSkillFile,
    };

    fn rule(name: &str, version: u64, tag: &str) -> SourceRule {
        let body = format!("{name} version {version}");
        SourceRule {
            name: name.into(),
            version,
            sha256: format!("{:x}", Sha256::digest(body.as_bytes())),
            description: format!("Use {name}"),
            owner: None,
            tags: vec![tag.into()],
            body,
            known_bodies: Vec::new(),
            minimum_hotsheet_version: None,
            historical_versions: Vec::new(),
            selected_sha256: None,
        }
    }

    fn source(rules: Vec<SourceRule>) -> GuidanceSource {
        GuidanceSource {
            id: "fixture".into(),
            repository: "local fixture".into(),
            requested_ref: "v1".into(),
            commit: "a".repeat(40),
            rules,
            skills: Vec::new(),
            selected_rules: Vec::new(),
            selected_skills: Vec::new(),
        }
    }

    #[test]
    fn empty_refill_dismiss_version_change_accept_and_restart() {
        let root = tempfile::tempdir().unwrap();
        assert!(scan(root.path()).unwrap().source_registration_suggested);
        fs::write(root.path().join("Cargo.toml"), "[package]\nname='demo'\n").unwrap();
        let mut config = GuidanceConfig {
            schema_version: 1,
            explicit_selection: false,
            sources: vec![source(vec![
                rule("rust-format", 1, "rust"),
                rule("python", 1, "python"),
            ])],
        };
        sources::save(root.path(), &config).unwrap();
        let first = scan(root.path()).unwrap();
        assert_eq!(first.candidates.len(), 1);
        assert_eq!(first.candidates[0].state, "new");
        assert_eq!(first.changes[0].change, "new");
        assert!(first.first_accept_replaces_bundled_default);
        assert_eq!(first.candidates[0].evidence, ["Cargo.toml"]);
        assert!(!root.path().join(".hotsheet2/settings.json").exists());
        let candidate = &first.candidates[0];
        let dismissed = decide(
            root.path(),
            "fixture",
            "rule",
            "rust-format",
            &candidate.fingerprint,
            "dismissed",
        )
        .unwrap();
        assert_eq!(dismissed.candidates[0].state, "dismissed");
        assert!(dismissed.changes.is_empty());
        assert_eq!(scan(root.path()).unwrap().candidates[0].state, "dismissed");

        config.sources[0].rules[0] = rule("rust-format", 2, "rust");
        sources::save(root.path(), &config).unwrap();
        let changed = scan(root.path()).unwrap();
        assert_eq!(changed.candidates[0].state, "new");
        assert_eq!(changed.changes[0].change, "changed");
        assert_ne!(changed.candidates[0].fingerprint, candidate.fingerprint);
        assert!(
            decide(
                root.path(),
                "fixture",
                "rule",
                "rust-format",
                &candidate.fingerprint,
                "accepted"
            )
            .is_err()
        );
        let accepted = decide(
            root.path(),
            "fixture",
            "rule",
            "rust-format",
            &changed.candidates[0].fingerprint,
            "accepted",
        )
        .unwrap();
        assert_eq!(accepted.candidates[0].state, "selected");
        assert!(!accepted.first_accept_replaces_bundled_default);
        assert_eq!(
            sources::load(root.path()).unwrap().sources[0].selected_rules,
            ["rust-format"]
        );
        assert!(
            !root.path().join("AGENTS.md").exists(),
            "accepting must not apply"
        );
        assert_eq!(scan(root.path()).unwrap().candidates[0].state, "selected");
        sources::save(root.path(), &GuidanceConfig::default()).unwrap();
        assert_eq!(scan(root.path()).unwrap().changes[0].change, "withdrawn");
    }

    #[test]
    fn independent_dismissals_coalesce_and_a_skill_can_be_selected() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Cargo.toml"), "").unwrap();
        let mut fixture = source(vec![rule("rust-a", 1, "rust"), rule("rust-b", 1, "rust")]);
        fixture.skills.push(SourceSkill {
            name: "rust-check".into(),
            files: vec![SourceSkillFile {
                path: "SKILL.md".into(),
                sha256: format!("{:x}", Sha256::digest(b"skill")),
                base64: "c2tpbGw=".into(),
                known_base64: Vec::new(),
            }],
        });
        let mut historical_skill = fixture.skills[0].clone();
        historical_skill.files[0].known_base64.push("b2xk".into());
        assert_eq!(
            sources::skill_digest(&fixture.skills[0]),
            sources::skill_digest(&historical_skill),
            "old skill bodies do not change the current recommendation"
        );
        sources::save(
            root.path(),
            &GuidanceConfig {
                schema_version: 1,
                explicit_selection: false,
                sources: vec![fixture],
            },
        )
        .unwrap();
        let report = scan(root.path()).unwrap();
        assert_eq!(report.candidates.len(), 3);
        let first = report
            .candidates
            .iter()
            .find(|item| item.name == "rust-a")
            .unwrap()
            .clone();
        let second = report
            .candidates
            .iter()
            .find(|item| item.name == "rust-b")
            .unwrap()
            .clone();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                decide(
                    root.path(),
                    "fixture",
                    "rule",
                    "rust-a",
                    &first.fingerprint,
                    "dismissed",
                )
                .unwrap()
            });
            scope.spawn(|| {
                decide(
                    root.path(),
                    "fixture",
                    "rule",
                    "rust-b",
                    &second.fingerprint,
                    "dismissed",
                )
                .unwrap()
            });
        });
        let report = scan(root.path()).unwrap();
        assert_eq!(
            report
                .candidates
                .iter()
                .filter(|item| item.state == "dismissed")
                .count(),
            2
        );
        let skill = report
            .candidates
            .iter()
            .find(|item| item.kind == "skill")
            .unwrap();
        decide(
            root.path(),
            "fixture",
            "skill",
            "rust-check",
            &skill.fingerprint,
            "accepted",
        )
        .unwrap();
        assert_eq!(
            sources::load(root.path()).unwrap().sources[0].selected_skills,
            ["rust-check"]
        );
    }

    #[test]
    fn dependency_evidence_changes_recommendation_without_reading_other_files() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("package.json"),
            r#"{"dependencies":{"react":"1"}}"#,
        )
        .unwrap();
        sources::save(
            root.path(),
            &GuidanceConfig {
                schema_version: 1,
                explicit_selection: false,
                sources: vec![source(vec![rule("react-ui", 1, "react")])],
            },
        )
        .unwrap();
        let first = scan(root.path()).unwrap();
        assert_eq!(first.candidates[0].evidence, ["package.json: react"]);
        decide(
            root.path(),
            "fixture",
            "rule",
            "react-ui",
            &first.candidates[0].fingerprint,
            "dismissed",
        )
        .unwrap();
        fs::write(root.path().join("package.json"), r#"{"dependencies":{}}"#).unwrap();
        let withdrawn = scan(root.path()).unwrap();
        assert!(withdrawn.candidates.is_empty());
        assert_eq!(withdrawn.changes[0].change, "withdrawn");
        assert_ne!(withdrawn.fingerprint, first.fingerprint);
    }

    #[test]
    fn malformed_review_state_fails_without_replacing_project_decisions() {
        let root = tempfile::tempdir().unwrap();
        settings(root.path())
            .set(
                SETTINGS_KEY,
                serde_json::json!({"decisions": [{
                    "source": "fixture", "kind": "rule", "name": "rust-check",
                    "fingerprint": "old", "decision": "unknown"
                }]}),
                Scope::Shared,
            )
            .unwrap();
        assert!(scan(root.path()).is_err());
        assert!(
            settings(root.path())
                .get(SETTINGS_KEY, Scope::Shared)
                .unwrap()
                .is_some()
        );
    }

    #[cfg(unix)]
    #[test]
    fn linked_manifest_outside_checkout_is_not_scanned() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("Cargo.toml"), "private").unwrap();
        fs::write(
            outside.path().join("package.json"),
            r#"{"dependencies":{"react":"1"}}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("Cargo.toml"),
            root.path().join("Cargo.toml"),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("package.json"),
            root.path().join("package.json"),
        )
        .unwrap();
        let report = scan(root.path()).unwrap();
        assert!(report.signals.is_empty());
    }
}
