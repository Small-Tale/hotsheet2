//! Ticket milestones for the Announcer (HS2-G3HJZH, `docs/15` §15.6.1).
//!
//! The activity stream carries what a tool did, step by step; what a person wants to hear is what
//! the work reached: "I started investigating a ticket focusing on … (QXVHPB). My initial thoughts
//! …", a question for them, a follow-up created, a completion. Those live in ticket writes, not in
//! tool signals. This module turns one changed ticket, compared with what was seen of it before,
//! into milestone candidates; coalesces a batch of them so one write is said once; scores each by a
//! deterministic rule; and renders the shown (structured, Markdown) and spoken (first person,
//! plain) template lines. Everything here is pure: the server owns the snapshots, the clock, the
//! batching, and the optional AI classifier and wording (`crate::announcer_ai`).
//!
//! Only AI work is narrated, since the agent speaks for itself ("I …"): a note counts when its
//! author role is `ai`; a status, phase, or blocked change counts when the newest new note (the
//! transition's own activity note) is AI-authored, or, with no authored note, while a worker holds
//! the claim. Human and system writes never become first-person milestones.

use std::collections::BTreeSet;

use hotsheet_model::{
    AttachmentActorRole, Note, NoteKind, ProjectTicketRef, StartedPhase, Status, Ticket,
};
use serde::Serialize;
use serde_json::{Value, json};
use time::OffsetDateTime;

use crate::activity::{ActivityEvent, ActivityKind, Importance};

/// A milestone scored at or above this is said aloud; below it is shown but not said.
pub const SPEAK_THRESHOLD: u8 = 50;
/// The longest note summary or reason carried into a line, in characters.
pub const MAX_FACT_CHARS: usize = 200;
/// The most follow-ups one line names; the rest are counted.
const MAX_NAMED_FOLLOW_UPS: usize = 3;

/// What a ticket reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MilestoneKind {
    /// Claimed or moved to Started.
    Started,
    /// A `## Preliminary thoughts` note.
    InitialThoughts,
    /// Any other AI regular note, by its summary.
    Progress,
    /// A follow-up ticket created from this one.
    FollowUp,
    /// A `feedback_needed` note: a question for the person.
    Question,
    /// The ticket became blocked.
    Blocked,
    /// Moved to the `initial_testing` phase.
    Testing,
    /// Moved to the `final_testing` phase: pushed, waiting on CI.
    AwaitingCi,
    /// Completed (or verified straight from an open state).
    Completed,
}

impl MilestoneKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::InitialThoughts => "initial_thoughts",
            Self::Progress => "progress",
            Self::FollowUp => "follow_up",
            Self::Question => "question",
            Self::Blocked => "blocked",
            Self::Testing => "testing",
            Self::AwaitingCi => "awaiting_ci",
            Self::Completed => "completed",
        }
    }

    /// The rule score before content adjustments (see [`rule_score`]).
    fn base_score(self) -> u8 {
        match self {
            Self::Question => 95,
            Self::Blocked => 90,
            Self::Completed => 85,
            Self::InitialThoughts => 75,
            Self::FollowUp => 70,
            Self::Progress => 55,
            Self::AwaitingCi => 45,
            Self::Started => 40,
            Self::Testing => 30,
        }
    }

    /// The closed activity kind the milestone is recorded as (`docs/15` §15.2).
    fn activity_kind(self) -> ActivityKind {
        match self {
            Self::Started | Self::Testing | Self::AwaitingCi | Self::Completed => {
                ActivityKind::TicketStatus
            }
            Self::InitialThoughts => ActivityKind::Plan,
            Self::Progress | Self::FollowUp => ActivityKind::Decision,
            // A question waits on the person, as a block does.
            Self::Question | Self::Blocked => ActivityKind::Blocked,
        }
    }
}

/// A follow-up ticket a milestone names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FollowUpFacts {
    pub slug: String,
    pub title: String,
}

/// One milestone candidate: structured facts only, never a note body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Milestone {
    /// Deterministic: the same write observed twice yields the same id (dedupe and AI cache key).
    pub id: String,
    pub kind: MilestoneKind,
    /// The store the ticket lives in.
    pub store: String,
    pub ticket_id: String,
    pub slug: String,
    pub title: String,
    /// The AI tool that did the work (`claude`, `codex`, …, or `agent`).
    pub tool: String,
    /// A note's one-line summary, or a blocked reason.
    pub summary: Option<String>,
    pub follow_ups: Vec<FollowUpFacts>,
}

