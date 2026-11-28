//! AI importance and wording for announcer milestones (HS2-G3HJZH, `docs/15` §15.6.1).
//!
//! The maintainer's answer on HS2-G3HJZH asked for templates over structured ticket data as the
//! source, an LLM rewriting each milestone into conversational speech, and "some kind of AI
//! importance / interestingness criteria … maybe using some kind of lighter AI classifier". This
//! module is that contract, independent of how a model is reached:
//!
//! - **Classify:** one prompt per batch (at most [`MAX_BATCH`] milestones) asks for a 0–100 score
//!   per milestone id; a milestone at or above [`SPEAK_THRESHOLD`] is said.
//! - **Rewrite:** one prompt per batch of the milestones that will be said asks for one first-person
//!   line each; a line is kept only if [`accept_line`] passes it, else the template stands.
//! - **Facts only:** a prompt carries each milestone's kind, short slug, title, note summary, and
//!   follow-up slugs and titles. Never a note body, ticket details, paths, or commands.
//! - **Bounded:** [`AiBudget`] caps calls per hour and pauses after a failure; [`DecisionCache`]
//!   remembers each milestone id's score and line, so a re-observed write never asks again. Any
//!   failure, timeout, unusable reply, budget refusal, or missing tool falls back to
//!   [`rule_score`] and the template line, per milestone.
//!
//! The model is reached through the injectable [`MilestoneAi`] trait; the server's live adapter
//! runs the person's signed-in AI CLI, and tests pass a scripted fake.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::announcer_milestones::{
    Decision, Milestone, SPEAK_THRESHOLD, coalesce, rule_score, short_slug, spoken_line,
};

/// The smallest Claude model, used through the signed-in `claude` CLI.
pub const CLASSIFIER_MODEL: &str = "claude-haiku-5-5";
/// Milestones described in one prompt at most.
pub const MAX_BATCH: usize = 8;
/// The longest spoken line accepted from a rewrite, in characters.
pub const MAX_SPOKEN_CHARS: usize = 320;
/// The largest reply read, in bytes.
pub const MAX_REPLY_BYTES: usize = 256 * 1024;
/// AI calls allowed per rolling hour; beyond it, rules and templates stand in.
pub const MAX_CALLS_PER_HOUR: usize = 30;
/// After a failed call, AI is not asked again for this long.
pub const FAILURE_BACKOFF: Duration = Duration::from_secs(10 * 60);
/// Milestone ids whose decisions are remembered.
pub const CACHE_LIMIT: usize = 512;

/// Which question a prompt asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiTask {
    Classify,
    Rewrite,
}

impl AiTask {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Classify => "classify",
            Self::Rewrite => "rewrite",
        }
    }

    /// The JSON Schema the reply must match (passed to the CLI as structured output).
    pub fn schema(self) -> &'static str {
        match self {
            Self::Classify => {
                r#"{"type":"object","properties":{"scores":{"type":"array","items":{"type":"object","properties":{"id":{"type":"string"},"score":{"type":"integer","minimum":0,"maximum":100}},"required":["id","score"]}}},"required":["scores"]}"#
            }
            Self::Rewrite => {
                r#"{"type":"object","properties":{"lines":{"type":"array","items":{"type":"object","properties":{"id":{"type":"string"},"speech":{"type":"string"}},"required":["id","speech"]}}},"required":["lines"]}"#
            }
        }
    }

    /// A short system prompt, so the call carries no agent instructions or tools.
    pub fn system_prompt(self) -> &'static str {
        match self {
            Self::Classify => {
                "You score work milestones for a spoken announcer. Reply only with the requested JSON."
            }
            Self::Rewrite => {
                "You write short spoken status lines for an AI agent's own work. Reply only with the requested JSON."
            }
        }
    }
}

/// Reaches a model: returns the raw reply text (the CLI's JSON output) or why it could not.
pub trait MilestoneAi: Send + Sync {
    fn ask(&self, task: AiTask, prompt: &str) -> Result<String, String>;
}

/// The machine-wide switches (`announcer.ai_importance`, `announcer.ai_rewrite`); both on by
/// default. Off, the rule score and the template line are used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiSettings {
    pub ai_importance: bool,
    pub ai_rewrite: bool,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            ai_importance: true,
            ai_rewrite: true,
        }
    }
}

