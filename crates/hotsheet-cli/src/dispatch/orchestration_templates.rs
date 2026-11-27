//! HS2-A4RVYA: the headless plan template surface. Every request goes through the
//! shared orchestration runtime; this file only shapes them.
use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use serde_json::{Map, Value, json};

#[derive(Args)]
pub(super) struct TemplateArgs {
    /// Registered checkout id of the orchestration project.
    #[arg(long, global = true)]
    pub(super) project: Option<String>,
    #[command(subcommand)]
    command: TemplateCmd,
}

/// Inputs and step choices of a preview or an apply.
#[derive(Args)]
struct ApplyInputs {
    /// One input as `key=value`. A value that reads as JSON (`true`, `3`, `"x"`) is
    /// taken as that value; anything else is text. Repeat for each input.
    #[arg(long = "input", value_name = "KEY=VALUE")]
    input: Vec<String>,
    /// Every input as one JSON object; `--input` values are added on top.
    #[arg(long, value_name = "JSON")]
    inputs: Option<String>,
    /// Always create a new ticket for this step instead of searching first.
    #[arg(long = "create", value_name = "STEP")]
    create: Vec<String>,
}

#[derive(Subcommand)]
enum TemplateCmd {
    /// The templates checked in at .hotsheet2/orchestration-templates.json and this
    /// machine's recent applies.
    List,
    /// One template by id.
    Show { template_id: String },
    /// Check a template definition (a JSON file, `-` for stdin) without writing it.
    Validate {
        #[arg(long)]
        file: String,
    },
    /// Add or replace templates from a JSON file (`-` for stdin): one template, an
    /// object with `templates`, or a whole templates file. Merged by template_id; a
    /// changed definition gets the next version, and the same one changes nothing.
    Set {
        #[arg(long)]
        file: String,
        /// Refuse the write when the file changed since this revision was read.
        #[arg(long)]
        expected_revision: Option<String>,
    },
    /// Retire a template: it can no longer be applied; applied work is unchanged.
    Retire { template_id: String },
    /// Save a goal's current plan steps as a new template.
    SaveGoal {
        /// The goal ticket (slug or id).
        #[arg(long)]
        goal: String,
        /// The goal ticket's source; omit when the project has one git source.
        #[arg(long, default_value = "")]
        source: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        /// Make each project the steps name an input, defaulting to that project.
        #[arg(long)]
        targets_as_inputs: bool,
    },
    /// What applying would do: the inputs as resolved, the problems, the steps, the
    /// projects with what asks there, and whether the planner proposes or updates.
    /// Writes nothing and asks nobody.
    Preview {
        template_id: String,
        #[command(flatten)]
        inputs: ApplyInputs,
    },
    /// Apply a template: open a new goal ticket with the template's brief and hand it
    /// to the planner (Propose Plan, or Update Plan once the plan has steps). The same
    /// --operation-id is the same apply: repeating it never opens a second goal or
    /// starts a second planner. An AI actor first asks through the permission card
    /// and is answered permission_required until a person answers.
    Apply {
        template_id: String,
        #[command(flatten)]
        inputs: ApplyInputs,
        /// The AI tool a new planner runs in, by plugin id.
        #[arg(long)]
        tool: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
        /// A retained id (at most 140 characters).
        #[arg(long)]
        operation_id: String,
    },
    /// One apply on this machine by its operation id.
    ApplyShow { operation_id: String },
    /// HS2-36XDDX: accept the plan the planner proposed for an apply's goal, as a
    /// person reviewing it. With the template's Wait for Accept on, the goal's planned
    /// steps wait for this before placement starts them; steps planned later wait for
    /// the next accept. A person's action: an AI actor is refused. HS2-0YHA2S: with
    /// `--all` instead of an operation id, every template plan held now.
    Accept {
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        operation_id: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// HS2-AMJNER: continue every apply that waits and can go on now (a person
    /// answered its permission request, or the planner is free), under its own
    /// operation id and as the actor who applied it. A server does this on its own.
    Resume,
}

fn read_json(path: &str) -> Result<Value> {
    let text = if path == "-" {
        std::io::read_to_string(std::io::stdin()).context("reading stdin")?
    } else {
        std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?
    };
    serde_json::from_str(&text).context("the file is not JSON")
}

/// The templates a file holds: one template, `{templates: [...]}` or a whole file.
fn templates_of(value: Value) -> Result<Vec<Value>> {
    match value {
        Value::Object(mut object) if object.contains_key("templates") => {
            match object.remove("templates") {
                Some(Value::Array(templates)) => Ok(templates),
                _ => bail!("`templates` must be an array"),
            }
        }
        Value::Object(_) => Ok(vec![value]),
        _ => bail!("expected a template object or an object with `templates`"),
    }
}

fn shaped_inputs(inputs: ApplyInputs) -> Result<(Value, Value)> {
    let mut values = match inputs.inputs {
        Some(text) => match serde_json::from_str(&text).context("--inputs is not JSON")? {
            Value::Object(object) => object,
            _ => bail!("--inputs must be a JSON object"),
        },
        None => Map::new(),
    };
    for pair in inputs.input {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| anyhow!("--input takes key=value, got `{pair}`"))?;
        let value = serde_json::from_str::<Value>(value)
            .ok()
            .filter(|value| !value.is_object() && !value.is_array())
            .unwrap_or_else(|| Value::String(value.to_owned()));
        values.insert(key.to_owned(), value);
    }
    let choices: Map<String, Value> = inputs
        .create
        .into_iter()
        .map(|step| (step, json!("create")))
        .collect();
    Ok((Value::Object(values), Value::Object(choices)))
}