/// The parts of a ticket a transition compares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Baseline {
    pub status: Status,
    pub phase: Option<StartedPhase>,
    pub blocked: bool,
}

impl Baseline {
    pub fn of(ticket: &Ticket) -> Self {
        Self {
            status: ticket.status,
            phase: ticket.started_phase,
            blocked: is_blocked(ticket),
        }
    }
}

/// What was seen of a ticket. Each part is `None` when unknown (a ticket first sighted after the
/// store was seeded): then nothing compares against it, and notes and follow-ups count as new only
/// when created after the seed time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MilestoneSnapshot {
    pub baseline: Option<Baseline>,
    pub notes: Option<BTreeSet<String>>,
    pub follow_ups: Option<BTreeSet<String>>,
}

impl MilestoneSnapshot {
    /// Everything about a ticket as it is now.
    pub fn of(ticket: &Ticket) -> Self {
        Self {
            baseline: Some(Baseline::of(ticket)),
            notes: Some(
                ticket
                    .notes
                    .iter()
                    .map(|note| note.id.to_string())
                    .collect(),
            ),
            follow_ups: Some(
                ticket
                    .follow_ups
                    .iter()
                    .map(ProjectTicketRef::qualified)
                    .collect(),
            ),
        }
    }

    /// Only the transition baseline (seeded from an index row, which has no notes).
    pub fn from_baseline(baseline: Baseline) -> Self {
        Self {
            baseline: Some(baseline),
            notes: None,
            follow_ups: None,
        }
    }

    /// A first observation can see the newest file before its queued writes are consumed.
    /// Recover its prior status only from a generated transition since subscription.
    pub fn recover_initial_status(
        previous: Option<&Self>,
        ticket: &Ticket,
        subscribed_at: OffsetDateTime,
    ) -> Option<Self> {
        if previous.is_some_and(|snapshot| {
            snapshot.notes.is_some()
                || snapshot
                    .baseline
                    .as_ref()
                    .is_some_and(|baseline| baseline.status != ticket.status)
        }) {
            return None;
        }
        let (from, to) = ticket
            .notes
            .iter()
            .rev()
            .filter(|note| !note.kind_explicit && note_is_new(note, None, subscribed_at))
            .find_map(crate::ops::recorded_status_transition)?;
        if to != ticket.status || from == to {
            return None;
        }
        let mut baseline = previous
            .and_then(|snapshot| snapshot.baseline.clone())
            .unwrap_or_else(|| Baseline::of(ticket));
        baseline.status = from;
        Some(Self::from_baseline(baseline))
    }
}

/// A follow-up target as the server resolved it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowUpTarget {
    pub slug: String,
    pub title: String,
    pub created_at: Option<OffsetDateTime>,
    /// The target's creator role was `ai`; `None` when unrecorded.
    pub created_by_ai: Option<bool>,
}

fn is_blocked(ticket: &Ticket) -> bool {
    ticket
        .blocked_reason
        .as_deref()
        .is_some_and(|reason| !reason.trim().is_empty())
        || !ticket.blocked_by.is_empty()
}

/// The tool a worker or actor id names: `claude-4b58…` is `claude`. Unknown ids are `agent`.
pub fn tool_of(actor_id: &str) -> String {
    const TOOLS: &[&str] = &[
        "claude",
        "codex",
        "gemini",
        "antigravity",
        "agy",
        "opencode",
        "goose",
        "cursor",
        "copilot",
        "aider",
        "amp",
    ];
    let lead = actor_id
        .trim()
        .split(['-', ':', '@', '/', ' '])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if TOOLS.contains(&lead.as_str()) {
        lead
    } else {
        "agent".into()
    }
}

/// `HS2-QXVHPB` is said as `QXVHPB`, as in the maintainer's examples.
pub fn short_slug(slug: &str) -> &str {
    slug.rsplit_once('-')
        .map(|(_, rest)| rest)
        .filter(|rest| !rest.is_empty())
        .unwrap_or(slug)
}

fn note_is_new(note: &Note, seen: Option<&BTreeSet<String>>, since: OffsetDateTime) -> bool {
    match seen {
        Some(seen) => !seen.contains(&note.id.to_string()),
        None => note.created_at.instant().is_some_and(|at| at >= since),
    }
}

fn is_ai(note: &Note) -> bool {
    note.actor
        .as_ref()
        .is_some_and(|actor| actor.role == AttachmentActorRole::Ai)
}

