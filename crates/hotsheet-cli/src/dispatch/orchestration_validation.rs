//! HS2-WN0MXZ: headless validation steps of an orchestration ticket. Every request
//! goes through the shared orchestration runtime; this file only shapes them and, for
//! `run`, repeats the one request until its attempt is finished.
use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use hotsheet_cli::orchestration::{MAX_VALIDATION_WAIT_MS, OrchestrationRuntime};
use hotsheet_ticketing::orchestration::OrchestrationError;
use serde_json::{Value, json};

#[derive(Args)]
pub(super) struct ValidationArgs {
    /// Registered owning checkout id.
    #[arg(long, global = true)]
    pub(super) project: Option<String>,
    #[command(subcommand)]
    command: ValidationCmd,
}

#[derive(Args)]
struct StepArgs {
    /// The orchestration ticket (slug or id) the step is attached to.
    #[arg(long)]
    ticket: String,
    /// The orchestration ticket's source; omit when the project has one git source.
    #[arg(long, default_value = "")]
    source: String,
    /// The step's name within the ticket.
    #[arg(long)]
    step: String,
}

#[derive(Subcommand)]
enum ValidationCmd {
    /// Attach a validation step to an orchestration ticket, or change its definition.
    /// The same definition again changes nothing; a changed one makes earlier evidence
    /// unverified. Needs a validation_run grant on the target.
    Define {
        #[command(flatten)]
        step: StepArgs,
        /// The declared target project the step runs in.
        #[arg(long)]
        target: String,
        /// A test profile the target project defines in its own
        /// .hotsheet2/validation-profiles.json. Runs without asking a person.
        #[arg(long, conflicts_with = "program", required_unless_present = "program")]
        profile: Option<String>,
        /// A program to run in the target checkout instead of a profile (no shell).
        /// Every run of such a step waits for a person's approval.
        #[arg(long)]
        program: Option<String>,
        /// One argument of --program. Repeatable, in order.
        #[arg(long = "arg", allow_hyphen_values = true, requires = "program")]
        args: Vec<String>,
        /// A step of the same ticket that must have passed first. Repeatable.
        #[arg(long = "requires")]
        requires: Vec<String>,
        /// The exit code that means success (default 0).
        #[arg(long, allow_hyphen_values = true)]
        expect_exit: Option<i32>,
        /// Also require this SHA-256 of the step's standard output.
        #[arg(long)]
        expect_stdout_sha256: Option<String>,
        /// 1 to 3600 (default 300).
        #[arg(long)]
        timeout_seconds: Option<u32>,
        /// Attempts one run may make while the step fails, 1 to 5 (default 1).
        #[arg(long)]
        max_attempts: Option<u32>,
    },
    /// The latest evidence of every step of a ticket, or of the whole project.
    List {
        #[arg(long)]
        ticket: Option<String>,
        #[arg(long, default_value = "")]
        source: String,
    },
    /// The latest evidence of one step and its retained attempts. Contacts nothing.
    Show {
        #[command(flatten)]
        step: StepArgs,
    },
    /// Run a step in its target and wait for the result. Repeating the command with
    /// the same --operation-id continues that request and never runs a finished one
    /// again. The result is in the JSON (`result.state`), not in the exit status.
    Run {
        #[command(flatten)]
        step: StepArgs,
        #[arg(long)]
        operation_id: String,
        /// The dispatch run this validation belongs to; its events then report it.
        #[arg(long)]
        run: Option<String>,
        /// Start (or look once) and return at once instead of waiting.
        #[arg(long)]
        no_wait: bool,
        /// HS2-ZYM6ZT: the person whose request this carries out, recorded in the
        /// audit beside the acting identity. Attribution only: the gate still decides
        /// for --actor-id.
        #[arg(long)]
        requested_by: Option<String>,
    },
    /// Stop a step's running attempt (HS2-17ABM2). It only lowers activity, so it
    /// never asks a person. The attempt ends `cancelled`, unless the step ended by
    /// itself first; a finished attempt is answered as recorded.
    Cancel {
        #[command(flatten)]
        step: StepArgs,
        /// The request to stop; omit for whichever attempt is running.
        #[arg(long)]
        operation_id: Option<String>,
        /// Return at once instead of waiting for the stopped attempt to end.
        #[arg(long)]
        no_wait: bool,
    },
    /// List and check the test profiles a checkout offers in its checked-in
    /// .hotsheet2/validation-profiles.json (HS2-YYNW9F). Without --target, the
    /// --project checkout's own file; with it, a declared target's file, read through
    /// the gate as an observation. Exits non-zero when the file has problems.
    Profiles {
        /// A declared target project of the --project orchestration project.
        #[arg(long)]
        target: Option<String>,
    },
    /// HS2-ZCR3Y7: print a kept copy of a step's output stream by its SHA-256, from
    /// the --project checkout this machine hosts. Only small streams are kept, only on
    /// the machine that ran the step, and only for 14 days.
    Output {
        /// The stream's SHA-256, as a result records it.
        sha256: String,
    },
    /// Remove a step and its retained results.
    Remove {
        #[command(flatten)]
        step: StepArgs,
    },
}

