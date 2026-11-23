//! HS2-42TARA: the headless plan of prerequisites and its readiness. Every request
//! goes through the shared orchestration runtime; this file only shapes them.
use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde_json::{Value, json};

#[derive(Args)]
pub(super) struct ReadinessArgs {
    /// Registered owning checkout id.
    #[arg(long, global = true)]
    pub(super) project: Option<String>,
    /// HS2-190RDX: act on this plan update's draft instead of the live plan
    /// (define, show, plan and remove only).
    #[arg(long, global = true)]
    pub(super) plan_update: Option<String>,
    #[command(subcommand)]
    command: ReadinessCmd,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)] // HS2-2FG91X: parsed once per process; `define` carries its flags inline for clap
enum ReadinessCmd {
    /// Plan one orchestration ticket's work on one exact target ticket, with what must
    /// hold before it is dispatched. Every named ticket is read through the gate
    /// (tickets_read) and stored by its canonical id. The same definition again
    /// changes nothing; one step per target ticket.
    Define {
        /// The orchestration ticket (slug or id) the work is for.
        #[arg(long)]
        ticket: String,
        /// The orchestration ticket's source; omit when the project has one git source.
        #[arg(long, default_value = "")]
        source: String,
        /// The declared target project.
        #[arg(long)]
        target: String,
        /// The target's ticket source.
        #[arg(long)]
        connection: String,
        /// The target ticket the step works (slug or id).
        #[arg(long)]
        id: String,
        /// A qualified target ticket that must be confirmed finished first, as
        /// `<target-id>/<connection>:<ticket>`. Repeatable.
        #[arg(long = "requires")]
        requires: Vec<String>,
        /// Which gates of every --requires ticket must be green: none, ci (the
        /// default) or deployment.
        #[arg(long, default_value = "ci")]
        gates: String,
        /// A validation step of the same orchestration ticket whose latest evidence
        /// must be passed. Repeatable.
        #[arg(long = "requires-validation")]
        requires_validation: Vec<String>,
        /// One prerequisite as JSON, for what the flags do not say, such as a pinned
        /// commit: {"kind":"ticket","target_id":..,"connection_id":..,"native_id":..,
        /// "gates":"ci","commit":"<40-hex>"}. Repeatable.
        #[arg(long = "prerequisite")]
        prerequisites: Vec<String>,
        /// HS2-2FG91X: a resource the session working this step must hold, as
        /// `<namespace>/<resource>` in the resource-claim service's names (for
        /// example `vm/macos-vm`). Repeatable, at most four. The scheduler does not
        /// start the work while someone holds it and never claims it itself.
        #[arg(long = "resource")]
        resources: Vec<String>,
        /// HS2-HCW7MD: the AI tool preferred for this work, by plugin id. The
        /// scheduler tries it first; another allowed tool takes the work when it
        /// cannot start (HS2-TH53NN).
        #[arg(long)]
        tool: Option<String>,
        /// HS2-TH53NN: the model preferred for this work, a preference like `--tool`.
        #[arg(long)]
        model: Option<String>,
        /// HS2-ZY4Z4G: what the target's machine must be for this work, as
        /// `os=<macos|linux|windows>`, `arch=<x86_64|arm64>`, `tool=<plugin id>`,
        /// `toolchain=<name>`, `min_memory_gb=<n>` or `gpu=<any|vendor>`. Repeatable;
        /// several `os` or `arch` values are alternatives, several tools or
        /// toolchains are all needed. The scheduler places the work only where every
        /// requirement is met.
        #[arg(long = "require")]
        require: Vec<String>,
        /// HS2-YDY5HJ: the revision of this ticket's step you read (0 when none is
        /// planned). A step changed since, here or on another machine, is refused
        /// with plan_conflict instead of overwritten.
        #[arg(long)]
        expected_revision: Option<u32>,
    },
    /// One step's level and the reason for each prerequisite that does not hold.
    /// Answers from this machine's records and contacts nothing.
    Show { step_id: String },
    /// Every step with its level: confirmed, provisional, blocked, stale, cyclic,
    /// in_progress or finished, with counts and the cycles found. Contacts nothing.
    Plan,
    /// The work that could be started now, confirmed first, with what each item
    /// unblocks and the target ticket's priority inputs. Contacts nothing.
    Ready,
    /// Read again the tickets a step, or the whole plan, names (tickets_read). A
    /// ticket that cannot be confirmed is listed under `unverified` and is stale.
    Refresh { step_id: Option<String> },
    /// Withdraw a step from the plan: from the checked-in plan, and this machine's
    /// reads and level for it.
    Remove { step_id: String },
    /// HS2-YDY5HJ: move the plan definitions this machine kept before the plan was
    /// checked in into `.hotsheet2/orchestration-plan.json`, merged by step id.
    /// Repeating it changes nothing. A definition that conflicts with a checked-in one
    /// is refused (plan_conflict) unless a person passes --discard-local, which drops
    /// the conflicting local definitions. Levels and reads stay on this machine.
    Migrate {
        #[arg(long)]
        discard_local: bool,
    },
    /// HS2-F1BKQ0: the plan's state where every plan read fails: ok, a conflict with the
    /// step ids involved and whether a person's --discard-local would resolve it, or
    /// which file (checked_in or local) is damaged. Reads only.
    Health,
    /// HS2-JSF985: which work was started ahead of a pending gate, on which gate,
    /// and what became of it: provisional, confirmed, at_risk or reopened, with the
    /// prerequisites to repair first and the gates to re-evaluate. Contacts nothing.
    Speculation,
    /// HS2-JSF985: record that the fix of a failed prerequisite invalidated a step's
    /// provisional work. The record is kept and the run on the target is not touched.
    Reopen {
        step_id: String,
        /// A stable reason code, never free text.
        #[arg(long)]
        reason: String,
    },
}