/// The tool behind this write's transitions: the newest new authored note decides (a person's
/// status change writes a human-authored activity note); with none, a live claim's worker.
fn transition_tool(ticket: &Ticket, new_notes: &[&Note]) -> Option<String> {
    let newest = new_notes.iter().rev().find_map(|note| note.actor.as_ref());
    match newest {
        Some(actor) if actor.role == AttachmentActorRole::Ai => {
            Some(tool_of(actor.id.as_deref().unwrap_or_default()))
        }
        Some(_) => None,
        None => ticket.claimed_by.as_deref().map(tool_of),
    }
}

/// A note's summary, else its first line of prose, flattened and bounded.
fn note_fact(note: &Note) -> Option<String> {
    let summary = note
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let text = summary.map(str::to_owned).or_else(|| {
        note.text
            .lines()
            .map(|line| line.trim().trim_start_matches('#').trim())
            .find(|line| {
                !line.is_empty()
                    && !line.eq_ignore_ascii_case("preliminary thoughts")
                    && !line.eq_ignore_ascii_case("feedback needed")
            })
            .map(str::to_owned)
    })?;
    let plain = plain_fact(&text);
    (!plain.is_empty()).then_some(plain)
}

fn is_preliminary(note: &Note) -> bool {
    let first = note.text.trim_start().lines().next().unwrap_or_default();
    let heading = first.trim_start_matches('#').trim().to_ascii_lowercase();
    first.starts_with('#') && heading.starts_with("preliminary thoughts")
}

/// The milestones one ticket write produced, in note order then transition order.
pub fn ticket_milestones(
    store: &str,
    previous: Option<&MilestoneSnapshot>,
    ticket: &Ticket,
    since: OffsetDateTime,
    resolve: impl Fn(&ProjectTicketRef) -> Option<FollowUpTarget>,
) -> Vec<Milestone> {
    let seen_notes = previous.and_then(|snapshot| snapshot.notes.as_ref());
    let new_notes: Vec<&Note> = ticket
        .notes
        .iter()
        .filter(|note| note_is_new(note, seen_notes, since))
        .collect();
    let ticket_id = ticket.id.to_string();
    let milestone =
        |kind: MilestoneKind, key: &str, tool: String, summary: Option<String>| Milestone {
            id: format!("{store}:{ticket_id}:{}:{key}", kind.as_str()),
            kind,
            store: store.to_owned(),
            ticket_id: ticket_id.clone(),
            slug: ticket.slug.clone(),
            title: plain_fact(&ticket.title),
            tool,
            summary,
            follow_ups: Vec::new(),
        };
    let mut out = Vec::new();
    for note in &new_notes {
        if !is_ai(note) {
            continue;
        }
        let tool = tool_of(
            note.actor
                .as_ref()
                .and_then(|actor| actor.id.as_deref())
                .unwrap_or_default(),
        );
        let key = note.id.to_string();
        match note.kind {
            NoteKind::FeedbackNeeded => {
                out.push(milestone(
                    MilestoneKind::Question,
                    &key,
                    tool,
                    note_fact(note),
                ));
            }
            NoteKind::Regular => {
                let Some(fact) = note_fact(note) else {
                    continue;
                };
                let kind = if is_preliminary(note) {
                    MilestoneKind::InitialThoughts
                } else {
                    MilestoneKind::Progress
                };
                out.push(milestone(kind, &key, tool, Some(fact)));
            }
            NoteKind::FeedbackDraft | NoteKind::Status | NoteKind::Activity => {}
        }
    }
    let ai_tool = transition_tool(ticket, &new_notes);
    // Transitions compare against a known baseline only.
    if let (Some(before), Some(tool)) = (previous.and_then(|s| s.baseline.as_ref()), &ai_tool) {
        let now = Baseline::of(ticket);
        let key = ticket.updated_at.as_str();
        if now.status != before.status {
            match now.status {
                Status::Started => {
                    out.push(milestone(MilestoneKind::Started, key, tool.clone(), None));
                }
                Status::Completed | Status::Verified
                    if !matches!(before.status, Status::Completed | Status::Verified) =>
                {
                    out.push(milestone(MilestoneKind::Completed, key, tool.clone(), None));
                }
                _ => {}
            }
        }
        if now.status == Status::Started && now.phase != before.phase {
            let kind = match now.phase {
                Some(StartedPhase::InitialTesting) => Some(MilestoneKind::Testing),
                Some(StartedPhase::FinalTesting) => Some(MilestoneKind::AwaitingCi),
                _ => None,
            };
            if let Some(kind) = kind {
                out.push(milestone(kind, key, tool.clone(), None));
            }
        }
        if now.blocked && !before.blocked {
            let reason = ticket.blocked_reason.as_deref().map(plain_fact);
            let reason = reason.filter(|reason| !reason.is_empty());
            out.push(milestone(MilestoneKind::Blocked, key, tool.clone(), reason));
        }
    }
    let seen_follow_ups = previous.and_then(|snapshot| snapshot.follow_ups.as_ref());
    for reference in &ticket.follow_ups {
        let qualified = reference.qualified();
        if seen_follow_ups.is_some_and(|seen| seen.contains(&qualified)) {
            continue;
        }
        let Some(target) = resolve(reference) else {
            continue;
        };
        if seen_follow_ups.is_none() && !target.created_at.is_some_and(|at| at >= since) {
            continue;
        }
        let tool = match target.created_by_ai {
            Some(true) => ai_tool.clone().unwrap_or_else(|| "agent".into()),
            Some(false) => continue,
            None => match &ai_tool {
                Some(tool) => tool.clone(),
                None => continue,
            },
        };
        let mut follow_up = milestone(MilestoneKind::FollowUp, &qualified, tool, None);
        follow_up.follow_ups.push(FollowUpFacts {
            slug: target.slug,
            title: plain_fact(&target.title),
        });
        out.push(follow_up);
    }
    out
}