pub(super) fn route(
    project: &str,
    arguments: TemplateArgs,
) -> Result<(&'static str, String, Value)> {
    let base = format!("/checkouts/{project}/orchestration/templates");
    Ok(match arguments.command {
        TemplateCmd::List => ("POST", format!("{base}/list"), json!({})),
        TemplateCmd::Show { template_id } => (
            "POST",
            format!("{base}/show"),
            json!({"template_id": template_id}),
        ),
        TemplateCmd::Validate { file } => {
            let mut templates = templates_of(read_json(&file)?)?;
            if templates.len() != 1 {
                bail!("validate checks one template at a time");
            }
            (
                "POST",
                format!("{base}/validate"),
                json!({"template": templates.remove(0)}),
            )
        }
        TemplateCmd::Set {
            file,
            expected_revision,
        } => (
            "POST",
            format!("{base}/set"),
            json!({"templates": templates_of(read_json(&file)?)?,
                "expected_revision": expected_revision}),
        ),
        TemplateCmd::Retire { template_id } => (
            "POST",
            format!("{base}/retire"),
            json!({"template_id": template_id}),
        ),
        TemplateCmd::SaveGoal {
            goal,
            source,
            name,
            description,
            targets_as_inputs,
        } => (
            "POST",
            format!("{base}/save_goal"),
            json!({"goal": {"connection_id": source, "native_id": goal}, "name": name,
                "description": description, "targets_as_inputs": targets_as_inputs}),
        ),
        TemplateCmd::Preview {
            template_id,
            inputs,
        } => {
            let (inputs, choices) = shaped_inputs(inputs)?;
            (
                "POST",
                format!("{base}/preview"),
                json!({"template_id": template_id, "inputs": inputs, "choices": choices}),
            )
        }
        TemplateCmd::Apply {
            template_id,
            inputs,
            tool,
            model,
            effort,
            operation_id,
        } => {
            let (inputs, choices) = shaped_inputs(inputs)?;
            (
                "POST",
                format!("{base}/apply"),
                json!({"template_id": template_id, "inputs": inputs, "choices": choices,
                    "tool": tool, "model": model, "effort": effort,
                    "operation_id": operation_id}),
            )
        }
        TemplateCmd::ApplyShow { operation_id } => (
            "POST",
            format!("{base}/apply_show"),
            json!({"operation_id": operation_id}),
        ),
        TemplateCmd::Accept { operation_id, all } => (
            "POST",
            format!("{base}/accept"),
            match operation_id {
                Some(operation_id) if !all => json!({"operation_id": operation_id}),
                _ => json!({"all": true}),
            },
        ),
        TemplateCmd::Resume => ("POST", format!("{base}/resume"), json!({})),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Harness {
        #[command(flatten)]
        arguments: TemplateArgs,
    }
    fn request(line: &[&str]) -> Result<(&'static str, String, Value)> {
        let parsed =
            Harness::try_parse_from(std::iter::once("template").chain(line.iter().copied()))?;
        route("owner-1", parsed.arguments)
    }

    #[test]
    fn hs2_a4rvya_every_template_command_is_one_request_to_the_shared_runtime() {
        let base = "/checkouts/owner-1/orchestration/templates";
        assert_eq!(
            request(&["list"]).unwrap(),
            ("POST", format!("{base}/list"), json!({}))
        );
        assert_eq!(
            request(&["show", "T1"]).unwrap(),
            ("POST", format!("{base}/show"), json!({"template_id":"T1"}))
        );
        assert_eq!(
            request(&["retire", "T1"]).unwrap(),
            (
                "POST",
                format!("{base}/retire"),
                json!({"template_id":"T1"})
            )
        );
        assert_eq!(
            request(&[
                "save-goal",
                "--goal",
                "OWN-1",
                "--name",
                "Feature",
                "--targets-as-inputs"
            ])
            .unwrap(),
            (
                "POST",
                format!("{base}/save_goal"),
                json!({"goal":{"connection_id":"","native_id":"OWN-1"},"name":"Feature",
                    "description":null,"targets_as_inputs":true})
            )
        );
        assert_eq!(
            request(&[
                "apply",
                "T1",
                "--tool",
                "claude",
                "--operation-id",
                "op-1",
                "--inputs",
                r#"{"feature":"Rate limits"}"#,
                "--input",
                "contract=false",
                "--input",
                "count=3",
                "--input",
                "note=a=b",
                "--create",
                "web",
            ])
            .unwrap(),
            (
                "POST",
                format!("{base}/apply"),
                json!({"template_id":"T1","tool":"claude","model":null,"effort":null,
                    "operation_id":"op-1",
                    "inputs":{"feature":"Rate limits","contract":false,"count":3,"note":"a=b"},
                    "choices":{"web":"create"}})
            )
        );
        // An apply names its tool and retains an operation id.
        assert!(request(&["apply", "T1", "--tool", "claude"]).is_err());
        assert!(request(&["apply", "T1", "--operation-id", "op"]).is_err());
        assert!(request(&["preview", "T1", "--input", "novalue"]).is_err());
        assert_eq!(
            request(&["preview", "T1", "--input", "feature=Search"]).unwrap(),
            (
                "POST",
                format!("{base}/preview"),
                json!({"template_id":"T1","inputs":{"feature":"Search"},"choices":{}})
            )
        );
        assert_eq!(
            request(&["resume"]).unwrap(),
            ("POST", format!("{base}/resume"), json!({}))
        );
        assert_eq!(
            request(&["accept", "op-1"]).unwrap(),
            (
                "POST",
                format!("{base}/accept"),
                json!({"operation_id": "op-1"})
            )
        );
        // HS2-0YHA2S: every held plan, never together with one.
        assert_eq!(
            request(&["accept", "--all"]).unwrap(),
            ("POST", format!("{base}/accept"), json!({"all": true}))
        );
        assert!(request(&["accept"]).is_err());
        assert!(request(&["accept", "op-1", "--all"]).is_err());
        assert_eq!(
            request(&["apply-show", "op-1"]).unwrap(),
            (
                "POST",
                format!("{base}/apply_show"),
                json!({"operation_id":"op-1"})
            )
        );
    }

    #[test]
    fn hs2_a4rvya_a_file_holds_one_template_a_list_or_a_whole_templates_file() {
        let dir = tempfile::tempdir().unwrap();
        let one = dir.path().join("one.json");
        std::fs::write(&one, r#"{"name":"A","goal":{"title":"A"},"steps":[]}"#).unwrap();
        let whole = dir.path().join("whole.json");
        std::fs::write(
            &whole,
            r#"{"schema_version":1,"revision":"R","templates":[{"name":"A"},{"name":"B"}]}"#,
        )
        .unwrap();
        let (_, _, body) = request(&["set", "--file", one.to_str().unwrap()]).unwrap();
        assert_eq!(body["templates"].as_array().unwrap().len(), 1);
        let (_, _, body) = request(&[
            "set",
            "--file",
            whole.to_str().unwrap(),
            "--expected-revision",
            "R",
        ])
        .unwrap();
        assert_eq!(body["templates"].as_array().unwrap().len(), 2);
        assert_eq!(body["expected_revision"], "R");
        let (_, path, body) = request(&["validate", "--file", one.to_str().unwrap()]).unwrap();
        assert!(path.ends_with("/validate"));
        assert_eq!(body["template"]["name"], "A");
        assert!(request(&["validate", "--file", whole.to_str().unwrap()]).is_err());
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, "[1]").unwrap();
        assert!(request(&["set", "--file", bad.to_str().unwrap()]).is_err());
    }
}
