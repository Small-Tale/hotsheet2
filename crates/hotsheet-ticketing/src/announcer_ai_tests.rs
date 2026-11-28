//! HS2-G3HJZH: the AI importance and wording contract, with a scripted model.

use std::sync::Mutex;

use crate::announcer_milestones::{FollowUpFacts, MilestoneKind};

use super::*;

fn milestone(kind: MilestoneKind, key: &str) -> Milestone {
    Milestone {
        id: format!("s1:t-{key}:{}:{key}", kind.as_str()),
        kind,
        store: "s1".into(),
        ticket_id: format!("t-{key}"),
        slug: "HS2-QXVHPB".into(),
        title: "Code organization".into(),
        tool: "claude".into(),
        summary: Some("Several files are too long".into()),
        follow_ups: Vec::new(),
    }
}

/// A scripted model: records each prompt and answers from a queue.
#[derive(Default)]
struct Fake {
    asked: Mutex<Vec<(AiTask, String)>>,
    replies: Mutex<VecDeque<Result<String, String>>>,
}

impl Fake {
    fn answering(replies: Vec<Result<String, String>>) -> Self {
        Self {
            asked: Mutex::default(),
            replies: Mutex::new(replies.into()),
        }
    }

    fn asked(&self) -> Vec<(AiTask, String)> {
        self.asked.lock().unwrap().clone()
    }
}

impl MilestoneAi for Fake {
    fn ask(&self, task: AiTask, prompt: &str) -> Result<String, String> {
        self.asked.lock().unwrap().push((task, prompt.to_owned()));
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err("no scripted reply".into()))
    }
}

/// The CLI's `--output-format json` shape, with the structured output.
fn cli_reply(payload: Value) -> Result<String, String> {
    Ok(json!({
        "type": "result",
        "is_error": false,
        "result": payload.to_string(),
        "structured_output": payload,
    })
    .to_string())
}

#[test]
fn hs2_g3hjzh_prompts_carry_facts_only() {
    let mut m = milestone(MilestoneKind::Progress, "a");
    m.follow_ups.push(FollowUpFacts {
        slug: "HS2-123456".into(),
        title: "Pilot".into(),
    });
    let prompt = classify_prompt(std::slice::from_ref(&m));
    for fact in [
        "QXVHPB",
        "Code organization",
        "Several files are too long",
        "123456",
        "Pilot",
    ] {
        assert!(prompt.contains(fact), "{fact}");
    }
    assert!(
        !prompt.contains("HS2-QXVHPB"),
        "only the short slug is sent"
    );
    let prompt = rewrite_prompt(&[(m.clone(), "I made progress.".into())]);
    assert!(prompt.contains("I made progress."));
    assert!(prompt.contains("first person"));
    for task in [AiTask::Classify, AiTask::Rewrite] {
        let schema: Value = serde_json::from_str(task.schema()).unwrap();
        assert_eq!(schema["type"], "object", "{}", task.as_str());
        assert!(!task.system_prompt().is_empty());
    }
}

#[test]
fn hs2_g3hjzh_replies_parse_from_each_cli_shape() {
    let batch = [
        milestone(MilestoneKind::Progress, "a"),
        milestone(MilestoneKind::Started, "b"),
    ];
    let (a, b) = (batch[0].id.clone(), batch[1].id.clone());
    let payload = json!({"scores": [
        {"id": a, "score": 82.4}, {"id": b, "score": 140}, {"id": a, "score": 1},
        {"id": "invented", "score": 99}, {"id": 3, "score": 5}
    ]});
    let structured = cli_reply(payload.clone()).unwrap();
    let scores = parse_scores(&structured, &batch).unwrap();
    assert_eq!(scores.get(&a), Some(&82));
    assert_eq!(scores.get(&b), Some(&100));
    assert_eq!(scores.len(), 2);
    // Only `result` text, fenced in prose.
    let fenced = json!({"result": format!("Here:\n```json\n{payload}\n```")}).to_string();
    assert_eq!(parse_scores(&fenced, &batch).unwrap(), scores);
    // The bare object.
    assert_eq!(parse_scores(&payload.to_string(), &batch).unwrap(), scores);
    for bad in [
        "not json".to_owned(),
        json!({"is_error": true, "result": "{}"}).to_string(),
        json!({"result": "no object"}).to_string(),
        json!({"scores": []}).to_string(),
        json!({"other": 1}).to_string(),
        "x".repeat(MAX_REPLY_BYTES + 1),
    ] {
        assert!(parse_scores(&bad, &batch).is_err(), "{bad:.60}");
    }
}

