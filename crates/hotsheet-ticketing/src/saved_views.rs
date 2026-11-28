//! Saved views and their query language, evaluated per ticket (HS2-G8NXNC). A checkout's
//! saved views live in its shared settings (`views`). The web client shows a view by sending
//! its structured terms to the server and evaluating Boolean expressions itself
//! (`clients/web/src/advanced-search.ts`); this is the same language for a process that has
//! one ticket in hand, such as a notification rule asking whether a changed ticket is in a
//! view. `docs/32-notifier-plugins.md` §32.5 lists the differences.

use std::collections::HashSet;

use hotsheet_model::{CloseReason, Status, Ticket};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{Scope, Settings, SettingsError};

const SETTINGS_KEY: &str = "views";

/// One saved view: a named query shared by everyone using the checkout.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SavedView {
    pub id: String,
    pub name: String,
    pub query: String,
}

/// The saved views in `settings`, empty when none were saved.
pub fn from_settings(settings: &Settings) -> Result<Vec<SavedView>, SettingsError> {
    settings
        .get(SETTINGS_KEY, Scope::Shared)?
        .map(|value| {
            serde_json::from_value(value).map_err(|source| SettingsError::Invalid {
                key: SETTINGS_KEY.to_owned(),
                source,
            })
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

/// Replace every saved view in `settings`.
pub fn replace(settings: &Settings, views: &[SavedView]) -> Result<(), SettingsError> {
    settings.set(SETTINGS_KEY, serde_json::json!(views), Scope::Shared)
}

/// Check ids, names, and queries before saving.
pub fn validate(views: &[SavedView]) -> Result<(), String> {
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for view in views {
        let id = view.id.trim();
        let name = view.name.trim();
        let query = view.query.trim();
        if id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("view ids must be 1–64 letters, numbers, or hyphens".to_owned());
        }
        if name.is_empty() || name.chars().count() > 80 {
            return Err("view names must be 1–80 characters".to_owned());
        }
        if query.is_empty() || query.chars().count() > 2_000 {
            return Err("view queries must be 1–2000 characters".to_owned());
        }
        if !ids.insert(id.to_ascii_lowercase()) {
            return Err("view ids must be unique".to_owned());
        }
        if !names.insert(name.to_lowercase()) {
            return Err("view names must be unique".to_owned());
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
enum Lexeme {
    Term(String),
    And,
    Or,
    Not,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Term(String),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

fn lex(query: &str) -> Vec<Lexeme> {
    let chars: Vec<char> = query.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '(' {
            out.push(Lexeme::Left);
            i += 1;
        } else if c == ')' {
            out.push(Lexeme::Right);
            i += 1;
        } else if c == '"' {
            let (mut value, mut closed) = (String::new(), false);
            i += 1;
            while i < chars.len() {
                let c = chars[i];
                i += 1;
                if c == '\\' && i < chars.len() {
                    value.push(chars[i]);
                    i += 1;
                } else if c == '"' {
                    closed = true;
                    break;
                } else {
                    value.push(c);
                }
            }
            out.push(Lexeme::Term(if closed {
                value
            } else {
                format!("\"{value}")
            }));
        } else {
            let start = i;
            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '(' && chars[i] != ')'
            {
                // A quoted value keeps its spaces: `tag:"two words"`, `updated-after:"4h ago"`.
                if chars[i] == '"' {
                    i += 1;
                    while i < chars.len() && chars[i] != '"' {
                        i += if chars[i] == '\\' { 2 } else { 1 };
                    }
                }
                i += 1;
            }
            i = i.min(chars.len());
            let value: String = chars[start..i].iter().collect();
            out.push(match value.to_ascii_lowercase().as_str() {
                "and" => Lexeme::And,
                "or" => Lexeme::Or,
                "not" => Lexeme::Not,
                _ => Lexeme::Term(value),
            });
        }
    }
    out
}

struct Parser {
    lexemes: Vec<Lexeme>,
    cursor: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Lexeme> {
        self.lexemes.get(self.cursor)
    }

    fn primary(&mut self) -> Option<Expr> {
        match self.peek()?.clone() {
            Lexeme::Term(value) => {
                self.cursor += 1;
                Some(Expr::Term(value))
            }
            Lexeme::Left => {
                self.cursor += 1;
                let value = self.or()?;
                (self.peek() == Some(&Lexeme::Right)).then(|| {
                    self.cursor += 1;
                    value
                })
            }
            _ => None,
        }
    }

    fn not(&mut self) -> Option<Expr> {
        if self.peek() == Some(&Lexeme::Not) {
            self.cursor += 1;
            return Some(Expr::Not(Box::new(self.not()?)));
        }
        self.primary()
    }

    fn and(&mut self) -> Option<Expr> {
        let mut left = self.not()?;
        while let Some(next) = self.peek() {
            if matches!(next, Lexeme::Right | Lexeme::Or) {
                break;
            }
            if next == &Lexeme::And {
                self.cursor += 1;
            }
            left = Expr::And(Box::new(left), Box::new(self.not()?));
        }
        Some(left)
    }

    fn or(&mut self) -> Option<Expr> {
        let mut left = self.and()?;
        while self.peek() == Some(&Lexeme::Or) {
            self.cursor += 1;
            left = Expr::Or(Box::new(left), Box::new(self.and()?));
        }
        Some(left)
    }
}

/// Whether `query` uses AND, OR, NOT, or parentheses (the client's
/// `usesBooleanSearchExpression`).
fn is_boolean(lexemes: &[Lexeme]) -> bool {
    lexemes.iter().any(|l| !matches!(l, Lexeme::Term(_)))
}

/// Every supported `is:` value. The web client's `SEARCH_LIFECYCLES`
/// (`clients/web/src/inline-search.ts`) must list the same values in the same order; its
/// HS2-2GCMY1 parity test reads this constant.
const LIFECYCLES: &[&str] = &[
    "ai-created",
    "up-next",
    "active",
    "needs-review",
    "blocked",
    "open",
    "closed",
    "duplicate",
    "not-started",
    "started",
    "completed",
    "verified",
    "backlog",
    "backlogged",
    "archived",
];

fn lifecycle(term: &str) -> Option<String> {
    let state = strip_prefix_ci(term, "is:")?.to_ascii_lowercase();
    LIFECYCLES.contains(&state.as_str()).then_some(state)
}

fn strip_prefix_ci<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    (value.len() >= prefix.len() && value[..prefix.len()].eq_ignore_ascii_case(prefix))
        .then(|| &value[prefix.len()..])
}

fn unquote(value: &str) -> String {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        value[1..value.len() - 1].replace("\\\"", "\"")
    } else {
        value.to_owned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum DateField {
    Created,
    Completed,
    Started,
    Verified,
    Archived,
    Updated,
}

/// One term's meaning; anything unrecognised is case-insensitive text.
#[derive(Debug, Clone, PartialEq)]
enum Term {
    Tag(String),
    /// `group:name`, holding the normalized comparison key (HS2-44W3TP).
    Group(String),
    HasGroup,
    /// `milestone:<id or title>` (HS2-A820C1), holding the unquoted value.
    Milestone(String),
    HasMilestone,
    /// `due:<2026-12-01`, `due:<=7d`, ... over the effective due date (HS2-A820C1).
    Due(DueBound),
    /// `is:overdue`: open, and its effective due has passed in the project zone.
    Overdue,
    Is(String),
    HasAttachment,
    HasMediaAnnotation,
    HasCommit,
    Attachment(String),
    Date {
        field: DateField,
        after: bool,
        at: OffsetDateTime,
    },
    Text(String),
}

/// The comparison of a `due:` term.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DueOp {
    Before,
    OnOrBefore,
    After,
    OnOrAfter,
}

/// A `due:` bound: a calendar date or a number of days from now (HS2-A820C1).
#[derive(Debug, Clone, PartialEq, Eq)]
enum DueBound {
    Date(DueOp, String),
    Days(DueOp, u32),
}

fn due_term(raw: &str) -> Option<DueBound> {
    let value = strip_prefix_ci(raw, "due:")?.trim();
    let (op, rest) = [
        ("<=", DueOp::OnOrBefore),
        (">=", DueOp::OnOrAfter),
        ("<", DueOp::Before),
        (">", DueOp::After),
    ]
    .into_iter()
    .find_map(|(prefix, op)| value.strip_prefix(prefix).map(|rest| (op, rest.trim())))?;
    if let Some(days) = rest
        .strip_suffix(['d', 'D'])
        .and_then(|days| days.parse::<u32>().ok())
    {
        return Some(DueBound::Days(op, days));
    }
    hotsheet_model::parse_due_on(rest).map(|_| DueBound::Date(op, rest.to_owned()))
}

fn classify(raw: &str, now: OffsetDateTime) -> Term {
    if raw.eq_ignore_ascii_case("is:overdue") {
        return Term::Overdue;
    }
    if let Some(bound) = due_term(raw) {
        return Term::Due(bound);
    }
    if let Some(state) = lifecycle(raw) {
        return Term::Is(state);
    }
    if let Some(what) = strip_prefix_ci(raw, "has:") {
        match what.to_ascii_lowercase().as_str() {
            "attachment" => return Term::HasAttachment,
            "media-annotation" => return Term::HasMediaAnnotation,
            "commit" => return Term::HasCommit,
            "group" => return Term::HasGroup,
            "milestone" => return Term::HasMilestone,
            _ => {}
        }
    }
    // HS2-44W3TP: the same quoting as `tag:`, compared by the normalized group key. A
    // value that names no group (only spaces) stays plain text, like an empty `tag:`.
    if let Some(key) = strip_prefix_ci(raw, "group:")
        .map(unquote)
        .and_then(|name| hotsheet_model::group_key_of_input(&name))
    {
        return Term::Group(key);
    }
    if let Some(value) = strip_prefix_ci(raw, "milestone:")
        .map(unquote)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        return Term::Milestone(value);
    }
    if let Some(name) = strip_prefix_ci(raw, "tag:")
        .map(unquote)
        .filter(|n| !n.is_empty())
    {
        return Term::Tag(name);
    }
    if let Some(name) = strip_prefix_ci(raw, "attachment:")
        .map(unquote)
        .filter(|n| !n.is_empty())
    {
        return Term::Attachment(name);
    }
    if let Some(term) = date_term(raw, now) {
        return term;
    }
    Term::Text(raw.to_owned())
}

fn date_term(raw: &str, now: OffsetDateTime) -> Option<Term> {
    let (key, value) = raw.split_once(':')?;
    let (field, direction) = key.split_once('-')?;
    let field = match field.to_ascii_lowercase().as_str() {
        "created" => DateField::Created,
        "completed" => DateField::Completed,
        "started" => DateField::Started,
        "verified" => DateField::Verified,
        "archived" => DateField::Archived,
        "updated" => DateField::Updated,
        _ => return None,
    };
    let after = match direction.to_ascii_lowercase().as_str() {
        "after" => true,
        "before" => false,
        _ => return None,
    };
    Some(Term::Date {
        field,
        after,
        at: parse_date(&unquote(value.trim()), now)?,
    })
}

/// The client's ISO, `YYYY/M/D [H:MM]`, and `N m|h|d|w ago` forms. A date with no zone is
/// read as UTC: this process has no viewer locale. Locale-ordered dates are not read.
fn parse_date(value: &str, now: OffsetDateTime) -> Option<OffsetDateTime> {
    use time::format_description::well_known::Rfc3339;
    let value = value.trim();
    if let Ok(at) = OffsetDateTime::parse(value, &Rfc3339) {
        return Some(at);
    }
    if let Some(ago) = value
        .strip_suffix(" ago")
        .or_else(|| value.strip_suffix(" AGO"))
    {
        let ago = ago.trim();
        let unit = ago.chars().last()?.to_ascii_lowercase();
        let amount: f64 = ago[..ago.len() - 1].trim().parse().ok()?;
        let seconds = match unit {
            'm' => 60.0,
            'h' => 3_600.0,
            'd' => 86_400.0,
            'w' => 604_800.0,
            _ => return None,
        };
        return Some(now - time::Duration::seconds_f64(amount * seconds));
    }
    let (date, clock) = match value.split_once(['T', ' ']) {
        Some((date, clock)) => (date, Some(clock.trim())),
        None => (value, None),
    };
    let separator = if date.contains('-') { '-' } else { '/' };
    let parts: Vec<&str> = date.split(separator).collect();
    let [year, month, day] = parts.as_slice() else {
        return None;
    };
    if year.len() != 4 || (separator == '-' && (month.len() != 2 || day.len() != 2)) {
        return None;
    }
    let (hour, minute) = match clock {
        Some(clock) => {
            let mut pieces = clock.split(':');
            (
                pieces.next()?.parse::<u8>().ok()?,
                pieces.next()?.parse::<u8>().ok()?,
            )
        }
        None => (0, 0),
    };
    let month = time::Month::try_from(month.parse::<u8>().ok()?).ok()?;
    let date = time::Date::from_calendar_date(year.parse().ok()?, month, day.parse().ok()?).ok()?;
    let clock = time::Time::from_hms(hour, minute, 0).ok()?;
    Some(time::PrimitiveDateTime::new(date, clock).assume_utc())
}

/// A parsed saved-view query, ready to test tickets against.
#[derive(Debug, Clone)]
pub struct ViewQuery {
    source: String,
    shape: Shape,
    /// The project's group names, so `group:name` matches tickets that store a group
    /// id (HS2-9KZSD4). Without them a name matches only legacy name values and ids.
    group_names: Option<crate::groups::GroupNames>,
    /// The project's milestones and time zone, so `milestone:<title>`, `due:` and
    /// `is:overdue` read effective due dates (HS2-A820C1). Without them a title matches
    /// nothing, only own due dates count, and the zone is UTC.
    milestones: Option<MilestoneContext>,
}

/// The milestone registry snapshot and zone a view reads (HS2-A820C1).
#[derive(Debug, Clone)]
pub struct MilestoneContext {
    pub milestones: Vec<crate::milestones::Milestone>,
    pub zone: jiff::tz::TimeZone,
}

#[derive(Debug, Clone)]
enum Shape {
    /// A Boolean expression, each term read structurally.
    Expression(Expr),
    /// No Boolean operators: every structured term must hold, and the remaining words are one
    /// phrase, as the client sends them to the server's text filter.
    Terms {
        terms: Vec<String>,
        phrase: Option<String>,
    },
    /// An expression that does not parse: the whole query is text.
    Text(String),
}

impl ViewQuery {
    pub fn parse(query: &str) -> Self {
        let lexemes = lex(query);
        let shape = if is_boolean(&lexemes) {
            let mut parser = Parser { lexemes, cursor: 0 };
            match parser.or() {
                Some(expr) if parser.cursor == parser.lexemes.len() => Shape::Expression(expr),
                _ => Shape::Text(query.trim().to_owned()),
            }
        } else {
            let terms: Vec<String> = lexemes
                .into_iter()
                .filter_map(|l| match l {
                    Lexeme::Term(value) => Some(value),
                    _ => None,
                })
                .collect();
            // A lifecycle term sends the client down its expression path, where each word
            // is its own term; otherwise the words are one server-side text phrase.
            if terms.iter().any(|t| lifecycle(t).is_some()) {
                Shape::Terms {
                    terms,
                    phrase: None,
                }
            } else {
                let probe = OffsetDateTime::UNIX_EPOCH;
                let (words, structured): (Vec<String>, Vec<String>) = terms
                    .into_iter()
                    .partition(|t| matches!(classify(t, probe), Term::Text(_)));
                Shape::Terms {
                    terms: structured,
                    phrase: (!words.is_empty()).then(|| words.join(" ")),
                }
            }
        };
        Self {
            source: query.to_owned(),
            shape,
            group_names: None,
            milestones: None,
        }
    }

    /// Resolve `milestone:` titles and effective due dates through a project's milestone
    /// registry snapshot, in the project time zone (HS2-A820C1).
    #[must_use]
    pub fn with_milestones(mut self, context: MilestoneContext) -> Self {
        self.milestones = Some(context);
        self
    }

    /// Resolve `group:` terms through a project's group registry snapshot.
    #[must_use]
    pub fn with_group_names(mut self, names: crate::groups::GroupNames) -> Self {
        self.group_names = Some(names);
        self
    }

    /// The query text this was parsed from.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Whether the query has a `has:commit` term, which needs a repository scan.
    pub fn uses_commits(&self) -> bool {
        let probe = OffsetDateTime::UNIX_EPOCH;
        let commit = |raw: &String| classify(raw, probe) == Term::HasCommit;
        fn walk(expr: &Expr, commit: &dyn Fn(&String) -> bool) -> bool {
            match expr {
                Expr::Term(raw) => commit(raw),
                Expr::Not(inner) => walk(inner, commit),
                Expr::And(left, right) | Expr::Or(left, right) => {
                    walk(left, commit) || walk(right, commit)
                }
            }
        }
        match &self.shape {
            Shape::Expression(expr) => walk(expr, &commit),
            Shape::Terms { terms, .. } => terms.iter().any(commit),
            Shape::Text(_) => false,
        }
    }

    /// Whether `ticket` is in the view at `now` (relative dates and live claims use it).
    /// `has:commit` never matches here; see [`ViewQuery::matches_with_commits`].
    pub fn matches(&self, ticket: &Ticket, now: OffsetDateTime) -> bool {
        self.matches_with_commits(ticket, now, &|| false)
    }

    /// [`ViewQuery::matches`], answering `has:commit` with `has_commit`: whether the
    /// project's repository has a commit naming the ticket (HS2-VPG8XA). It is called only
    /// when a `has:commit` term is evaluated.
    pub fn matches_with_commits(
        &self,
        ticket: &Ticket,
        now: OffsetDateTime,
        has_commit: &dyn Fn() -> bool,
    ) -> bool {
        let at = At {
            ticket,
            now,
            has_commit,
            group_names: self.group_names.as_ref(),
            milestones: self.milestones.as_ref(),
        };
        match &self.shape {
            Shape::Expression(expr) => eval(expr, &at),
            Shape::Terms { terms, phrase } => {
                terms.iter().all(|t| term_matches(t, &at))
                    && phrase
                        .as_deref()
                        .is_none_or(|p| crate::ops::matches_text(ticket, &p.to_lowercase()))
            }
            Shape::Text(text) => crate::ops::matches_text(ticket, &text.to_lowercase()),
        }
    }
}

/// What one evaluation reads: the ticket, the time, and the commit lookup.
struct At<'a> {
    ticket: &'a Ticket,
    now: OffsetDateTime,
    has_commit: &'a dyn Fn() -> bool,
    group_names: Option<&'a crate::groups::GroupNames>,
    milestones: Option<&'a MilestoneContext>,
}

impl At<'_> {
    fn milestone(&self, id: &str) -> Option<&crate::milestones::Milestone> {
        self.milestones?
            .milestones
            .iter()
            .find(|milestone| milestone.id == id)
    }

    fn zone(&self) -> jiff::tz::TimeZone {
        self.milestones
            .map_or(jiff::tz::TimeZone::UTC, |context| context.zone.clone())
    }

    /// The ticket's effective due date (docs/38 §38.3).
    fn effective_due(&self) -> Option<&str> {
        let t = self.ticket;
        crate::milestone_rollup::effective_due_on(
            t.due_on.as_deref(),
            t.milestone.as_deref().and_then(|id| self.milestone(id)),
        )
    }

    fn now_ms(&self) -> i64 {
        i64::try_from(self.now.unix_timestamp_nanos() / 1_000_000).unwrap_or(i64::MAX)
    }
}

fn due_matches(bound: &DueBound, at: &At<'_>) -> bool {
    let Some(due) = at.effective_due() else {
        return false;
    };
    let compare = |op: DueOp, ordering: std::cmp::Ordering| match op {
        DueOp::Before => ordering.is_lt(),
        DueOp::OnOrBefore => ordering.is_le(),
        DueOp::After => ordering.is_gt(),
        DueOp::OnOrAfter => ordering.is_ge(),
    };
    match bound {
        DueBound::Date(op, date) => compare(*op, due.cmp(date.as_str())),
        DueBound::Days(op, days) => {
            let Some(deadline) = crate::milestone_rollup::due_deadline_ms(Some(due), &at.zone())
            else {
                return false;
            };
            let limit = at
                .now_ms()
                .saturating_add(i64::from(*days).saturating_mul(86_400_000));
            compare(*op, deadline.cmp(&limit))
        }
    }
}

/// Whether a stored group value is the group `key` names: a legacy name by its key, an
/// id by itself or by its registry name (HS2-9KZSD4).
fn group_term_matches(value: &str, key: &str, names: Option<&crate::groups::GroupNames>) -> bool {
    match hotsheet_model::GroupValue::of(value) {
        hotsheet_model::GroupValue::Legacy(name) => hotsheet_model::group_key(name) == key,
        hotsheet_model::GroupValue::Id(id) => {
            hotsheet_model::group_key(id) == key
                || names
                    .and_then(|names| names.get(id))
                    .is_some_and(|name| hotsheet_model::group_key(name) == key)
        }
    }
}

fn eval(expr: &Expr, at: &At<'_>) -> bool {
    match expr {
        Expr::Term(raw) => term_matches(raw, at),
        Expr::Not(inner) => !eval(inner, at),
        Expr::And(left, right) => eval(left, at) && eval(right, at),
        Expr::Or(left, right) => eval(left, at) || eval(right, at),
    }
}

fn term_matches(raw: &str, at: &At<'_>) -> bool {
    let (t, now) = (at.ticket, at.now);
    match classify(raw, now) {
        Term::Tag(name) => t.tags.iter().any(|tag| tag.eq_ignore_ascii_case(&name)),
        Term::Group(key) => t
            .group
            .as_deref()
            .is_some_and(|value| group_term_matches(value, &key, at.group_names)),
        Term::HasGroup => t.group.is_some(),
        Term::Milestone(value) => t.milestone.as_deref().is_some_and(|id| {
            id == value
                || at.milestone(id).is_some_and(|milestone| {
                    milestone.named
                        && crate::milestones::milestone_title_key(&milestone.title)
                            == crate::milestones::milestone_title_key(&value)
                })
        }),
        Term::HasMilestone => t.milestone.is_some(),
        Term::Due(bound) => due_matches(&bound, at),
        Term::Overdue => {
            // Open work as the roll-up counts it: not started, started, or Backlog.
            matches!(
                t.status,
                Status::NotStarted | Status::Started | Status::Backlog
            ) && t.close_reason.is_none()
                && at.effective_due().is_some_and(|due| {
                    crate::milestone_rollup::due_deadline_ms(Some(due), &at.zone()).is_some_and(
                        |deadline| crate::project_time_zone::is_overdue(deadline, at.now_ms()),
                    )
                })
        }
        Term::Is(state) => is_state(&state, t, now),
        Term::HasAttachment => !t.attachments.is_empty(),
        Term::HasMediaAnnotation => t.attachments.iter().any(|a| !a.annotations.is_empty()),
        Term::HasCommit => (at.has_commit)(),
        Term::Attachment(pattern) => t
            .attachments
            .iter()
            .any(|a| crate::ops::filename_glob_matches(&pattern, &a.filename)),
        Term::Date { field, after, at } => {
            let (stamp, status) = match field {
                DateField::Created => (Some(&t.created_at), None),
                DateField::Updated => (Some(&t.updated_at), None),
                DateField::Completed => (t.completed_at.as_ref(), None),
                DateField::Verified => (t.verified_at.as_ref(), None),
                DateField::Started => (Some(&t.updated_at), Some(Status::Started)),
                DateField::Archived => (Some(&t.updated_at), Some(Status::Archive)),
            };
            status.is_none_or(|s| t.status == s)
                && stamp
                    .and_then(|s| s.instant())
                    .is_some_and(|value| if after { value >= at } else { value <= at })
        }
        Term::Text(text) => {
            let needle = text.to_lowercase();
            !needle.is_empty() && crate::ops::matches_text(t, &needle)
        }
    }
}

/// HS2-C7917F: closed means a terminal status (completed, verified, or archive) or a
/// recorded close outcome. [`crate::ops::close`] keeps a Backlog ticket's status and records
/// only the outcome, so status alone misses it; the web evaluator applies the same rule.
fn is_closed(t: &Ticket) -> bool {
    t.close_reason.is_some()
        || matches!(
            t.status,
            Status::Completed | Status::Verified | Status::Archive
        )
}

fn is_state(state: &str, t: &Ticket, now: OffsetDateTime) -> bool {
    let statuses: &[Status] = match state {
        "ai-created" => {
            return t
                .created_by
                .as_ref()
                .is_some_and(|actor| actor.role == hotsheet_model::AttachmentActorRole::Ai);
        }
        "up-next" => return t.up_next && t.status.is_active(),
        "active" => {
            return t.status.is_active()
                && t.claimed_by.is_some()
                && t.claim_lease_expires_at
                    .as_ref()
                    .and_then(|s| s.instant())
                    .is_some_and(|expiry| expiry > now);
        }
        "needs-review" => return t.feedback_needed() || !t.review_requests.is_empty(),
        // HS2-2GCMY1: the same definition claim-next uses: a non-empty blocked reason.
        "blocked" => return crate::ops::is_blocked(t, &std::collections::HashSet::new()),
        "duplicate" => return t.close_reason == Some(CloseReason::Duplicate),
        "not-started" => &[Status::NotStarted],
        "started" => &[Status::Started],
        "completed" => &[Status::Completed],
        "verified" => &[Status::Verified],
        "backlog" | "backlogged" => &[Status::Backlog],
        "archived" => &[Status::Archive],
        "open" => &[Status::NotStarted, Status::Started],
        "closed" => return is_closed(t),
        _ => return false,
    };
    statuses.contains(&t.status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_model::ticket::Note;
    use hotsheet_model::{Attachment, NoteKind, ReviewKind, ReviewRequest, Timestamp, Ulid};

    fn at(raw: &str) -> OffsetDateTime {
        OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339).unwrap()
    }

    fn ticket() -> Ticket {
        let mut t = Ticket::new(
            Ulid::new(),
            "HS-1",
            "Fix the login page",
            "bug",
            Timestamp::new("2026-10-01T12:00:00Z"),
            Timestamp::new("2026-10-01T12:00:00Z"),
        );
        t.tags = vec!["Docs".into(), "web ui".into()];
        t.details = "The submit button overlaps.".into();
        t
    }

    fn matches(query: &str, t: &Ticket) -> bool {
        ViewQuery::parse(query).matches(t, at("2026-10-30T00:00:00Z"))
    }

    #[test]
    fn round_trips_views_through_shared_project_settings() {
        let root = tempfile::tempdir().unwrap();
        let settings =
            Settings::for_project_with_global_home(root.path(), root.path().join("home"));
        let views = vec![SavedView {
            id: "needs-docs".to_owned(),
            name: "Needs docs".to_owned(),
            query: "tag:docs AND NOT status:completed".to_owned(),
        }];
        validate(&views).unwrap();
        replace(&settings, &views).unwrap();
        assert_eq!(from_settings(&settings).unwrap(), views);
    }

    #[test]
    fn rejects_empty_queries_and_case_insensitive_duplicate_names() {
        let empty = vec![SavedView {
            id: "empty".into(),
            name: "Empty".into(),
            query: " ".into(),
        }];
        assert!(validate(&empty).unwrap_err().contains("queries"));
        let duplicates = vec![
            SavedView {
                id: "one".into(),
                name: "Review".into(),
                query: "one".into(),
            },
            SavedView {
                id: "two".into(),
                name: "review".into(),
                query: "two".into(),
            },
        ];
        assert!(validate(&duplicates).unwrap_err().contains("names"));
    }

    #[test]
    fn plain_words_are_one_phrase_like_the_server_text_filter() {
        let t = ticket();
        assert!(matches("login page", &t));
        assert!(matches("LOGIN", &t));
        assert!(matches("submit button", &t));
        // Not adjacent: the phrase does not appear.
        assert!(!matches("page login", &t));
        assert!(matches("tag:docs login", &t));
        assert!(!matches("tag:other login", &t));
    }

    #[test]
    fn a_lifecycle_term_splits_words_like_the_client_expression_path() {
        let t = ticket();
        assert!(matches("is:open page login", &t));
        assert!(!matches("is:completed login", &t));
    }

    #[test]
    fn hs2_18qfj5_ai_created_requires_recorded_ai_provenance_and_supports_boolean_terms() {
        let mut t = ticket();
        assert!(!matches("is:ai-created", &t));
        assert!(matches("NOT is:ai-created", &t));
        t.created_by = Some(hotsheet_model::NoteActor {
            role: hotsheet_model::AttachmentActorRole::Human,
            id: None,
        });
        assert!(!matches("IS:AI-CREATED", &t));
        t.created_by.as_mut().unwrap().role = hotsheet_model::AttachmentActorRole::Ai;
        assert!(matches("IS:AI-CREATED", &t));
        assert!(matches("is:ai-created AND tag:docs", &t));
        assert!(!matches("NOT is:ai-created", &t));
    }

    #[test]
    fn boolean_expressions_follow_precedence_and_parentheses() {
        let t = ticket();
        assert!(matches("tag:docs AND NOT is:completed", &t));
        assert!(matches("tag:nope OR tag:docs", &t));
        assert!(!matches("tag:nope OR tag:docs AND is:completed", &t));
        assert!(matches("(tag:nope OR tag:docs) AND is:not-started", &t));
        assert!(matches("NOT NOT login", &t));
        assert!(matches("(login submit)", &t), "implicit AND across words");
        assert!(
            !matches("login submit", &t),
            "without operators the words are one phrase"
        );
        assert!(matches("tag:\"web ui\" and login", &t));
        assert!(matches("\"login page\" or nothing", &t));
    }

    fn milestone_context(zone: &str) -> MilestoneContext {
        let named = |id: &str, title: &str, due: Option<&str>, state| {
            let mut m = crate::milestones::Milestone::unknown(id);
            m.title = title.into();
            m.due_on = due.map(str::to_string);
            m.state = state;
            m.named = true;
            m
        };
        MilestoneContext {
            milestones: vec![
                named(
                    "7K3M9P2QXA",
                    "Toolchain 2.0",
                    Some("2026-11-05"),
                    crate::milestones::MilestoneState::Open,
                ),
                named(
                    "4N8R2T6VWX",
                    "Old Release",
                    Some("2026-10-01"),
                    crate::milestones::MilestoneState::Closed,
                ),
            ],
            zone: jiff::tz::TimeZone::get(zone).unwrap(),
        }
    }

    fn matches_in(query: &str, t: &Ticket, zone: &str, now: &str) -> bool {
        ViewQuery::parse(query)
            .with_milestones(milestone_context(zone))
            .matches(t, at(now))
    }

    #[test]
    fn hs2_a820c1_milestone_terms_match_by_id_or_title() {
        let now = "2026-10-30T00:00:00Z";
        let mut t = ticket();
        assert!(!matches_in("has:milestone", &t, "UTC", now));
        assert!(matches_in("NOT has:milestone", &t, "UTC", now));
        t.milestone = Some("7K3M9P2QXA".into());
        for query in [
            "milestone:7K3M9P2QXA",
            "milestone:\"toolchain   2.0\"",
            "MILESTONE:\"Toolchain 2.0\"",
            "has:milestone",
            "has:milestone AND tag:docs",
        ] {
            assert!(matches_in(query, &t, "UTC", now), "{query}");
        }
        assert!(!matches_in("milestone:\"Old Release\"", &t, "UTC", now));
        assert!(
            !matches("milestone:\"Toolchain 2.0\"", &t),
            "a title needs the registry snapshot"
        );
        assert!(
            matches("milestone:7K3M9P2QXA", &t),
            "an id matches without one"
        );
        // An unknown id still matches itself.
        t.milestone = Some("ZZZZZZZZZZ".into());
        assert!(matches_in("milestone:ZZZZZZZZZZ", &t, "UTC", now));
        assert!(!matches_in("milestone:\"Toolchain 2.0\"", &t, "UTC", now));
    }

    #[test]
    fn hs2_a820c1_due_terms_read_the_effective_due_date() {
        let now = "2026-10-30T00:00:00Z";
        let mut t = ticket();
        assert!(
            !matches_in("due:<2099-01-01", &t, "UTC", now),
            "no due date"
        );
        // Own date only.
        t.due_on = Some("2026-11-20".into());
        assert!(matches_in("due:<2026-11-21", &t, "UTC", now));
        assert!(!matches_in("due:<2026-11-20", &t, "UTC", now));
        assert!(matches_in("due:<=2026-11-20", &t, "UTC", now));
        assert!(matches_in("due:>=2026-11-20", &t, "UTC", now));
        assert!(!matches_in("due:>2026-11-20", &t, "UTC", now));
        // The open milestone's earlier date wins.
        t.milestone = Some("7K3M9P2QXA".into());
        assert!(matches_in("due:<=2026-11-05", &t, "UTC", now));
        assert!(matches_in("due:<=7d", &t, "UTC", now));
        assert!(!matches_in("due:<=5d", &t, "UTC", now));
        // A closed milestone lends no date: only the ticket's own counts.
        t.milestone = Some("4N8R2T6VWX".into());
        assert!(!matches_in("due:<=2026-11-05", &t, "UTC", now));
        t.due_on = None;
        assert!(!matches_in("due:<2099-01-01", &t, "UTC", now));
        // Malformed bounds are plain text, never a due filter.
        assert!(!matches_in("due:<soon", &t, "UTC", now));
    }

    #[test]
    fn hs2_a820c1_is_overdue_uses_the_project_zone_and_open_status() {
        let mut t = ticket();
        t.due_on = Some("2026-11-05".into());
        // The deadline is the end of the day in the project zone.
        assert!(!matches_in("is:overdue", &t, "UTC", "2026-11-05T23:59:59Z"));
        assert!(matches_in("is:overdue", &t, "UTC", "2026-11-06T00:00:01Z"));
        // Berlin (UTC+1) ends Nov 5 at 23:00 UTC; Los Angeles (UTC-8) at 08:00 UTC Nov 6.
        assert!(matches_in(
            "is:overdue",
            &t,
            "Europe/Berlin",
            "2026-11-05T23:30:00Z"
        ));
        assert!(!matches_in(
            "is:overdue",
            &t,
            "America/Los_Angeles",
            "2026-11-06T07:00:00Z"
        ));
        assert!(matches_in(
            "is:overdue",
            &t,
            "America/Los_Angeles",
            "2026-11-06T08:00:01Z"
        ));
        // Finished or closed tickets are never overdue.
        t.status = Status::Completed;
        assert!(!matches_in("is:overdue", &t, "UTC", "2027-01-01T00:00:00Z"));
        t.status = Status::Backlog;
        assert!(matches_in("is:overdue", &t, "UTC", "2027-01-01T00:00:00Z"));
        t.close_reason = Some(CloseReason::NotPlanned);
        assert!(!matches_in("is:overdue", &t, "UTC", "2027-01-01T00:00:00Z"));
    }

    #[test]
    fn hs2_44w3tp_group_terms_match_by_normalized_name() {
        let mut t = ticket();
        t.group = Some("Board Drag & Drop".into());
        assert!(matches("group:\"board drag & drop\"", &t));
        assert!(matches("GROUP:\"  BOARD   drag &  DROP \"", &t));
        assert!(matches("has:group", &t));
        assert!(matches("HAS:GROUP login", &t));
        assert!(
            !matches("group:board", &t),
            "a group name is not a prefix match"
        );
        assert!(!matches("group:\"no such group\"", &t));
        assert!(!matches("NOT has:group", &t));

        t.group = Some("Sprint".into());
        assert!(matches("group:sprint", &t));
        assert!(matches("group:SPRINT login", &t));
        assert!(!matches("group:sprint nothing-here", &t));

        t.group = Some("Say \"hi\" now".into());
        assert!(matches(r#"group:"say \"HI\" now""#, &t), "escaped quotes");
        assert!(!matches(r#"group:"say hi now""#, &t));

        t.group = Some("Überarbeitung".into());
        assert!(matches("group:überarbeitung", &t));
        assert!(matches("group:ÜBERARBEITUNG", &t), "Unicode case folding");
    }

    #[test]
    fn hs2_9kzsd4_group_terms_match_ids_through_registry_names() {
        let mut t = ticket();
        t.group = Some("7K3M9P2QXA".into());
        let plain = |q: &str| ViewQuery::parse(q).matches(&t, OffsetDateTime::UNIX_EPOCH);
        let named = |q: &str| {
            let names = [("7K3M9P2QXA".to_string(), "Board Drag".to_string())]
                .into_iter()
                .collect();
            ViewQuery::parse(q)
                .with_group_names(names)
                .matches(&t, OffsetDateTime::UNIX_EPOCH)
        };
        assert!(plain("group:7K3M9P2QXA"), "an id always matches itself");
        assert!(!plain("group:\"board drag\""), "no registry, no name");
        assert!(named("group:\"BOARD  drag\""));
        assert!(named("group:7k3m9p2qxa"));
        assert!(!named("group:board"));
        assert!(named("has:group AND NOT group:other"));
    }

    #[test]
    fn hs2_44w3tp_group_terms_combine_with_boolean_operators() {
        let mut grouped = ticket();
        grouped.group = Some("Sprint 12".into());
        let ungrouped = ticket();

        assert!(matches("NOT has:group", &ungrouped));
        assert!(!matches("has:group", &ungrouped));
        assert!(!matches("group:\"sprint 12\"", &ungrouped));
        assert!(matches("NOT group:\"sprint 12\"", &ungrouped));

        assert!(matches("group:\"sprint 12\" AND tag:docs", &grouped));
        assert!(!matches("group:\"sprint 12\" AND tag:nope", &grouped));
        assert!(matches("group:other OR group:\"sprint 12\"", &grouped));
        assert!(matches("has:group AND NOT group:other", &grouped));
        assert!(!matches("NOT (has:group OR tag:nope)", &grouped));
        assert!(matches("(NOT has:group OR tag:docs) AND login", &grouped));
        assert!(matches("NOT has:group OR tag:docs", &ungrouped));
        assert!(!matches("has:group AND is:completed", &grouped));
        // An empty group value is plain text, exactly like an empty tag value.
        assert!(!matches("group:\"\" OR has:group", &ungrouped));
    }

    #[test]
    fn an_unparseable_expression_is_whole_query_text() {
        let mut t = ticket();
        assert!(!matches("(login", &t));
        t.title = "Odd (login title".into();
        assert!(matches("(login", &t));
        assert!(!matches("login AND", &t));
    }

    #[test]
    fn lifecycle_terms_read_status_queue_review_and_claims() {
        let mut t = ticket();
        assert!(matches("is:not-started", &t) && matches("is:open", &t));
        assert!(!matches("is:up-next", &t));
        t.up_next = true;
        assert!(matches("is:up-next", &t));
        t.status = Status::Backlog;
        assert!(!matches("is:up-next", &t), "Up Next is active work only");
        assert!(matches("is:backlog", &t) && matches("is:backlogged", &t));
        t.status = Status::Archive;
        assert!(matches("is:archived", &t) && matches("is:closed", &t));
        t.status = Status::Completed;
        t.close_reason = Some(CloseReason::Duplicate);
        assert!(matches("is:duplicate", &t) && matches("is:completed", &t));
        t.status = Status::Verified;
        assert!(matches("is:verified", &t) && !matches("is:open", &t));

        t.status = Status::Started;
        assert!(!matches("is:active", &t));
        t.claimed_by = Some("w1".into());
        t.claim_lease_expires_at = Some(Timestamp::new("2026-10-30T00:05:00Z"));
        assert!(matches("is:active", &t));
        t.claim_lease_expires_at = Some(Timestamp::new("2026-10-29T23:55:00Z"));
        assert!(!matches("is:active", &t), "expired lease");

        assert!(!matches("is:needs-review", &t));
        t.review_requests.push(ReviewRequest {
            who: "a@x.test".into(),
            kind: ReviewKind::Review,
            by: Ulid::new(),
            at: Timestamp::new("2026-10-02T00:00:00Z"),
            requested_by: None,
        });
        assert!(matches("is:needs-review", &t));
        assert!(!matches("is:bogus", &t));
    }

    #[test]
    fn hs2_c7917f_is_closed_includes_a_close_outcome_on_any_status() {
        let mut t = ticket();
        t.status = Status::Backlog;
        assert!(!matches("is:closed", &t) && matches("NOT is:closed", &t));
        for reason in [
            CloseReason::NotPlanned,
            CloseReason::Obsolete,
            CloseReason::WorksAsDesigned,
            CloseReason::Completed,
            CloseReason::Duplicate,
        ] {
            t.close_reason = Some(reason);
            assert!(matches("is:closed", &t), "{reason:?} on Backlog is closed");
            assert!(matches("is:closed AND is:backlog", &t) && !matches("is:open", &t));
        }
        t.close_reason = None;
        for status in [Status::Completed, Status::Verified, Status::Archive] {
            t.status = status;
            assert!(
                matches("is:closed", &t),
                "{status:?} is closed without an outcome"
            );
        }
    }

    #[test]
    fn hs2_2gcmy1_is_blocked_follows_the_blocked_reason() {
        let mut t = ticket();
        assert!(!matches("is:blocked", &t));
        t.blocked_reason = Some("   ".into());
        assert!(!matches("is:blocked", &t), "a blank reason does not block");
        t.blocked_reason = Some("Waits on the vendor".into());
        assert!(matches("is:blocked", &t) && matches("IS:BLOCKED", &t));
        assert!(matches("is:blocked AND is:open", &t) && !matches("NOT is:blocked", &t));
        t.blocked_reason = None;
        assert!(matches("NOT is:blocked", &t));
    }

    #[test]
    fn attachment_and_presence_terms() {
        let mut t = ticket();
        assert!(!matches("has:attachment", &t));
        t.attachments.push(Attachment {
            id: Ulid::new(),
            filename: "shot-01.png".into(),
            created_at: Timestamp::new("2026-10-02T00:00:00Z"),
            batch_id: None,
            batch_label: None,
            actor: None,
            purpose: None,
            annotations: Vec::new(),
            crop: None,
        });
        assert!(matches("has:attachment", &t));
        assert!(matches("attachment:shot-*.png", &t));
        assert!(matches("attachment:SHOT", &t));
        assert!(!matches("attachment:*.pdf", &t));
        assert!(!matches("has:media-annotation", &t));
        assert!(!matches("has:commit", &t), "needs a repository scan");
    }

    #[test]
    fn hs2_vpg8xa_has_commit_asks_the_lookup_only_when_evaluated() {
        let t = ticket();
        let now = at("2026-10-30T00:00:00Z");
        let asked = std::cell::Cell::new(0);
        let yes = || {
            asked.set(asked.get() + 1);
            true
        };
        let q = |s: &str| ViewQuery::parse(s);
        assert!(q("has:commit").uses_commits());
        assert!(q("tag:docs OR NOT has:COMMIT").uses_commits());
        assert!(q("has:commit login").uses_commits());
        assert!(!q("tag:docs AND login").uses_commits());
        assert!(
            !q("(has:commit").uses_commits(),
            "unparseable: whole-query text"
        );
        assert!(q("has:commit").matches_with_commits(&t, now, &yes));
        assert!(!q("has:commit").matches_with_commits(&t, now, &|| false));
        assert!(!q("has:commit").matches(&t, now), "no lookup, no commit");
        assert!(q("NOT has:commit").matches(&t, now));
        assert!(q("tag:docs OR has:commit").matches_with_commits(&t, now, &yes));
        assert_eq!(asked.get(), 1, "OR stops at the tag");
        assert!(q("has:commit login").matches_with_commits(&t, now, &yes));
        assert!(q("tag:docs").matches_with_commits(&t, now, &|| panic!("not asked")));
    }

    #[test]
    fn date_terms_accept_iso_slash_and_relative_forms() {
        let mut t = ticket();
        assert!(matches("created-after:2026-09-30", &t));
        assert!(!matches("created-before:2026-09-30", &t));
        assert!(matches("created-before:\"2026/10/01 12:00\"", &t));
        assert!(!matches("created-before:\"2026/10/01 11:59\"", &t));
        assert!(matches("created-after:2026-10-01T11:00:00Z", &t));
        assert!(matches("updated-before:\"2w ago\"", &t));
        assert!(!matches("updated-after:\"2w ago\"", &t));
        assert!(
            !matches("completed-after:2020-01-01", &t),
            "never completed"
        );
        t.completed_at = Some(Timestamp::new("2026-10-29T00:00:00Z"));
        assert!(matches("completed-after:\"2d ago\"", &t));
        assert!(!matches("started-after:2020-01-01", &t), "not started");
        t.status = Status::Started;
        assert!(matches("started-after:2020-01-01", &t));
        // A date that does not parse is ordinary text, as in the client.
        assert!(!matches("created-after:someday", &t));
        t.details = "see created-after:someday".into();
        assert!(matches("created-after:someday", &t));
    }

    #[test]
    fn notes_details_and_tags_are_searched_as_text() {
        let mut t = ticket();
        t.notes.push(Note {
            id: Ulid::new(),
            kind: NoteKind::Regular,
            kind_explicit: false,
            created_at: Timestamp::new("2026-10-02T00:00:00Z"),
            edited_at: Timestamp::new("2026-10-02T00:00:00Z"),
            summary: None,
            confidence: None,
            feedback_for: None,
            feedback_request: false,
            response_to: None,
            ai_feedback: None,
            human_edited: false,
            actor: None,
            text: "Waiting on QA".into(),
        });
        assert!(matches("on qa", &t));
        assert!(matches("web", &t));
        assert!(matches("overlaps", &t));
    }
}
