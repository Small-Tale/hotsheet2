//! Bounded, read-only context for an optional guidance interview.
//!
//! Only allowlisted local signal labels and pinned candidate identities leave this
//! module. Project files, rule bodies, descriptions, paths outside the recognized
//! manifests, environment values, and credentials are never read into the prompt.

use crate::guidance_recommendations::{self, RecommendationError};
use hotsheet_ticketing::{Scope, Settings};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const MAX_CANDIDATES: usize = 24;
const MAX_QUESTIONS: usize = 3;
const MAX_LABEL_CHARS: usize = 80;
const STATE_KEY: &str = "guidance_interview_v1";

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct InterviewCandidate {
    pub id: String,
    pub source: String,
    pub kind: String,
    pub name: String,
    pub version: u64,
    pub pinned_commit: String,
    pub selected: bool,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct InterviewContext {
    pub signals: Vec<String>,
    pub candidates: Vec<InterviewCandidate>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct InterviewQuestion {
    pub id: String,
    pub prompt: String,
    pub choices: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct InterviewPreview {
    pub fingerprint: String,
    pub context: InterviewContext,
    pub questions: Vec<InterviewQuestion>,
    /// Exact text to share with a chosen AI tool after explicit consent.
    pub outbound_prompt: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct InterviewAdvice {
    pub candidate_id: String,
    pub stance: String,
    pub explanation: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct InterviewAnswer {
    pub advice: Vec<InterviewAdvice>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct StoredInterview {
    fingerprint: String,
    answer: Option<InterviewAnswer>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct InterviewReview {
    pub preview: InterviewPreview,
    /// `not_started`, `awaiting_answer`, or `answered`. A changed scan resets the view.
    pub state: String,
    pub answer: Option<InterviewAnswer>,
}

fn settings(project: &Path) -> Settings {
    Settings::for_project(project)
}

fn bounded_label(value: &str) -> String {
    if value.chars().count() <= MAX_LABEL_CHARS {
        return value.to_owned();
    }
    let mut hash = Sha256::new();
    hash.update(value.as_bytes());
    let suffix = format!("{:x}", hash.finalize());
    format!(
        "{}…#{}",
        value.chars().take(MAX_LABEL_CHARS).collect::<String>(),
        &suffix[..8]
    )
}

fn read_state(project: &Path) -> Result<Option<StoredInterview>, RecommendationError> {
    settings(project)
        .get(STATE_KEY, Scope::Shared)?
        .map(|value| {
            serde_json::from_value(value)
                .map_err(|error| RecommendationError::Invalid(error.to_string()))
        })
        .transpose()
}

pub fn review(project: &Path) -> Result<InterviewReview, RecommendationError> {
    let preview = preview(project)?;
    let stored = read_state(project)?.filter(|state| {
        state.fingerprint == preview.fingerprint && preview.outbound_prompt.is_some()
    });
    Ok(InterviewReview {
        preview,
        state: match &stored {
            None => "not_started",
            Some(StoredInterview { answer: None, .. }) => "awaiting_answer",
            Some(_) => "answered",
        }
        .into(),
        answer: stored.and_then(|state| state.answer),
    })
}

/// Record the explicit sharing choice. The application never sends the prompt itself.
pub fn open(
    project: &Path,
    fingerprint: &str,
    consent: bool,
) -> Result<InterviewReview, RecommendationError> {
    if !consent {
        return Err(RecommendationError::Invalid(
            "explicit interview sharing consent required".into(),
        ));
    }
    let current = review(project)?;
    if current.preview.outbound_prompt.is_none() || current.preview.fingerprint != fingerprint {
        return Err(RecommendationError::Invalid(
            "interview context changed since preview".into(),
        ));
    }
    if current.state != "not_started" {
        return Ok(current);
    }
    write_state(
        project,
        StoredInterview {
            fingerprint: fingerprint.into(),
            answer: None,
        },
    )?;
    let result = review(project)?;
    if result.state != "awaiting_answer" || result.preview.fingerprint != fingerprint {
        return Err(RecommendationError::Invalid(
            "interview context changed since preview".into(),
        ));
    }
    Ok(result)
}

/// Store a bounded advisory answer. No guidance source, selection, or installed file changes.
pub fn submit(
    project: &Path,
    fingerprint: &str,
    answer: InterviewAnswer,
) -> Result<InterviewReview, RecommendationError> {
    let current = review(project)?;
    if current.preview.fingerprint != fingerprint || current.state == "not_started" {
        return Err(RecommendationError::Invalid(
            "interview is not open for this context".into(),
        ));
    }
    if answer.advice.is_empty() || answer.advice.len() > MAX_CANDIDATES {
        return Err(RecommendationError::Invalid(
            "interview advice count is out of bounds".into(),
        ));
    }
    let ids = current
        .preview
        .context
        .candidates
        .iter()
        .map(|candidate| candidate.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for item in &answer.advice {
        if !ids.contains(item.candidate_id.as_str())
            || !seen.insert(item.candidate_id.as_str())
            || !matches!(item.stance.as_str(), "consider" | "avoid" | "uncertain")
            || item.explanation.trim().is_empty()
            || item.explanation.chars().count() > 400
        {
            return Err(RecommendationError::Invalid(
                "invalid interview advice".into(),
            ));
        }
    }
    write_state(
        project,
        StoredInterview {
            fingerprint: fingerprint.into(),
            answer: Some(answer),
        },
    )?;
    let result = review(project)?;
    if result.state != "answered" || result.preview.fingerprint != fingerprint {
        return Err(RecommendationError::Invalid(
            "interview context changed since preview".into(),
        ));
    }
    Ok(result)
}

fn write_state(project: &Path, next: StoredInterview) -> Result<(), RecommendationError> {
    let settings = settings(project);
    for _ in 0..8 {
        if settings.get(STATE_KEY, Scope::Local)?.is_some() {
            return Err(RecommendationError::Invalid(
                "local interview state shadows project review".into(),
            ));
        }
        let previous = settings.get_effective(STATE_KEY)?;
        let stored: Option<StoredInterview> = previous
            .clone()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| RecommendationError::Invalid(error.to_string()))?;
        if let Some(stored) = stored {
            if stored.fingerprint == next.fingerprint {
                if stored == next {
                    return Ok(());
                }
                if next.answer.is_none() || stored.answer.is_some() {
                    return Err(RecommendationError::Invalid(
                        "interview changed concurrently; reload before continuing".into(),
                    ));
                }
            } else if next.answer.is_some() {
                return Err(RecommendationError::Invalid(
                    "interview context changed since preview".into(),
                ));
            }
        } else if next.answer.is_some() {
            return Err(RecommendationError::Invalid("interview is not open".into()));
        }
        let value = serde_json::to_value(&next)
            .map_err(|error| RecommendationError::Invalid(error.to_string()))?;
        if settings.set_if(STATE_KEY, value, Scope::Shared, |current| {
            current == previous.as_ref()
        })? {
            return Ok(());
        }
    }
    Err(RecommendationError::Invalid(
        "interview changed concurrently; retry".into(),
    ))
}

/// Produce the exact outbound text without contacting a provider or changing guidance.
pub fn preview(project: &Path) -> Result<InterviewPreview, RecommendationError> {
    let report = guidance_recommendations::scan(project)?;
    let signal_keys = report
        .signals
        .iter()
        .map(|signal| signal.key.as_str())
        .filter(|key| {
            matches!(
                *key,
                "rust"
                    | "javascript"
                    | "typescript"
                    | "python"
                    | "go"
                    | "swift"
                    | "ruby"
                    | "react"
                    | "vue"
                    | "svelte"
                    | "nextjs"
                    | "kerf"
            )
        })
        .collect::<BTreeSet<_>>();
    let candidates = report
        .candidates
        .iter()
        .take(MAX_CANDIDATES)
        .enumerate()
        .map(|(index, candidate)| InterviewCandidate {
            id: format!("candidate-{}", index + 1),
            source: bounded_label(&candidate.source),
            kind: candidate.kind.clone(),
            name: bounded_label(&candidate.name),
            version: candidate.version,
            pinned_commit: candidate.pinned_commit.clone(),
            selected: candidate.selected,
            // Evidence is from the scanner's fixed manifest/dependency allowlist.
            evidence: candidate.evidence.iter().take(4).cloned().collect(),
        })
        .collect::<Vec<_>>();
    let context = InterviewContext {
        signals: signal_keys.iter().map(|key| (*key).to_owned()).collect(),
        candidates,
    };
    let mut questions = Vec::new();
    let frameworks = ["react", "vue", "svelte", "nextjs", "kerf"]
        .into_iter()
        .filter(|name| signal_keys.contains(name))
        .collect::<Vec<_>>();
    if frameworks.len() > 1 {
        questions.push(InterviewQuestion {
            id: "active-framework".into(),
            prompt: "Which detected UI frameworks are actively used by this project?".into(),
            choices: frameworks.iter().map(|name| (*name).to_owned()).collect(),
        });
    }
    let mut by_evidence: BTreeMap<&str, Vec<&InterviewCandidate>> = BTreeMap::new();
    for candidate in &context.candidates {
        if candidate.selected {
            continue;
        }
        for evidence in &candidate.evidence {
            by_evidence.entry(evidence).or_default().push(candidate);
        }
    }
    if let Some((evidence, matching)) = by_evidence.into_iter().find(|(_, matching)| {
        matching.len() > 1
            && matching
                .iter()
                .map(|item| item.source.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                > 1
    }) {
        questions.push(InterviewQuestion {
            id: "overlapping-guidance".into(),
            prompt: format!(
                "Are the pinned guidance items matched by {evidence} meant to coexist?"
            ),
            choices: matching
                .iter()
                .take(6)
                .map(|item| item.id.clone())
                .collect(),
        });
    }
    if context.candidates.is_empty() {
        questions.clear();
    }
    questions.truncate(MAX_QUESTIONS);
    let outbound_prompt = if questions.is_empty() {
        None
    } else {
        let payload = serde_json::to_string_pretty(&serde_json::json!({
            "context": context,
            "questions": questions,
        }))
        .expect("serializing bounded interview context");
        Some(format!(
            "You are advising on pinned project guidance. Use only the JSON below. Ask at most three specific questions to resolve its ambiguities, then explain which listed candidates may help and why. Return final JSON only: {{\"advice\":[{{\"candidate_id\":\"candidate-1\",\"stance\":\"consider|avoid|uncertain\",\"explanation\":\"brief reason\"}}]}}. Use only listed candidate ids. Never suggest a new rule body, registration, selection, or application. Your answer is advisory only.\n\n{payload}"
        ))
    };
    let fingerprint = {
        let mut hash = Sha256::new();
        hash.update(report.fingerprint.as_bytes());
        hash.update(outbound_prompt.as_deref().unwrap_or("").as_bytes());
        format!("{:x}", hash.finalize())
    };
    Ok(InterviewPreview {
        fingerprint,
        context,
        questions,
        outbound_prompt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guidance_sources::{self, GuidanceConfig, GuidanceSource, SourceRule};
    use std::fs;

    fn source(id: &str, name: &str, tag: &str) -> GuidanceSource {
        let body = format!("private rule body for {name}");
        GuidanceSource {
            id: id.into(),
            repository: "https://private.invalid/secret-repository".into(),
            requested_ref: "main".into(),
            commit: "a".repeat(40),
            rules: vec![SourceRule {
                name: name.into(),
                version: 1,
                sha256: format!("{:x}", Sha256::digest(body.as_bytes())),
                description: "Sensitive description: password abc123".into(),
                owner: None,
                tags: vec![tag.into()],
                body,
                known_bodies: Vec::new(),
                minimum_hotsheet_version: None,
                historical_versions: Vec::new(),
                selected_sha256: None,
            }],
            skills: Vec::new(),
            selected_rules: Vec::new(),
            selected_skills: Vec::new(),
        }
    }

    #[test]
    fn no_ambiguity_needs_no_interview_or_outbound_text() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname='private'\n",
        )
        .unwrap();
        guidance_sources::save(
            root.path(),
            &GuidanceConfig {
                schema_version: 1,
                explicit_selection: false,
                sources: vec![source("first", "rust-format", "rust")],
            },
        )
        .unwrap();
        let result = preview(root.path()).unwrap();
        assert!(result.questions.is_empty());
        assert!(result.outbound_prompt.is_none());
        assert_eq!(result.context.signals, ["rust"]);
    }

    #[test]
    fn conflicting_frameworks_without_a_pinned_candidate_stay_local() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("package.json"),
            r#"{"dependencies":{"react":"1","vue":"1"}}"#,
        )
        .unwrap();
        let result = preview(root.path()).unwrap();
        assert!(result.questions.is_empty());
        assert!(result.outbound_prompt.is_none());
        assert!(result.context.candidates.is_empty());
    }

    #[test]
    fn ambiguity_produces_bounded_exact_context_without_raw_values() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("package.json"),
            r#"{"secret":"private-password","dependencies":{"react":"private-value","vue":"private-value"}}"#,
        )
        .unwrap();
        guidance_sources::save(
            root.path(),
            &GuidanceConfig {
                schema_version: 1,
                explicit_selection: false,
                sources: vec![
                    source("first", "react-review", "react"),
                    source("second", "vue-review", "vue"),
                ],
            },
        )
        .unwrap();
        let first = preview(root.path()).unwrap();
        let second = preview(root.path()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.questions[0].id, "active-framework");
        assert!(first.questions.len() <= MAX_QUESTIONS);
        let text = first.outbound_prompt.unwrap();
        assert!(text.contains("react-review"));
        assert!(text.contains("vue-review"));
        for private in [
            "private-password",
            "private-value",
            "secret-repository",
            "Sensitive description",
            "private rule body",
        ] {
            assert!(!text.contains(private), "outbound context leaked {private}");
        }
        assert!(!root.path().join(".hotsheet2/settings.json").exists());
        assert!(open(root.path(), &first.fingerprint, false).is_err());
        assert_eq!(review(root.path()).unwrap().state, "not_started");
        assert!(open(root.path(), "stale", true).is_err());
        let opened = open(root.path(), &first.fingerprint, true).unwrap();
        assert_eq!(opened.state, "awaiting_answer");
        assert_eq!(review(root.path()).unwrap().state, "awaiting_answer");
        assert!(
            submit(
                root.path(),
                &first.fingerprint,
                InterviewAnswer {
                    advice: vec![InterviewAdvice {
                        candidate_id: "other".into(),
                        stance: "consider".into(),
                        explanation: "Unknown candidate".into(),
                    }]
                },
            )
            .is_err()
        );
        assert!(
            submit(
                root.path(),
                &first.fingerprint,
                InterviewAnswer {
                    advice: vec![
                        InterviewAdvice {
                            candidate_id: "candidate-1".into(),
                            stance: "consider".into(),
                            explanation: "Maybe.".into()
                        },
                        InterviewAdvice {
                            candidate_id: "candidate-1".into(),
                            stance: "avoid".into(),
                            explanation: "Maybe not.".into()
                        },
                    ]
                }
            )
            .is_err()
        );
        let answered = submit(
            root.path(),
            &first.fingerprint,
            InterviewAnswer {
                advice: vec![InterviewAdvice {
                    candidate_id: "candidate-1".into(),
                    stance: "uncertain".into(),
                    explanation: "Ask which framework is used at runtime.".into(),
                }],
            },
        )
        .unwrap();
        assert_eq!(answered.state, "answered");
        assert_eq!(review(root.path()).unwrap().answer, answered.answer);
        assert!(
            submit(
                root.path(),
                &first.fingerprint,
                InterviewAnswer {
                    advice: vec![InterviewAdvice {
                        candidate_id: "candidate-1".into(),
                        stance: "consider".into(),
                        explanation: "Conflicting replay.".into()
                    },]
                }
            )
            .is_err()
        );
        assert!(
            guidance_sources::load(root.path())
                .unwrap()
                .sources
                .iter()
                .all(|source| source.selected_rules.is_empty())
        );
        assert!(!root.path().join("AGENTS.md").exists());
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname='changed'\n",
        )
        .unwrap();
        assert_eq!(review(root.path()).unwrap().state, "not_started");
        assert!(
            submit(
                root.path(),
                &first.fingerprint,
                InterviewAnswer { advice: vec![] }
            )
            .is_err()
        );
    }

    #[test]
    fn long_pinned_identifiers_cannot_expand_outbound_context_without_bound() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("package.json"),
            r#"{"dependencies":{"react":"1","vue":"1"}}"#,
        )
        .unwrap();
        guidance_sources::save(
            root.path(),
            &GuidanceConfig {
                schema_version: 1,
                explicit_selection: false,
                sources: vec![source(
                    "fixture",
                    &format!("react-{}", "a".repeat(10_000)),
                    "react",
                )],
            },
        )
        .unwrap();
        let result = preview(root.path()).unwrap();
        assert!(result.outbound_prompt.unwrap().len() < 4_000);
        assert!(result.context.candidates[0].name.contains('…'));
    }
}