/// One write said once: per ticket, initial thoughts replace a bare start; follow-ups join the
/// note that created them (else one follow-up line); a completion takes its note's summary and
/// replaces the testing phases. Order is each surviving milestone's first appearance.
pub fn coalesce(batch: Vec<Milestone>) -> Vec<Milestone> {
    let mut out: Vec<Milestone> = Vec::new();
    let tickets: Vec<(String, String)> = {
        let mut seen = BTreeSet::new();
        batch
            .iter()
            .map(|m| (m.store.clone(), m.ticket_id.clone()))
            .filter(|key| seen.insert(key.clone()))
            .collect()
    };
    for (store, ticket) in tickets {
        let mut group: Vec<Milestone> = batch
            .iter()
            .filter(|m| m.store == store && m.ticket_id == ticket)
            .cloned()
            .collect();
        let has = |group: &[Milestone], kind| group.iter().any(|m| m.kind == kind);
        if has(&group, MilestoneKind::InitialThoughts) {
            group.retain(|m| m.kind != MilestoneKind::Started);
        }
        if let Some(done) = group
            .iter()
            .position(|m| m.kind == MilestoneKind::Completed)
        {
            let note = group
                .iter()
                .rev()
                .find(|m| m.kind == MilestoneKind::Progress)
                .and_then(|m| m.summary.clone());
            if group[done].summary.is_none() {
                group[done].summary = note;
            }
            group.retain(|m| {
                !matches!(
                    m.kind,
                    MilestoneKind::Progress | MilestoneKind::Testing | MilestoneKind::AwaitingCi
                )
            });
        }
        let follow_ups: Vec<FollowUpFacts> = group
            .iter()
            .filter(|m| m.kind == MilestoneKind::FollowUp)
            .flat_map(|m| m.follow_ups.clone())
            .collect();
        if !follow_ups.is_empty() {
            let host = group.iter().rposition(|m| {
                matches!(
                    m.kind,
                    MilestoneKind::Progress
                        | MilestoneKind::InitialThoughts
                        | MilestoneKind::Completed
                )
            });
            match host {
                Some(at) => {
                    group[at].follow_ups.extend(follow_ups);
                    group.retain(|m| m.kind != MilestoneKind::FollowUp);
                }
                None => {
                    let first = group
                        .iter()
                        .position(|m| m.kind == MilestoneKind::FollowUp)
                        .unwrap_or_default();
                    let mut joined = group[first].clone();
                    joined.follow_ups = follow_ups;
                    group.retain(|m| m.kind != MilestoneKind::FollowUp);
                    group.insert(first.min(group.len()), joined);
                }
            }
        }
        out.extend(group);
    }
    // Keep the batch's overall order by each milestone's first appearance.
    let position = |m: &Milestone| {
        batch
            .iter()
            .position(|b| b.id == m.id)
            .unwrap_or(usize::MAX)
    };
    out.sort_by_key(position);
    out
}

const FAILURE_WORDS: &[&str] = &[
    "fail",
    "failed",
    "failing",
    "fails",
    "failure",
    "error",
    "errors",
    "broke",
    "broken",
    "red",
    "regression",
    "crash",
    "crashed",
];
const DONE_WORDS: &[&str] = &[
    "passed", "passing", "green", "pushed", "merged", "shipped", "fixed", "done",
];

