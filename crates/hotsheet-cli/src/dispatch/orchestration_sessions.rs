//! HS2-GDJXGZ: headless start, list, safe-point check, stop and takeover of the AI
//! sessions an orchestration project owns. Every request goes through the shared
//! orchestration runtime; this file only shapes them.
use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use serde_json::{Value, json};

#[derive(Args)]
pub(super) struct SessionArgs {
    /// Registered owning checkout id.
    #[arg(long, global = true)]
    project: Option<String>,
    /// A checkout this machine hosts: `list` shows the sessions orchestrators started
    /// on it, as this machine's own terminal host sees them, and `takeover`, `pause`
    /// and `stop` act on one of them by its hosted operation id (HS2-6MNMVB).
    #[arg(long, global = true, conflicts_with = "project")]
    checkout: Option<String>,
    #[command(subcommand)]
    command: SessionCmd,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)] // HS2-7YSKXE: parsed once per process; `start` carries its flags inline for clap
enum SessionCmd {
    /// Start an AI session on the server that hosts a target. Repeating the command
    /// with the same --operation-id returns the same session and never launches twice.
    Start {
        #[arg(long)]
        target: String,
        #[arg(long)]
        operation_id: String,
        /// The AI tool plugin id, such as claude or codex.
        #[arg(long)]
        tool: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
        /// Required when the policy configures accounts for the tool.
        #[arg(long)]
        account: Option<String>,
        /// worker (default), dispatcher or integrator.
        #[arg(long)]
        role: Option<String>,
        /// The run the session is started for; its tab is labelled with it.
        #[arg(long)]
        run: Option<String>,
        /// The lost or failed session this one replaces.
        #[arg(long)]
        replaces: Option<String>,
        /// HS2-7YSKXE: a resource the work needs, as `<namespace>/<resource>`. The
        /// session is told to acquire it itself. Repeatable, at most four.
        #[arg(long = "resource")]
        resources: Vec<String>,
    },
    /// Sessions this project started, from this machine's records; --active leaves out
    /// finished ones. With --checkout, the sessions orchestrators started on a checkout
    /// this machine hosts.
    List {
        #[arg(long)]
        active: bool,
        /// With --checkout: include each session's safe point.
        #[arg(long)]
        safe_point: bool,
    },
    /// One orchestrator-owned session as this machine recorded it; contacts no host.
    Show { session_id: String },
    /// Read one session, or every unfinished session without an id, from its host and
    /// bring the record up to date.
    Sync { session_id: Option<String> },
    /// Whether the session may be wound down now, as its host established it.
    SafePoint { session_id: String },
    /// Tell a running session which exact ticket its run is for, through its host's
    /// session push (HS2-R7QH35). A tool that is not ready gets it when it becomes so.
    Request { session_id: String },
    /// Stop a session. An orchestrator stops only its own sessions at a safe point;
    /// --force is a person's request (pass --actor-role human). With --checkout, the
    /// argument is the hosted operation id and this machine checks the safe point.
    Stop {
        session_id: String,
        /// Stop even when the session is not at a safe point.
        #[arg(long)]
        force: bool,
    },
    /// Take a session over (a person's action; pass --actor-role human). The
    /// orchestrator treats it as human-owned from then on. With --checkout, the
    /// argument is the hosted operation id, recorded at this machine.
    Takeover { session_id: String },
    /// Take a session over and pause it: it keeps running and gets nothing new.
    /// With --checkout, the argument is the hosted operation id.
    Pause { session_id: String },
}

