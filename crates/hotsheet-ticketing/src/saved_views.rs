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

const LIFECYCLES: &[&str] = &[
    "up-next",
    "active",
    "needs-review",
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

fn classify(raw: &str, now: OffsetDateTime) -> Term {
    if let Some(state) = lifecycle(raw) {
        return Term::Is(state);
    }
    if let Some(what) = strip_prefix_ci(raw, "has:") {
        match what.to_ascii_lowercase().as_str() {
            "attachment" => return Term::HasAttachment,
            "media-annotation" => return Term::HasMediaAnnotation,
            "commit" => return Term::HasCommit,
            _ => {}
        }
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
        }
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

fn is_state(state: &str, t: &Ticket, now: OffsetDateTime) -> bool {
    let statuses: &[Status] = match state {
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
        "duplicate" => return t.close_reason == Some(CloseReason::Duplicate),
        "not-started" => &[Status::NotStarted],
        "started" => &[Status::Started],
        "completed" => &[Status::Completed],
        "verified" => &[Status::Verified],
        "backlog" | "backlogged" => &[Status::Backlog],
        "archived" => &[Status::Archive],
        "open" => &[Status::NotStarted, Status::Started],
        "closed" => &[Status::Completed, Status::Verified, Status::Archive],
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