/// The facts a prompt may carry about one milestone.
fn facts(m: &Milestone) -> Value {
    let mut value = json!({
        "id": m.id,
        "kind": m.kind.as_str(),
        "ticket": short_slug(&m.slug),
        "title": m.title,
    });
    if let Some(summary) = &m.summary {
        value["summary"] = json!(summary);
    }
    if !m.follow_ups.is_empty() {
        value["follow_ups"] = m
            .follow_ups
            .iter()
            .map(|f| json!({ "ticket": short_slug(&f.slug), "title": f.title }))
            .collect();
    }
    value
}

/// The classification prompt for one batch.
pub fn classify_prompt(batch: &[Milestone]) -> String {
    let items: Vec<Value> = batch.iter().map(facts).collect();
    format!(
        "A person listens to an announcer while AI agents work on their tickets. Score how worth \
         saying aloud each milestone is, from 0 to 100.\n\
         - 70-100: a question the person must answer, a block, a completion, a failure (tests, \
         checks, CI, builds), initial thoughts on a newly started ticket, a decision that created \
         follow-up tickets.\n\
         - 40-69: substantive progress or a result worth knowing.\n\
         - 0-39: routine bookkeeping, a bare start, a phase change with nothing new.\n\
         Return every id exactly once.\n\nMilestones:\n{}",
        serde_json::to_string_pretty(&items).unwrap_or_default()
    )
}

/// The rewrite prompt for one batch of milestones that will be said, each with its template.
pub fn rewrite_prompt(batch: &[(Milestone, String)]) -> String {
    let items: Vec<Value> = batch
        .iter()
        .map(|(m, template)| {
            let mut value = facts(m);
            value["template"] = json!(template);
            value
        })
        .collect();
    format!(
        "Rewrite each milestone as one or two short conversational sentences that the AI agent \
         says aloud about its own work, in the first person (\"I\", \"my\"). Example: \"I started \
         investigating a ticket focusing on code organization and documentation (QXVHPB). My \
         initial thoughts are that several files are over our 1000-line limit. Deeper evaluation \
         is ongoing.\" Keep each ticket's short id in parentheses exactly as given. Use only the \
         facts given and invent nothing. No Markdown, no project name, no surrounding quotes. \
         Return every id exactly once.\n\nMilestones:\n{}",
        serde_json::to_string_pretty(&items).unwrap_or_default()
    )
}

/// The JSON object a reply carries: the CLI's `structured_output`, else its `result` text parsed
/// as JSON (fences and prose around the object tolerated), else the reply itself.
pub fn reply_payload(reply: &str) -> Result<Value, String> {
    if reply.len() > MAX_REPLY_BYTES {
        return Err("reply too large".into());
    }
    let outer = parse_object(reply).ok_or("reply is not JSON")?;
    if outer.get("is_error").and_then(Value::as_bool) == Some(true) {
        return Err("the AI tool reported an error".into());
    }
    if let Some(structured) = outer.get("structured_output").filter(|v| v.is_object()) {
        return Ok(structured.clone());
    }
    if let Some(result) = outer.get("result").and_then(Value::as_str) {
        return parse_object(result).ok_or_else(|| "result is not JSON".into());
    }
    Ok(outer)
}

fn parse_object(text: &str) -> Option<Value> {
    let text = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        return value.is_object().then_some(value);
    }
    let (start, end) = (text.find('{')?, text.rfind('}')?);
    (start < end)
        .then(|| serde_json::from_str::<Value>(&text[start..=end]).ok())
        .flatten()
        .filter(Value::is_object)
}

/// Scores for the batch's ids; ids the reply leaves out, repeats, or invents are ignored. A reply
/// that scores none of the batch is unusable.
pub fn parse_scores(reply: &str, batch: &[Milestone]) -> Result<HashMap<String, u8>, String> {
    let payload = reply_payload(reply)?;
    let wanted: HashSet<&str> = batch.iter().map(|m| m.id.as_str()).collect();
    let mut scores = HashMap::new();
    for item in payload
        .get("scores")
        .and_then(Value::as_array)
        .ok_or("reply has no scores")?
    {
        let (Some(id), Some(score)) = (
            item.get("id").and_then(Value::as_str),
            item.get("score").and_then(Value::as_f64),
        ) else {
            continue;
        };
        if wanted.contains(id) && score.is_finite() && !scores.contains_key(id) {
            let score = score.round().clamp(0.0, 100.0) as u8;
            scores.insert(id.to_owned(), score);
        }
    }
    if scores.is_empty() {
        return Err("reply scored none of the milestones".into());
    }
    Ok(scores)
}