fn has_word(text: &str, words: &[&str]) -> bool {
    text.split(|c: char| !c.is_alphanumeric())
        .any(|word| words.contains(&word.to_ascii_lowercase().as_str()))
}

/// The deterministic importance (0–100) used when AI scoring is off or unavailable. Questions,
/// blocks, completions, and initial thoughts are said; failures lift a progress note; bare starts
/// and test phases are shown but not said.
pub fn rule_score(milestone: &Milestone) -> u8 {
    let mut score = u32::from(milestone.kind.base_score());
    if let Some(summary) = &milestone.summary {
        if has_word(summary, FAILURE_WORDS) {
            score += 25;
        } else if milestone.kind == MilestoneKind::Progress && has_word(summary, DONE_WORDS) {
            score += 5;
        }
    }
    if !milestone.follow_ups.is_empty() && milestone.kind != MilestoneKind::FollowUp {
        score += 10;
    }
    u8::try_from(score.min(100)).unwrap_or(100)
}

/// Flatten Markdown-ish text to one plain line: no code marks, emphasis, headings, or link
/// targets (`[text](url)` keeps its text).
pub fn plain_fact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        rest = &rest[c.len_utf8()..];
        match c {
            ']' if rest.starts_with('(') => {
                // Drop a link target up to its closing parenthesis.
                rest = rest.find(')').map_or("", |end| &rest[end + 1..]);
            }
            '`' | '*' | '_' | '#' | '>' | '[' | ']' => {}
            '\n' | '\r' | '\t' => out.push(' '),
            _ => out.push(c),
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    bound(
        collapsed.trim_end_matches(['.', ';', ':', ',']).trim(),
        MAX_FACT_CHARS,
    )
}

fn bound(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let cut: String = text.chars().take(limit).collect();
    let at = cut.rfind(' ').unwrap_or(cut.len());
    format!("{}…", cut[..at].trim_end())
}