/// The owning (or hosted) checkout id and the request for the shared runtime.
pub(super) fn route(arguments: SessionArgs) -> Result<(String, (&'static str, String, Value))> {
    if let Some(checkout) = arguments.checkout {
        // HS2-6MNMVB: the hosting machine's own half, by hosted operation id.
        let path = format!("/checkouts/{checkout}/target-sessions");
        let request = match arguments.command {
            SessionCmd::List { safe_point, .. } => {
                let query = if safe_point { "?safe_point=true" } else { "" };
                ("GET", format!("{path}{query}"), Value::Null)
            }
            SessionCmd::Takeover { session_id } => (
                "PATCH",
                path,
                json!({"operation_id": session_id, "pause": false}),
            ),
            SessionCmd::Pause { session_id } => (
                "PATCH",
                path,
                json!({"operation_id": session_id, "pause": true}),
            ),
            SessionCmd::Stop { session_id, force } => (
                "DELETE",
                path,
                json!({"operation_id": session_id, "force": force}),
            ),
            _ => bail!("--checkout applies to `session list`, `takeover`, `pause` and `stop` only"),
        };
        return Ok((checkout, request));
    }
    let project = arguments
        .project
        .ok_or_else(|| anyhow::anyhow!("--project <registered-checkout-id> is required"))?;
    let base = format!("/checkouts/{project}/orchestration/sessions");
    let request = match arguments.command {
        SessionCmd::Start {
            target,
            operation_id,
            tool,
            model,
            effort,
            account,
            role,
            run,
            replaces,
            resources,
        } => {
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
            (
                "POST",
                base,
                json!({"target_id": target, "operation_id": operation_id, "tool": tool,
                    "model": model, "effort": effort, "account": account, "role": role,
                    "run_id": run, "replaces": replaces, "resources": needs}),
            )
        }
        SessionCmd::List { active, .. } => (
            "GET",
            format!("{base}{}", if active { "?active=true" } else { "" }),
            Value::Null,
        ),
        SessionCmd::Show { session_id } => ("GET", format!("{base}/{session_id}"), Value::Null),
        SessionCmd::Sync {
            session_id: Some(session_id),
        } => ("POST", format!("{base}/{session_id}/sync"), json!({})),
        SessionCmd::Sync { session_id: None } => (
            "POST",
            format!("/checkouts/{project}/orchestration/session-sync"),
            json!({}),
        ),
        SessionCmd::SafePoint { session_id } => {
            ("POST", format!("{base}/{session_id}/safe-point"), json!({}))
        }
        SessionCmd::Request { session_id } => {
            ("POST", format!("{base}/{session_id}/request"), json!({}))
        }
        SessionCmd::Stop { session_id, force } => (
            "POST",
            format!("{base}/{session_id}/stop"),
            json!({"force": force}),
        ),
        SessionCmd::Takeover { session_id } => (
            "POST",
            format!("{base}/{session_id}/takeover"),
            json!({"pause": false}),
        ),
        SessionCmd::Pause { session_id } => (
            "POST",
            format!("{base}/{session_id}/takeover"),
            json!({"pause": true}),
        ),
    };
    Ok((project, request))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Harness {
        #[command(flatten)]
        arguments: SessionArgs,
    }
    fn request(line: &[&str]) -> Result<(String, (&'static str, String, Value))> {
        let parsed =
            Harness::try_parse_from(std::iter::once("session").chain(line.iter().copied()))?;
        route(parsed.arguments)
    }
    const SESSION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FB0";

    #[test]
    fn hs2_gdjxgz_every_session_command_is_one_request_to_the_shared_runtime() {
        let base = "/checkouts/owner-1/orchestration";
        let cases: [(&[&str], &str, String, Value); 11] = [
            (
                &[
                    "start",
                    "--target",
                    "T",
                    "--operation-id",
                    "op-1",
                    "--tool",
                    "codex",
                    "--model",
                    "model-a",
                    "--effort",
                    "high",
                    "--account",
                    "team",
                    "--role",
                    "integrator",
                    "--run",
                    "R",
                    "--replaces",
                    "S",
                    "--resource",
                    "vm/macos-vm",
                ],
                "POST",
                format!("{base}/sessions"),
                json!({"target_id":"T","operation_id":"op-1","tool":"codex","model":"model-a",
                    "effort":"high","account":"team","role":"integrator","run_id":"R",
                    "replaces":"S",
                    "resources":[{"namespace":"vm","resource":"macos-vm"}]}),
            ),
            (&["list"], "GET", format!("{base}/sessions"), Value::Null),
            (
                &["list", "--active"],
                "GET",
                format!("{base}/sessions?active=true"),
                Value::Null,
            ),
            (
                &["show", SESSION],
                "GET",
                format!("{base}/sessions/{SESSION}"),
                Value::Null,
            ),
            (&["sync"], "POST", format!("{base}/session-sync"), json!({})),
            (
                &["sync", SESSION],
                "POST",
                format!("{base}/sessions/{SESSION}/sync"),
                json!({}),
            ),
            (
                &["safe-point", SESSION],
                "POST",
                format!("{base}/sessions/{SESSION}/safe-point"),
                json!({}),
            ),
            // HS2-R7QH35
            (
                &["request", SESSION],
                "POST",
                format!("{base}/sessions/{SESSION}/request"),
                json!({}),
            ),
            (
                &["stop", SESSION, "--force"],
                "POST",
                format!("{base}/sessions/{SESSION}/stop"),
                json!({"force": true}),
            ),
            (
                &["takeover", SESSION],
                "POST",
                format!("{base}/sessions/{SESSION}/takeover"),
                json!({"pause": false}),
            ),
            (
                &["pause", SESSION],
                "POST",
                format!("{base}/sessions/{SESSION}/takeover"),
                json!({"pause": true}),
            ),
        ];
        for (line, method, path, body) in cases {
            let mut arguments = line.to_vec();
            arguments.extend(["--project", "owner-1"]);
            let (owner, request) = request(&arguments).unwrap();
            assert_eq!(owner, "owner-1", "{line:?}");
            assert_eq!(request, (method, path, body), "{line:?}");
        }
        // A plain stop does not force.
        let stop = request(&["stop", SESSION, "--project", "owner-1"]).unwrap();
        assert_eq!(stop.1.2, json!({"force": false}));
        // An unset optional start field is sent as null, never as an empty string.
        let start = request(&[
            "start",
            "--target",
            "T",
            "--operation-id",
            "op",
            "--tool",
            "codex",
            "--project",
            "owner-1",
        ])
        .unwrap();
        assert_eq!(start.1.2["role"], Value::Null);
        assert_eq!(start.1.2["run_id"], Value::Null);
    }

    #[test]
    fn hs2_gdjxgz_hosted_commands_need_no_owning_project_and_the_rest_do() {
        let hosted = request(&["list", "--checkout", "hosted-1"]).unwrap();
        assert_eq!(
            hosted,
            (
                "hosted-1".into(),
                (
                    "GET",
                    "/checkouts/hosted-1/target-sessions".into(),
                    Value::Null
                )
            )
        );
        let with_point = request(&["list", "--checkout", "hosted-1", "--safe-point"]).unwrap();
        assert_eq!(
            with_point.1.1,
            "/checkouts/hosted-1/target-sessions?safe_point=true"
        );
        for refused in [
            &["sync", "--checkout", "hosted-1"][..],
            &["show", SESSION, "--checkout", "hosted-1"],
            &["safe-point", SESSION, "--checkout", "hosted-1"],
            &["list"],
            &["show", SESSION],
            &["list", "--checkout", "hosted-1", "--project", "owner-1"],
            &["start", "--project", "owner-1", "--tool", "codex"],
        ] {
            assert!(request(refused).is_err(), "{refused:?}");
        }
    }

    /// HS2-6MNMVB: a person at the hosting machine takes over, pauses or stops a
    /// session by its hosted operation id.
    #[test]
    fn hs2_6mnmvb_the_hosting_machine_takes_over_pauses_and_stops_by_operation() {
        let path = "/checkouts/hosted-1/target-sessions".to_owned();
        for (arguments, method, body) in [
            (
                &["takeover", "sched-op", "--checkout", "hosted-1"][..],
                "PATCH",
                json!({"operation_id": "sched-op", "pause": false}),
            ),
            (
                &["pause", "sched-op", "--checkout", "hosted-1"],
                "PATCH",
                json!({"operation_id": "sched-op", "pause": true}),
            ),
            (
                &["stop", "sched-op", "--checkout", "hosted-1"],
                "DELETE",
                json!({"operation_id": "sched-op", "force": false}),
            ),
            (
                &["stop", "sched-op", "--checkout", "hosted-1", "--force"],
                "DELETE",
                json!({"operation_id": "sched-op", "force": true}),
            ),
        ] {
            assert_eq!(
                request(arguments).unwrap(),
                ("hosted-1".into(), (method, path.clone(), body)),
                "{arguments:?}"
            );
        }
    }
}