fn required_ticket(spec: &str, gates: &str) -> Result<Value> {
    let parsed = spec
        .split_once('/')
        .and_then(|(target, rest)| Some((target, rest.split_once(':')?)));
    let Some((target, (connection, native))) = parsed.filter(|(target, (connection, native))| {
        !target.is_empty() && !connection.is_empty() && !native.is_empty()
    }) else {
        bail!("--requires takes <target-id>/<connection>:<ticket>");
    };
    Ok(
        json!({"kind": "ticket", "target_id": target, "connection_id": connection,
        "native_id": native, "gates": gates}),
    )
}

pub(super) fn route(
    project: &str,
    arguments: ReadinessArgs,
) -> Result<(&'static str, String, Value)> {
    let base = format!("/checkouts/{project}/orchestration/readiness");
    let drafts = matches!(
        arguments.command,
        ReadinessCmd::Define { .. }
            | ReadinessCmd::Show { .. }
            | ReadinessCmd::Plan
            | ReadinessCmd::Remove { .. }
    );
    if arguments.plan_update.is_some() && !drafts {
        bail!("--plan-update applies to define, show, plan and remove only");
    }
    let plan_update = arguments.plan_update;
    let (method, path, mut body) = match arguments.command {
        ReadinessCmd::Define {
            ticket,
            source,
            target,
            connection,
            id,
            requires,
            gates,
            requires_validation,
            prerequisites,
            resources,
            tool,
            model,
            require,
            expected_revision,
        } => {
            let mut requirements = hotsheet_ticketing::orchestration::StepRequirements::default();
            for spec in &require {
                requirements
                    .add(spec)
                    .map_err(|error| anyhow::anyhow!("--require {spec}: {error}"))?;
            }
            let mut needs = Vec::new();
            for spec in &resources {
                let Some((namespace, resource)) = spec
                    .split_once('/')
                    .filter(|(namespace, resource)| !namespace.is_empty() && !resource.is_empty())
                else {
                    bail!("--resource takes <namespace>/<resource>");
                };
                needs.push(json!({"namespace": namespace, "resource": resource}));
            }
            let mut all = Vec::new();
            for spec in &requires {
                all.push(required_ticket(spec, &gates)?);
            }
            for step in requires_validation {
                all.push(json!({"kind": "validation", "step_id": step}));
            }
            for raw in &prerequisites {
                all.push(
                    serde_json::from_str::<Value>(raw)
                        .context("--prerequisite takes one JSON object")?,
                );
            }
            (
                "POST",
                format!("{base}/define"),
                json!({"ticket": {"connection_id": source, "native_id": ticket},
                    "target_id": target, "connection_id": connection, "native_id": id,
                    "prerequisites": all, "resources": needs, "tool": tool, "model": model,
                    "requirements": requirements, "expected_revision": expected_revision}),
            )
        }
        ReadinessCmd::Show { step_id } => {
            ("POST", format!("{base}/show"), json!({"step_id": step_id}))
        }
        ReadinessCmd::Plan => ("POST", format!("{base}/plan"), json!({})),
        ReadinessCmd::Ready => ("POST", format!("{base}/ready"), json!({})),
        ReadinessCmd::Refresh { step_id } => (
            "POST",
            format!("{base}/refresh"),
            json!({"step_id": step_id}),
        ),
        ReadinessCmd::Remove { step_id } => (
            "POST",
            format!("{base}/remove"),
            json!({"step_id": step_id}),
        ),
        ReadinessCmd::Migrate { discard_local } => (
            "POST",
            format!("{base}/migrate"),
            json!({"discard_local": discard_local}),
        ),
        ReadinessCmd::Speculation => ("POST", format!("{base}/speculation"), json!({})),
        ReadinessCmd::Health => ("POST", format!("{base}/health"), json!({})),
        ReadinessCmd::Reopen { step_id, reason } => (
            "POST",
            format!("{base}/reopen"),
            json!({"step_id": step_id, "reason": reason}),
        ),
    };
    if let (Some(update), Some(object)) = (plan_update, body.as_object_mut()) {
        object.insert("plan_update".into(), Value::String(update));
    }
    Ok((method, path, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        command: ReadinessCmd,
    }

    fn define(extra: &[&str]) -> Result<Value> {
        let base = [
            "readiness",
            "define",
            "--ticket",
            "OWN-1",
            "--target",
            "t1",
            "--connection",
            "git",
            "--id",
            "TGT-1",
        ];
        let parsed = Cli::try_parse_from(base.iter().chain(extra.iter()).copied())
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let (_, _, body) = route(
            "owner",
            ReadinessArgs {
                project: None,
                plan_update: None,
                command: parsed.command,
            },
        )?;
        Ok(body)
    }

    /// HS2-190RDX: `--plan-update` sends the draft's id with define, show, plan and
    /// remove, and is refused for anything else.
    #[test]
    fn hs2_190rdx_plan_update_names_the_draft() {
        #[derive(Parser)]
        struct Full {
            #[command(flatten)]
            arguments: ReadinessArgs,
        }
        let route_of = |line: &[&str]| {
            let parsed =
                Full::try_parse_from(std::iter::once("readiness").chain(line.iter().copied()))
                    .map_err(|error| anyhow::anyhow!("{error}"))?;
            route("owner", parsed.arguments)
        };
        for line in [
            &["plan", "--plan-update", "u-1"][..],
            &["remove", "S1", "--plan-update", "u-1"],
            &["show", "S1", "--plan-update", "u-1"],
            &[
                "define",
                "--ticket",
                "OWN-1",
                "--target",
                "t1",
                "--connection",
                "git",
                "--id",
                "TGT-1",
                "--plan-update",
                "u-1",
            ],
        ] {
            assert_eq!(
                route_of(line).unwrap().2["plan_update"],
                json!("u-1"),
                "{line:?}"
            );
        }
        assert_eq!(route_of(&["plan"]).unwrap().2, json!({}));
        assert!(route_of(&["ready", "--plan-update", "u-1"]).is_err());
    }

    /// HS2-ZY4Z4G: `--require` builds the step's requirements; a malformed one is
    /// refused before any request is made.
    #[test]
    fn hs2_zy4z4g_define_requires_what_the_machine_must_be() {
        let body = define(&[
            "--require",
            "os=windows",
            "--require",
            "arch=arm64",
            "--require",
            "tool=codex",
            "--require",
            "toolchain=cargo",
            "--require",
            "min_memory_gb=16",
            "--require",
            "gpu=any",
        ])
        .unwrap();
        assert_eq!(
            body["requirements"],
            json!({"os": ["windows"], "arch": ["arm64"], "tools": ["codex"],
                "toolchains": ["cargo"], "min_memory_gb": 16, "gpu": "any"})
        );
        assert_eq!(define(&[]).unwrap()["requirements"], json!({}));
        for bad in ["os=beos", "cpu=fast", "min_memory_gb=lots", "os"] {
            assert!(define(&["--require", bad]).is_err(), "{bad} refused");
        }
    }
}