/// Accepted lines for the batch's ids (see [`accept_line`]).
pub fn parse_lines(
    reply: &str,
    batch: &[(Milestone, String)],
) -> Result<HashMap<String, String>, String> {
    let payload = reply_payload(reply)?;
    let mut lines = HashMap::new();
    for item in payload
        .get("lines")
        .and_then(Value::as_array)
        .ok_or("reply has no lines")?
    {
        let (Some(id), Some(speech)) = (
            item.get("id").and_then(Value::as_str),
            item.get("speech").and_then(Value::as_str),
        ) else {
            continue;
        };
        let Some((m, _)) = batch.iter().find(|(m, _)| m.id == id) else {
            continue;
        };
        if lines.contains_key(id) {
            continue;
        }
        if let Some(line) = accept_line(speech, m) {
            lines.insert(id.to_owned(), line);
        }
    }
    Ok(lines)
}

/// A rewritten line is kept only when it is plain, bounded, in the first person, and still names
/// the ticket (and each follow-up) by its short slug. Markdown marks and line breaks are flattened.
pub fn accept_line(speech: &str, m: &Milestone) -> Option<String> {
    let flat: String = speech
        .chars()
        .filter(|c| !matches!(c, '*' | '`' | '#' | '_'))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let line = flat
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(['"', '\u{201c}', '\u{201d}', ' '])
        .to_owned();
    if line.is_empty() || line.chars().count() > MAX_SPOKEN_CHARS {
        return None;
    }
    let first_person = line
        .split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '\u{2019}')
        .any(|word| {
            word == "I"
                || word.starts_with("I'")
                || word.starts_with("I\u{2019}")
                || word == "My"
                || word == "my"
        });
    let names_tickets = std::iter::once(m.slug.as_str())
        .chain(m.follow_ups.iter().map(|f| f.slug.as_str()))
        .all(|slug| line.contains(short_slug(slug)));
    (first_person && names_tickets).then_some(line)
}

/// Caps AI calls per rolling hour and pauses them after a failure.
#[derive(Debug)]
pub struct AiBudget {
    max_per_hour: usize,
    backoff: Duration,
    calls: VecDeque<Instant>,
    paused_until: Option<Instant>,
}

impl Default for AiBudget {
    fn default() -> Self {
        Self::new(MAX_CALLS_PER_HOUR, FAILURE_BACKOFF)
    }
}

impl AiBudget {
    pub fn new(max_per_hour: usize, backoff: Duration) -> Self {
        Self {
            max_per_hour,
            backoff,
            calls: VecDeque::new(),
            paused_until: None,
        }
    }

    /// Whether a call may start now; an allowed call is counted.
    pub fn allow(&mut self, now: Instant) -> bool {
        if self.paused_until.is_some_and(|until| now < until) {
            return false;
        }
        let hour = Duration::from_secs(3600);
        while self
            .calls
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= hour)
        {
            self.calls.pop_front();
        }
        if self.calls.len() >= self.max_per_hour {
            return false;
        }
        self.calls.push_back(now);
        true
    }

    /// A call failed: no AI until the backoff ends.
    pub fn failed(&mut self, now: Instant) {
        self.paused_until = Some(now + self.backoff);
    }
}

/// Remembered scores and lines by milestone id, least recently inserted forgotten first.
#[derive(Debug, Default)]
pub struct DecisionCache {
    order: VecDeque<String>,
    scores: HashMap<String, u8>,
    lines: HashMap<String, String>,
}

impl DecisionCache {
    fn remember(&mut self, id: &str) {
        if !self.order.iter().any(|known| known == id) {
            self.order.push_back(id.to_owned());
        }
        while self.order.len() > CACHE_LIMIT {
            if let Some(old) = self.order.pop_front() {
                self.scores.remove(&old);
                self.lines.remove(&old);
            }
        }
    }

