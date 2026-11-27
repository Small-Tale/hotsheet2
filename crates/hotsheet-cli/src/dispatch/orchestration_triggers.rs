//! HS2-KXKA7X: the headless trigger surface. Every request goes through the shared
//! orchestration runtime; this file only shapes them, so each graphical trigger path
//! (set up, review, turn On, Dry Run, Pause, Resume, remove) has an idempotent one.
use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand, ValueEnum};
use serde_json::{Map, Value, json};

#[derive(Args)]
pub(super) struct TriggerArgs {
    /// Registered checkout id of the orchestration project.
    #[arg(long, global = true)]
    pub(super) project: Option<String>,
    #[command(subcommand)]
    command: TriggerCmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum Start {
    DryRun,
    Paused,
}

#[derive(Subcommand)]
enum TriggerCmd {
    /// This machine's triggers of the project, with their next run times.
    List,
    /// One trigger, with its recent runs.
    Show { trigger_id: String },
    /// A trigger's recent runs (the same answer as `show`).
    History { trigger_id: String },
    /// Add a trigger, or replace one's definition, from a JSON file (`-` for stdin):
    /// `name`, `source`, `action`, `overlap` and, to replace, `trigger_id`. A new
    /// trigger starts as Dry Run (or Paused); editing an On trigger sends it to Review.
    Set {
        #[arg(long)]
        file: String,
        /// How a new trigger starts.
        #[arg(long, value_enum)]
        start: Option<Start>,
    },
    /// What a trigger watches, which projects it may change, what one event creates,
    /// what will ask, and what happens next. Writes nothing and asks nobody.
    Review {
        /// A saved trigger.
        trigger_id: Option<String>,
        /// Or a definition not saved yet (JSON file, `-` for stdin).
        #[arg(long, conflicts_with = "trigger_id")]
        file: Option<String>,
    },
    /// Turn a trigger On after its review. A person's own act; an AI actor asks
    /// through the permission card and is answered permission_required until a
    /// person answers.
    On { trigger_id: String },
    /// Record what each event would do, without doing it.
    DryRun { trigger_id: String },
    /// Ignore events until resumed.
    Pause { trigger_id: String },
    /// Return to the state paused from. Never backfills.
    Resume { trigger_id: String },
    /// Forget a trigger. Work it applied is unchanged.
    Remove { trigger_id: String },
    /// Fire a trigger by hand as one delivery under a retained operation id.
    Fire {
        trigger_id: String,
        #[arg(long)]
        operation_id: String,
        /// An event value for the template's event inputs, as `field=value`
        /// (`commit`, `ci_run`, `ticket`, `branch`, `occurrence`, `gate`).
        #[arg(long = "evidence", value_name = "FIELD=VALUE")]
        evidence: Vec<String>,
    },
    /// Run one trigger pass now, as a server does on its own: due schedules, failed
    /// run gates, queued deliveries and answered turn-Ons.
    Pass,
}

fn read_json(path: &str) -> Result<Value> {
    let text = if path == "-" {
        std::io::read_to_string(std::io::stdin()).context("reading stdin")?
    } else {
        std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?
    };
    match serde_json::from_str(&text).context("the file is not JSON")? {
        Value::Object(object) => Ok(Value::Object(object)),
        _ => bail!("expected a trigger object"),
    }
}

pub(super) fn route(
    project: &str,
    arguments: TriggerArgs,
) -> Result<(&'static str, String, Value)> {
    let base = format!("/checkouts/{project}/orchestration/triggers");
    let one = |operation: &str, trigger_id: String| {
        (
            "POST",
            format!("{base}/{operation}"),
            json!({"trigger_id": trigger_id}),
        )
    };
    Ok(match arguments.command {
        TriggerCmd::List => ("POST", format!("{base}/list"), json!({})),
        TriggerCmd::Show { trigger_id } | TriggerCmd::History { trigger_id } => {
            one("show", trigger_id)
        }
        TriggerCmd::Set { file, start } => {
            let mut body = read_json(&file)?;
            if let Some(start) = start {
                body["start"] = json!(match start {
                    Start::DryRun => "dry_run",
                    Start::Paused => "paused",
                });
            }
            ("POST", format!("{base}/set"), body)
        }
        TriggerCmd::Review { trigger_id, file } => match (trigger_id, file) {
            (Some(trigger_id), None) => one("review", trigger_id),
            (None, Some(file)) => (
                "POST",
                format!("{base}/review"),
                json!({"trigger": read_json(&file)?}),
            ),
            _ => bail!("review names a trigger id or --file"),
        },
        TriggerCmd::On { trigger_id } => one("on", trigger_id),
        TriggerCmd::DryRun { trigger_id } => one("dry_run", trigger_id),
        TriggerCmd::Pause { trigger_id } => one("pause", trigger_id),
        TriggerCmd::Resume { trigger_id } => one("resume", trigger_id),
        TriggerCmd::Remove { trigger_id } => one("remove", trigger_id),
        TriggerCmd::Fire {
            trigger_id,
            operation_id,
            evidence,
        } => {
            let mut values = Map::new();
            for pair in evidence {
                let (field, value) = pair
                    .split_once('=')
                    .ok_or_else(|| anyhow!("--evidence takes field=value, got `{pair}`"))?;
                values.insert(field.to_owned(), Value::String(value.to_owned()));
            }
            (
                "POST",
                format!("{base}/fire"),
                json!({"trigger_id": trigger_id, "operation_id": operation_id,
                    "evidence": values}),
            )
        }
        TriggerCmd::Pass => ("POST", format!("{base}/pass"), json!({})),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Harness {
        #[command(flatten)]
        arguments: TriggerArgs,
    }
    fn request(line: &[&str]) -> Result<(&'static str, String, Value)> {
        let parsed =
            Harness::try_parse_from(std::iter::once("trigger").chain(line.iter().copied()))?;
        route("owner-1", parsed.arguments)
    }

    #[test]
    fn hs2_kxka7x_every_trigger_command_is_one_request_to_the_shared_runtime() {
        let base = "/checkouts/owner-1/orchestration/triggers";
        assert_eq!(
            request(&["list"]).unwrap(),
            ("POST", format!("{base}/list"), json!({}))
        );
        for (command, operation) in [
            ("show", "show"),
            ("history", "show"),
            ("review", "review"),
            ("on", "on"),
            ("dry-run", "dry_run"),
            ("pause", "pause"),
            ("resume", "resume"),
            ("remove", "remove"),
        ] {
            assert_eq!(
                request(&[command, "T1"]).unwrap(),
                (
                    "POST",
                    format!("{base}/{operation}"),
                    json!({"trigger_id":"T1"})
                ),
                "{command}"
            );
        }
        assert_eq!(
            request(&[
                "fire",
                "T1",
                "--operation-id",
                "op-1",
                "--evidence",
                "commit=abc",
                "--evidence",
                "ci_run=9182.1"
            ])
            .unwrap(),
            (
                "POST",
                format!("{base}/fire"),
                json!({"trigger_id":"T1","operation_id":"op-1",
                    "evidence":{"commit":"abc","ci_run":"9182.1"}})
            )
        );
        assert_eq!(
            request(&["pass"]).unwrap(),
            ("POST", format!("{base}/pass"), json!({}))
        );
        // A hand-fired delivery retains an operation id; evidence is field=value.
        assert!(request(&["fire", "T1"]).is_err());
        assert!(request(&["fire", "T1", "--operation-id", "o", "--evidence", "x"]).is_err());
        assert!(request(&["review"]).is_err());
    }

    #[test]
    fn hs2_kxka7x_a_definition_comes_from_a_file_and_starts_dry_run_or_paused() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("trigger.json");
        std::fs::write(
            &file,
            r#"{"name":"Nightly","source":{"kind":"schedule"},"action":{}}"#,
        )
        .unwrap();
        let path = file.to_str().unwrap();
        let (_, route, body) = request(&["set", "--file", path]).unwrap();
        assert!(route.ends_with("/set"));
        assert_eq!(body["name"], "Nightly");
        assert!(body.get("start").is_none());
        let (_, _, body) = request(&["set", "--file", path, "--start", "paused"]).unwrap();
        assert_eq!(body["start"], "paused");
        let (_, route, body) = request(&["review", "--file", path]).unwrap();
        assert!(route.ends_with("/review"));
        assert_eq!(body["trigger"]["name"], "Nightly");
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, "[1]").unwrap();
        assert!(request(&["set", "--file", bad.to_str().unwrap()]).is_err());
    }
}