/// Escape text quoted into the shown Markdown line.
fn md(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '*' | '_' | '`' | '[' | ']' | '<' | '>' | '\\' | '#') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn sentence(text: &str) -> String {
    let text = text.trim();
    if text.ends_with(['.', '!', '?', '…']) {
        text.to_owned()
    } else {
        format!("{text}.")
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The line the announcer shows: structured, with the slug and title in Markdown.
pub fn shown_line(m: &Milestone) -> String {
    let ticket = format!("**{}** {}", md(&m.slug), md(&m.title));
    let fact = m.summary.as_deref().map(md);
    let mut line = match (m.kind, fact) {
        (MilestoneKind::Started, _) => format!("Started {ticket}"),
        (MilestoneKind::InitialThoughts, Some(f)) => {
            format!("Started {ticket}. Initial thoughts: {f}")
        }
        (MilestoneKind::InitialThoughts, None) => format!("Started {ticket}"),
        (MilestoneKind::Progress, Some(f)) => format!("{ticket}: {f}"),
        (MilestoneKind::Progress, None) => format!("{ticket}: progress"),
        (MilestoneKind::FollowUp, _) => format!("{ticket}: created a follow-up"),
        (MilestoneKind::Question, Some(f)) => format!("{ticket} needs your answer: {f}"),
        (MilestoneKind::Question, None) => format!("{ticket} needs your answer"),
        (MilestoneKind::Blocked, Some(f)) => format!("{ticket} is blocked: {f}"),
        (MilestoneKind::Blocked, None) => format!("{ticket} is blocked"),
        (MilestoneKind::Testing, _) => format!("{ticket} is in testing"),
        (MilestoneKind::AwaitingCi, _) => format!("{ticket} is waiting on CI"),
        (MilestoneKind::Completed, Some(f)) => format!("Completed {ticket}: {f}"),
        (MilestoneKind::Completed, None) => format!("Completed {ticket}"),
    };
    if !m.follow_ups.is_empty() {
        let named: Vec<String> = m
            .follow_ups
            .iter()
            .map(|f| format!("**{}** {}", md(&f.slug), md(&f.title)))
            .collect();
        let plural = if named.len() > 1 { "s" } else { "" };
        line = if m.kind == MilestoneKind::FollowUp {
            format!("{ticket}: created follow-up{plural} {}", named.join(", "))
        } else {
            format!(
                "{} Follow-up{plural}: {}",
                sentence(&line),
                named.join(", ")
            )
        };
    }
    line
}

/// The line the announcer says by default: first person, the ticket by title and short slug. The
/// client adds "For {project}," when the project changes (HS2-MR8GA4).
pub fn spoken_line(m: &Milestone) -> String {
    let ticket = format!("{} ({})", m.title, short_slug(&m.slug));
    let fact = m.summary.as_deref();
    let mut line = match (m.kind, fact) {
        (MilestoneKind::Started, _) => format!("I started working on {ticket}."),
        (MilestoneKind::InitialThoughts, Some(f)) => format!(
            "I started investigating a ticket focusing on {ticket}. My initial thoughts: {}",
            sentence(f)
        ),
        (MilestoneKind::InitialThoughts, None) => {
            format!("I started investigating a ticket focusing on {ticket}.")
        }
        (MilestoneKind::Progress, Some(f)) => {
            format!("An update on {ticket}: {}", sentence(f))
        }
        (MilestoneKind::Progress, None) => format!("I made progress on {ticket}."),
        (MilestoneKind::FollowUp, _) => format!("While working on {ticket},"),
        (MilestoneKind::Question, Some(f)) => format!(
            "I have a question about {ticket}, when you have a moment: {}",
            sentence(f)
        ),
        (MilestoneKind::Question, None) => {
            format!("I have a question about {ticket}, when you have a moment.")
        }
        (MilestoneKind::Blocked, Some(f)) => format!("I'm blocked on {ticket}: {}", sentence(f)),
        (MilestoneKind::Blocked, None) => format!("I'm blocked on {ticket}."),
        (MilestoneKind::Testing, _) => format!("I'm testing {ticket}."),
        (MilestoneKind::AwaitingCi, _) => {
            format!("I've pushed {ticket} and I'm waiting on CI.")
        }
        (MilestoneKind::Completed, Some(f)) => {
            format!("I finished {ticket}. {}", sentence(&capitalized(f)))
        }
        (MilestoneKind::Completed, None) => format!("I finished {ticket}."),
    };
    if !m.follow_ups.is_empty() {
        let named: Vec<String> = m
            .follow_ups
            .iter()
            .take(MAX_NAMED_FOLLOW_UPS)
            .map(|f| format!("{} for {}", short_slug(&f.slug), f.title))
            .collect();
        let more = m.follow_ups.len().saturating_sub(MAX_NAMED_FOLLOW_UPS);
        let created = match (named.as_slice(), &m.follow_ups[..]) {
            ([_], [one]) => format!(
                "I've created a new ticket ({}) for {}.",
                short_slug(&one.slug),
                one.title
            ),
            _ => {
                let mut joined = named.join(", ");
                if more > 0 {
                    joined.push_str(&format!(", and {more} more"));
                }
                format!("I've created {} new tickets: {joined}.", m.follow_ups.len())
            }
        };
        line = format!("{line} {created}");
    }
    line
}

/// How a milestone was judged and worded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub score: u8,
    /// `ai` or `rules`.
    pub scored_by: &'static str,
    pub spoken: String,
    /// `ai` or `template`.
    pub worded_by: &'static str,
}

impl Decision {
    pub fn speaks(&self) -> bool {
        self.score >= SPEAK_THRESHOLD
    }
}

/// The activity event a decided milestone is recorded as. The shown `summary` is the structured
/// line; `detail.spoken` is what the announcer says, and `detail.speak` whether live narration
/// says it at all.
pub fn milestone_event(
    m: &Milestone,
    decision: &Decision,
    id: impl Into<String>,
    ts: impl Into<String>,
) -> ActivityEvent {
    let mut event = ActivityEvent::new(id, ts, m.tool.clone(), m.kind.activity_kind(), Value::Null);
    event.ticket = Some(m.ticket_id.clone());
    event.summary = shown_line(m);
    event.importance = if decision.speaks() {
        Importance::High
    } else if decision.score >= 30 {
        Importance::Normal
    } else {
        Importance::Low
    };
    event.detail = json!({
        "milestone": m.kind.as_str(),
        "candidate": m.id,
        "slug": m.slug,
        "title": m.title,
        "note_summary": m.summary,
        "follow_ups": m.follow_ups,
        "spoken": decision.spoken,
        "speak": decision.speaks(),
        "score": decision.score,
        "scored_by": decision.scored_by,
        "worded_by": decision.worded_by,
    });
    event
}

#[cfg(test)]
#[path = "announcer_milestones_tests.rs"]
mod tests;