    pub fn score(&self, id: &str) -> Option<u8> {
        self.scores.get(id).copied()
    }

    pub fn line(&self, id: &str) -> Option<&str> {
        self.lines.get(id).map(String::as_str)
    }

    fn set_score(&mut self, id: &str, score: u8) {
        self.scores.insert(id.to_owned(), score);
        self.remember(id);
    }

    fn set_line(&mut self, id: &str, line: String) {
        self.lines.insert(id.to_owned(), line);
        self.remember(id);
    }
}

/// Decides each batch: coalesce, score, and word. Owned by one worker, so at most one AI call is
/// in flight.
#[derive(Debug, Default)]
pub struct Decider {
    pub budget: AiBudget,
    pub cache: DecisionCache,
}

impl Decider {
    pub fn new(budget: AiBudget) -> Self {
        Self {
            budget,
            cache: DecisionCache::default(),
        }
    }

    /// Coalesce a batch and decide every milestone in it. `ai` is `None` when no model can be
    /// reached (no tool, the Announcer feature off, or a test process).
    pub fn decide(
        &mut self,
        batch: Vec<Milestone>,
        settings: AiSettings,
        ai: Option<&dyn MilestoneAi>,
        now: Instant,
    ) -> Vec<(Milestone, Decision)> {
        let batch = coalesce(batch);
        let mut scored: HashMap<String, u8> = HashMap::new();
        if let (true, Some(ai)) = (settings.ai_importance, ai) {
            let unknown: Vec<Milestone> = batch
                .iter()
                .filter(|m| self.cache.score(&m.id).is_none())
                .cloned()
                .collect();
            for chunk in unknown.chunks(MAX_BATCH) {
                if !self.budget.allow(now) {
                    break;
                }
                let reply = ai.ask(AiTask::Classify, &classify_prompt(chunk));
                match reply.and_then(|reply| parse_scores(&reply, chunk)) {
                    Ok(scores) => {
                        for (id, score) in scores {
                            self.cache.set_score(&id, score);
                        }
                    }
                    Err(_) => {
                        self.budget.failed(now);
                        break;
                    }
                }
            }
            for m in &batch {
                if let Some(score) = self.cache.score(&m.id) {
                    scored.insert(m.id.clone(), score);
                }
            }
        }
        let mut decided: Vec<(Milestone, Decision)> = batch
            .into_iter()
            .map(|m| {
                let (score, scored_by) = match scored.get(&m.id) {
                    Some(score) => (*score, "ai"),
                    None => (rule_score(&m), "rules"),
                };
                let decision = Decision {
                    score,
                    scored_by,
                    spoken: spoken_line(&m),
                    worded_by: "template",
                };
                (m, decision)
            })
            .collect();
        if let (true, Some(ai)) = (settings.ai_rewrite, ai) {
            self.reword(&mut decided, ai, now);
        }
        decided
    }

    fn reword(
        &mut self,
        decided: &mut [(Milestone, Decision)],
        ai: &dyn MilestoneAi,
        now: Instant,
    ) {
        let wanted: Vec<(Milestone, String)> = decided
            .iter()
            .filter(|(m, d)| d.score >= SPEAK_THRESHOLD && self.cache.line(&m.id).is_none())
            .map(|(m, d)| (m.clone(), d.spoken.clone()))
            .collect();
        for chunk in wanted.chunks(MAX_BATCH) {
            if !self.budget.allow(now) {
                break;
            }
            match ai
                .ask(AiTask::Rewrite, &rewrite_prompt(chunk))
                .and_then(|reply| parse_lines(&reply, chunk))
            {
                Ok(lines) => {
                    for (id, line) in lines {
                        self.cache.set_line(&id, line);
                    }
                }
                Err(_) => {
                    self.budget.failed(now);
                    break;
                }
            }
        }
        for (m, decision) in decided.iter_mut() {
            if decision.score < SPEAK_THRESHOLD {
                continue;
            }
            if let Some(line) = self.cache.line(&m.id) {
                decision.spoken = line.to_owned();
                decision.worded_by = "ai";
            }
        }
    }
}

#[cfg(test)]
#[path = "announcer_ai_tests.rs"]
mod tests;