/// The private body member that asks [`until_finished`] to repeat the request.
const WAIT: &str = "until_finished";

pub(super) fn route(
    project: &str,
    arguments: ValidationArgs,
) -> Result<(&'static str, String, Value)> {
    let base = format!("/checkouts/{project}/orchestration/validations");
    let ticket = |ticket: &str, source: &str| json!({"connection_id": source, "native_id": ticket});
    let of = |step: &StepArgs| json!({"ticket": ticket(&step.ticket, &step.source), "step_id": step.step});
    Ok(match arguments.command {
        ValidationCmd::Define {
            step,
            target,
            profile,
            program,
            args,
            requires,
            expect_exit,
            expect_stdout_sha256,
            timeout_seconds,
            max_attempts,
        } => {
            let check = match (profile, program) {
                (Some(name), None) => json!({"kind": "profile", "name": name}),
                (None, Some(program)) => {
                    json!({"kind": "command", "program": program, "args": args})
                }
                _ => bail!("give exactly one of --profile or --program"),
            };
            let mut definition = json!({"step_id": step.step, "target_id": target, "check": check,
                "prerequisites": requires});
            let mut evidence = serde_json::Map::new();
            if let Some(code) = expect_exit {
                evidence.insert("exit_code".into(), code.into());
            }
            if let Some(digest) = expect_stdout_sha256 {
                evidence.insert("stdout_sha256".into(), digest.into());
            }
            if !evidence.is_empty() {
                definition["evidence"] = Value::Object(evidence);
            }
            if let Some(seconds) = timeout_seconds {
                definition["timeout_seconds"] = seconds.into();
            }
            if let Some(attempts) = max_attempts {
                definition["retry"] = json!({"max_attempts": attempts});
            }
            (
                "POST",
                format!("{base}/define"),
                json!({"ticket": ticket(&step.ticket, &step.source), "step": definition}),
            )
        }
        ValidationCmd::List {
            ticket: named,
            source,
        } => (
            "POST",
            format!("{base}/list"),
            json!({"ticket": named.map(|named| ticket(&named, &source))}),
        ),
        ValidationCmd::Show { step } => ("POST", format!("{base}/evidence"), of(&step)),
        ValidationCmd::Remove { step } => ("POST", format!("{base}/remove"), of(&step)),
        ValidationCmd::Output { sha256 } => (
            "GET",
            format!("/checkouts/{project}/target-validation-output/{sha256}"),
            Value::Null,
        ),
        ValidationCmd::Profiles { target: None } => (
            "GET",
            format!("/checkouts/{project}/target-validation-profiles"),
            Value::Null,
        ),
        ValidationCmd::Profiles {
            target: Some(target),
        } => (
            "POST",
            format!("{base}/profiles"),
            json!({"target_id": target}),
        ),
        ValidationCmd::Cancel {
            step,
            operation_id,
            no_wait,
        } => {
            let mut body = of(&step);
            if let Some(operation_id) = operation_id {
                body["operation_id"] = operation_id.into();
            }
            // HS2-5EZZPH: like `run`, a waiting cancel repeats its bounded wait until
            // the host confirms the stop; one 8-second wait is not the end of it.
            body[WAIT] = (!no_wait).into();
            ("POST", format!("{base}/cancel"), body)
        }
        ValidationCmd::Run {
            step,
            operation_id,
            run,
            no_wait,
            requested_by,
        } => {
            let mut body = of(&step);
            body["operation_id"] = operation_id.into();
            body["run_id"] = run.into();
            if let Some(person) = requested_by {
                body["requested_by"] = person.into();
            }
            body[WAIT] = (!no_wait).into();
            ("POST", format!("{base}/run"), body)
        }
    })
}

/// Send a `run` request and, unless `--no-wait` was given, repeat it until its attempt
/// is finished and the retry policy has nothing left to try. Each repeat is one bounded
/// wait held open on the host; nothing sleeps between them. An answer that could not
/// confirm the target is returned as it is, so an unreachable target never spins.
/// HS2-5EZZPH: a `cancel` repeats the same way until the stopped attempt has ended
/// (it is no longer `running`); a cancel is answered as recorded once it has.
/// `None` means the body was not a validation run or cancel.
pub(super) fn until_finished(
    runtime: &OrchestrationRuntime,
    path: &str,
    body: &mut Value,
) -> Option<Result<Value, OrchestrationError>> {
    let wait = body.as_object_mut()?.remove(WAIT)? == true;
    if wait {
        body["wait_ms"] = MAX_VALIDATION_WAIT_MS.into();
    }
    Some((|| {
        loop {
            let view = runtime.request_now("POST", path, body)?;
            let state = view["result"]["state"].as_str().unwrap_or_default();
            let retries = !path.ends_with("/cancel")
                && state == "failed"
                && view["attempts_left"].as_u64().unwrap_or(0) > 0;
            let more = state == "running" || retries;
            if !wait || !more || view.get("unverified").is_some() {
                return Ok(view);
            }
        }
    })())
}