#[test]
fn hs2_g3hjzh_rewritten_lines_must_stay_first_person_and_name_the_ticket() {
    let mut m = milestone(MilestoneKind::Progress, "a");
    assert_eq!(
        accept_line("  \"I think we should pilot it first (QXVHPB).\" ", &m).as_deref(),
        Some("I think we should pilot it first (QXVHPB).")
    );
    assert_eq!(
        accept_line("My **plan** for\nQXVHPB: pilot first.", &m).as_deref(),
        Some("My plan for QXVHPB: pilot first.")
    );
    assert!(accept_line("I’ve looked at QXVHPB.", &m).is_some());
    assert!(
        accept_line("Claude looked at QXVHPB.", &m).is_none(),
        "third person"
    );
    assert!(
        accept_line("I looked at the ticket.", &m).is_none(),
        "slug dropped"
    );
    assert!(accept_line("", &m).is_none());
    let long = format!("I {} QXVHPB", "really ".repeat(60));
    assert!(accept_line(&long, &m).is_none(), "too long");
    m.follow_ups.push(FollowUpFacts {
        slug: "HS2-123456".into(),
        title: "Pilot".into(),
    });
    assert!(
        accept_line("I filed a ticket for QXVHPB.", &m).is_none(),
        "follow-up dropped"
    );
    assert!(accept_line("I filed 123456 for QXVHPB.", &m).is_some());
    let batch = [(m.clone(), "template".to_owned())];
    let reply = cli_reply(json!({"lines": [
        {"id": m.id, "speech": "I filed 123456 for QXVHPB."},
        {"id": m.id, "speech": "I said it twice (QXVHPB 123456)."},
        {"id": "invented", "speech": "I QXVHPB 123456"}
    ]}))
    .unwrap();
    let lines = parse_lines(&reply, &batch).unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[&m.id], "I filed 123456 for QXVHPB.");
    assert!(parse_lines(&json!({"x": 1}).to_string(), &batch).is_err());
}

#[test]
fn hs2_g3hjzh_budget_caps_calls_and_backs_off_after_failure() {
    let start = Instant::now();
    let mut budget = AiBudget::new(2, Duration::from_secs(60));
    assert!(budget.allow(start));
    assert!(budget.allow(start));
    assert!(!budget.allow(start), "hourly cap");
    assert!(
        budget.allow(start + Duration::from_secs(3600)),
        "a call ages out"
    );
    budget.failed(start + Duration::from_secs(3600));
    assert!(!budget.allow(start + Duration::from_secs(3630)), "paused");
    assert!(
        budget.allow(start + Duration::from_secs(3661)),
        "pause ends"
    );
}

#[test]
fn hs2_g3hjzh_ai_scores_and_words_with_one_call_each_per_batch() {
    let thoughts = milestone(MilestoneKind::InitialThoughts, "a");
    let started = milestone(MilestoneKind::Started, "b");
    let fake = Fake::answering(vec![
        cli_reply(json!({"scores": [
            {"id": thoughts.id, "score": 30}, {"id": started.id, "score": 90}
        ]})),
        cli_reply(json!({"lines": [
            {"id": started.id, "speech": "I just picked up the code organization ticket (QXVHPB)."}
        ]})),
    ]);
    let mut decider = Decider::default();
    let now = Instant::now();
    let decided = decider.decide(
        vec![thoughts.clone(), started.clone()],
        AiSettings::default(),
        Some(&fake),
        now,
    );
    let asked = fake.asked();
    assert_eq!(
        asked.iter().map(|(task, _)| *task).collect::<Vec<_>>(),
        [AiTask::Classify, AiTask::Rewrite]
    );
    // Only the milestone that will be said is reworded.
    assert!(asked[1].1.contains(&started.id) && !asked[1].1.contains(&thoughts.id));
    let (_, low) = &decided[0];
    assert_eq!(
        (low.score, low.scored_by, low.worded_by),
        (30, "ai", "template")
    );
    assert!(!low.speaks());
    let (_, high) = &decided[1];
    assert_eq!(
        (high.score, high.scored_by, high.worded_by),
        (90, "ai", "ai")
    );
    assert_eq!(
        high.spoken,
        "I just picked up the code organization ticket (QXVHPB)."
    );
    // Cached by milestone id: the same write observed again asks nothing.
    let again = decider.decide(
        vec![thoughts, started],
        AiSettings::default(),
        Some(&fake),
        now,
    );
    assert_eq!(fake.asked().len(), 2);
    assert_eq!(again, decided);
}

