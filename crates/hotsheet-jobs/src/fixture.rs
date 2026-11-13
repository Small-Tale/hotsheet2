//! A file-backed [`WatchSource`] for tests that need a real monitor process and no
//! network. Each external operation is one JSON file a test rewrites to move it along.
//! `hotsheet-cli` registers it only in debug builds, and only when asked by environment.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use serde::Deserialize;

use crate::feed::FeedLine;
use crate::model::{Job, Outcome};
use crate::monitor::{CancelReply, Observation, Observed, Poll, WatchError, WatchSource};
use crate::steps::{FlatStep, tree_from_flat};

/// The Job kind name and the `external.system` this source serves.
pub const FIXTURE_KIND: &str = "fixture-watch";

/// `<dir>/<external.id>.json`. Every poll appends `full` or `conditional` to
/// `<external.id>.polls`, and a cancel appends to `<external.id>.cancels`, so a test can
/// see what the monitor asked for.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Operation {
    /// The validator: an unchanged value answers the next poll with "not modified".
    etag: String,
    /// `waiting`, `running`, `succeeded`, `failed` or `cancelled`.
    state: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    steps: Option<Vec<FlatStep>>,
    #[serde(default)]
    feed: Vec<String>,
    /// `transient` or `gone` makes the poll fail that way.
    #[serde(default)]
    error: Option<String>,
    /// How a cancel is answered: `requested`, `failed`, or anything else for unsupported.
    #[serde(default)]
    cancel: Option<String>,
}

#[derive(Debug)]
pub struct FixtureWatchSource {
    dir: PathBuf,
    seen: BTreeMap<String, String>,
}

impl FixtureWatchSource {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            seen: BTreeMap::new(),
        }
    }

    fn operation(&self, job: &Job) -> Option<(String, Operation)> {
        let external = job.external.as_ref()?;
        let safe = external.system == FIXTURE_KIND
            && !external.id.is_empty()
            && external
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if !safe {
            return None;
        }
        let bytes = fs::read(self.dir.join(format!("{}.json", external.id))).ok()?;
        let operation = serde_json::from_slice(&bytes).ok()?;
        Some((external.id.clone(), operation))
    }

    fn log(&self, name: &str, suffix: &str, line: &str) {
        let path = self.dir.join(format!("{name}.{suffix}"));
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{line}");
        }
    }
}

impl WatchSource for FixtureWatchSource {
    fn kind(&self) -> &str {
        FIXTURE_KIND
    }

    fn poll(&mut self, job: &Job, _now_ms: u64) -> Result<Poll, WatchError> {
        let transient = WatchError::Transient {
            retry_after_ms: None,
        };
        // A file caught mid-rewrite reads as a transient failure and is retried.
        let (name, operation) = self.operation(job).ok_or(transient)?;
        match operation.error.as_deref() {
            Some("gone") => return Err(WatchError::Gone),
            Some(_) => return Err(transient),
            None => {}
        }
        if self.seen.get(&job.id) == Some(&operation.etag) {
            self.log(&name, "polls", "conditional");
            return Ok(Poll::NotModified);
        }
        self.log(&name, "polls", "full");
        let outcome = Outcome {
            summary: operation.summary.clone(),
            refs: BTreeMap::from([("operation".to_owned(), name)]),
        };
        let state = match operation.state.as_str() {
            "waiting" => Observed::Waiting,
            "running" => Observed::Running,
            "succeeded" => Observed::Succeeded(outcome),
            "failed" => Observed::Failed(outcome),
            "cancelled" => Observed::Cancelled(outcome),
            _ => return Err(transient),
        };
        let steps = match operation.steps {
            Some(flat) => Some(tree_from_flat(flat).map_err(|_| transient)?),
            None => None,
        };
        self.seen.insert(job.id.clone(), operation.etag);
        Ok(Poll::Observed(Box::new(Observation {
            steps,
            feed: operation.feed.into_iter().map(FeedLine::info).collect(),
            ..Observation::new(state)
        })))
    }

    fn cancel(&mut self, job: &Job) -> CancelReply {
        let Some((name, operation)) = self.operation(job) else {
            return CancelReply::Unsupported;
        };
        self.log(&name, "cancels", "cancel");
        match operation.cancel.as_deref() {
            Some("requested") => CancelReply::Requested,
            Some("failed") => CancelReply::Failed("fixture refused the cancel".into()),
            _ => CancelReply::Unsupported,
        }
    }

    fn forget(&mut self, job_id: &str) {
        self.seen.remove(job_id);
    }
}