#[test]
fn hs2_g3hjzh_settings_off_and_failures_fall_back_to_rules_and_templates() {
    let question = milestone(MilestoneKind::Question, "q");
    let template = spoken_line(&question);
    let rules = rule_score(&question);
    // Both switches off: the model is never asked.
    let fake = Fake::default();
    let off = AiSettings {
        ai_importance: false,
        ai_rewrite: false,
    };
    let decided =
        Decider::default().decide(vec![question.clone()], off, Some(&fake), Instant::now());
    assert!(fake.asked().is_empty());
    assert_eq!(decided[0].1.score, rules);
    assert_eq!(decided[0].1.spoken, template);
    // No model reachable.
    let decided = Decider::default().decide(
        vec![question.clone()],
        AiSettings::default(),
        None,
        Instant::now(),
    );
    assert_eq!(
        (decided[0].1.scored_by, decided[0].1.worded_by),
        ("rules", "template")
    );
    // Importance only: rules score, AI words.
    let fake = Fake::answering(vec![cli_reply(json!({"lines": [
        {"id": question.id, "speech": "Quick question on QXVHPB: I need your call."}
    ]}))]);
    let only_rewrite = AiSettings {
        ai_importance: false,
        ai_rewrite: true,
    };
    let decided = Decider::default().decide(
        vec![question.clone()],
        only_rewrite,
        Some(&fake),
        Instant::now(),
    );
    assert_eq!(
        (decided[0].1.scored_by, decided[0].1.worded_by),
        ("rules", "ai")
    );
    // A failed call falls back and pauses AI: the next batch does not ask.
    let fake = Fake::answering(vec![Err("timed out".into())]);
    let mut decider = Decider::default();
    let now = Instant::now();
    let decided = decider.decide(
        vec![question.clone()],
        AiSettings::default(),
        Some(&fake),
        now,
    );
    assert_eq!(
        fake.asked().len(),
        1,
        "the failed classify stops the rewrite too"
    );
    assert_eq!(
        (decided[0].1.scored_by, decided[0].1.worded_by),
        ("rules", "template")
    );
    let other = milestone(MilestoneKind::Blocked, "z");
    decider.decide(vec![other], AiSettings::default(), Some(&fake), now);
    assert_eq!(fake.asked().len(), 1);
    // An unusable rewrite keeps the template.
    let fake = Fake::answering(vec![
        cli_reply(json!({"scores": [{"id": question.id, "score": 99}]})),
        cli_reply(json!({"lines": [{"id": question.id, "speech": "Claude has a question."}]})),
    ]);
    let decided =
        Decider::default().decide(vec![question], AiSettings::default(), Some(&fake), now);
    assert_eq!(
        (decided[0].1.scored_by, decided[0].1.worded_by),
        ("ai", "template")
    );
    assert_eq!(decided[0].1.spoken, template);
}

#[test]
fn hs2_g3hjzh_batches_are_chunked_and_bounded() {
    let batch: Vec<Milestone> = (0..(MAX_BATCH + 2))
        .map(|n| milestone(MilestoneKind::Progress, &format!("{n:02}")))
        .collect();
    let scores = |chunk: &[Milestone]| {
        cli_reply(
            json!({"scores": chunk.iter().map(|m| json!({"id": m.id, "score": 10})).collect::<Vec<_>>()}),
        )
    };
    let fake = Fake::answering(vec![
        scores(&batch[..MAX_BATCH]),
        scores(&batch[MAX_BATCH..]),
    ]);
    let settings = AiSettings {
        ai_importance: true,
        ai_rewrite: false,
    };
    let decided = Decider::default().decide(batch.clone(), settings, Some(&fake), Instant::now());
    assert_eq!(fake.asked().len(), 2, "one call per chunk of {MAX_BATCH}");
    assert!(
        decided
            .iter()
            .all(|(_, d)| d.scored_by == "ai" && d.score == 10)
    );
    // A budget of one call scores the first chunk and leaves the rest to rules.
    let fake = Fake::answering(vec![scores(&batch[..MAX_BATCH])]);
    let mut decider = Decider::new(AiBudget::new(1, FAILURE_BACKOFF));
    let decided = decider.decide(batch, settings, Some(&fake), Instant::now());
    assert_eq!(fake.asked().len(), 1);
    assert_eq!(
        decided.iter().filter(|(_, d)| d.scored_by == "ai").count(),
        MAX_BATCH
    );
    assert_eq!(
        decided
            .iter()
            .filter(|(_, d)| d.scored_by == "rules")
            .count(),
        2
    );
}

#[test]
fn hs2_g3hjzh_the_cache_forgets_the_oldest_ids() {
    let mut cache = DecisionCache::default();
    for n in 0..=CACHE_LIMIT {
        cache.set_score(&format!("id-{n}"), 1);
    }
    cache.set_line("id-last", "I did it.".into());
    assert_eq!(cache.score("id-0"), None);
    assert_eq!(cache.score("id-1"), None);
    assert_eq!(cache.score(&format!("id-{CACHE_LIMIT}")), Some(1));
    assert_eq!(cache.line("id-last"), Some("I did it."));
}
