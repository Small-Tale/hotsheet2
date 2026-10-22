//! Read-only setup diagnosis. The shared setup planner supplies the exact artifact diff.

use std::path::Path;
use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Context, Result, bail};
use hotsheet_model::{Status, Ticket, Ulid};
use hotsheet_ticketing::{FsStore, Scope, Settings};
use serde_json::{Value, json};

use super::{hook_get_json, hook_server_route};

pub(super) fn run(
    store: &Path,
    project: &Path,
    selected: Option<&str>,
    as_json: bool,
) -> Result<()> {
    let plan = hotsheet_cli::setup::plan_refresh_setup(store, project)?;
    if let Some(id) = selected {
        if !plan.tools.iter().any(|tool| tool.tool == id) {
            bail!("unknown tool '{id}' (no such plugin)");
        }
    }
    let registry = registry_status(store, project);
    let live = live_status(store, project);
    let refresh_command = format!(
        "hotsheet-cli -C {} setup --refresh --project {}",
        shell_quote(&store.to_string_lossy()),
        shell_quote(&project.to_string_lossy())
    );
    let mut would_change = serde_json::to_value(&plan.changes)?;
    if registry["status"] != "registered" {
        if let Some(changes) = would_change.as_array_mut() {
            changes.push(json!({"path": registry["path"], "action": "write"}));
        }
    }
    if let Some(changes) = would_change.as_array_mut() {
        changes.extend(additional_refresh_changes(store, project)?);
        changes.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
        changes.dedup_by(|a, b| a["path"] == b["path"] && a["action"] == b["action"]);
    }
    let mut tools = Vec::new();
    for finding in &plan.tools {
        if selected.is_some_and(|id| id != finding.tool)
            || (selected.is_none() && !finding.detected && !finding.previously_managed)
        {
            continue;
        }
        let plugin = hotsheet_plugins::find(&finding.tool)
            .with_context(|| format!("plugin '{}' disappeared during diagnosis", finding.tool))?;
        let command = hotsheet_plugins::mcp_command(&plugin.manifest.mcp.command);
        let command_available = command_on_path(&command);
        let expected_args = plugin
            .manifest
            .mcp
            .args
            .iter()
            .map(|arg| {
                arg.replace(
                    "{store}",
                    &store
                        .canonicalize()
                        .unwrap_or_else(|_| store.to_path_buf())
                        .to_string_lossy(),
                )
            })
            .collect::<Vec<_>>();
        let changed_paths: Vec<_> = plan
            .changes
            .iter()
            .filter(|change| {
                finding
                    .artifacts
                    .iter()
                    .any(|artifact| artifact.path == change.path)
            })
            .collect();
        let failing = finding
            .artifacts
            .iter()
            .filter(|artifact| {
                matches!(
                    artifact.status,
                    hotsheet_plugins::setup::ArtifactStatus::Missing
                        | hotsheet_plugins::setup::ArtifactStatus::Outdated
                )
            })
            .collect::<Vec<_>>();
        let live_tool = live["terminals"].as_array().and_then(|terminals| {
            terminals
                .iter()
                .find(|terminal| terminal["tool"] == finding.tool)
        });
        let mcp_connected = live_tool.and_then(|terminal| terminal["mcp_connected"].as_bool());
        let verdict = if !finding.enabled {
            "disabled"
        } else if !command_available {
            "command_missing"
        } else if !failing.is_empty() {
            "setup_incomplete"
        } else if mcp_connected == Some(true) {
            "connected"
        } else if mcp_connected == Some(false) {
            "mcp_not_connected"
        } else {
            "configured_connection_unverified"
        };
        let cause = match verdict {
            "disabled" => "The project's enabled_plugins setting excludes this tool.",
            "command_missing" => "The configured MCP server command is unavailable on PATH.",
            "setup_incomplete" => "One or more managed setup artifacts are missing or outdated.",
            "connected" => "A live terminal reports an AI connection.",
            "mcp_not_connected" => "A live terminal for this tool reports no MCP lease.",
            _ => "Local setup is current; no live connection was confirmed.",
        };
        let hook_review = plugin
            .manifest
            .hooks
            .as_ref()
            .is_some_and(|hooks| hooks.change_notice.is_some());
        let fix = match verdict {
            "disabled" => format!("Enable this plugin in project settings, then run `{refresh_command}`."),
            "command_missing" => format!("Install hotsheet-mcp or put its executable on PATH, then run `{refresh_command}`."),
            "setup_incomplete" if hook_review && finding.artifacts.iter().any(|artifact| {
                artifact.kind == hotsheet_plugins::setup::ArtifactKind::Hooks
                    && matches!(artifact.status, hotsheet_plugins::setup::ArtifactStatus::Missing |
                        hotsheet_plugins::setup::ArtifactStatus::Outdated)
            }) => format!("Run `{refresh_command}`, review changed hooks in the tool (/hooks in Codex), then restart the session."),
            "setup_incomplete" => format!("Run `{refresh_command}`, then restart the AI tool."),
            "connected" => "No setup change is needed.".into(),
            "mcp_not_connected" => "Restart this tool's session so it reloads the MCP configuration, then check its MCP connection.".into(),
            _ if hook_review => "Start or restart the AI tool; review changed hooks in the tool's trust UI (Codex: /hooks).".into(),
            _ => "Start or restart the AI tool in a Hot Sheet terminal and check its MCP connection.".into(),
        };
        tools.push(json!({
            "tool": finding.tool,
            "enabled": finding.enabled,
            "detected": finding.detected,
            "previously_managed": finding.previously_managed,
            "verdict": verdict,
            "cause": cause,
            "fix": fix,
            "mcp_command": {"command": command, "available": command_available,
                "expected_args": expected_args},
            "artifacts": finding.artifacts,
            "live_terminal": live_tool,
            "would_change": changed_paths,
            "hook_trust": if !hook_review { "not_applicable" }
                else if live_tool.is_some_and(|terminal| terminal["hooks_active"] == true) { "reported_active" }
                else { "unverifiable_offline" },
        }));
    }
    let result = json!({
        "project": project,
        "store": store,
        "registry": registry,
        "live": live,
        "tools": tools,
        "guidance_notices": plan.report.set_up.iter().flat_map(|tool| &tool.notices)
            .filter(|notice| matches!(notice, hotsheet_plugins::setup::SetupNotice::BundledGuidanceBlocked { .. }))
            .collect::<Vec<_>>(),
        "would_change": would_change,
    });
    if as_json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_human(&result);
    }
    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Refresh's CLI wrapper also migrates settings and regenerates the derived worklist.
/// Predict their byte changes without taking settings locks or writing projections.
fn additional_refresh_changes(store: &Path, project: &Path) -> Result<Vec<Value>> {
    let settings = Settings::with_legacy_stores(project, [store]);
    let mut changes = Vec::new();
    for (scope, path) in [
        (
            Scope::Global,
            hotsheet_plugins::hotsheet_home().join("settings.json"),
        ),
        (Scope::Shared, project.join(".hotsheet2/settings.json")),
        (Scope::Local, project.join(".hotsheet2/settings.local.json")),
    ] {
        let mut map = settings.map(scope)?;
        if !path.is_file() && (map.is_empty() || scope == Scope::Global) {
            continue;
        }
        map.insert("$hotsheetSchema".into(), json!(1));
        let expected = format!("{}\n", serde_json::to_string_pretty(&Value::Object(map))?);
        if std::fs::read(&path).ok().as_deref() != Some(expected.as_bytes()) {
            changes.push(json!({"path": path_label(project, &path), "action": "write"}));
        }
    }
    changes.extend(worklist_refresh_changes(store, project)?);
    let local = project.join(".hotsheet2/settings.local.json");
    if local.is_file() || !settings.map(Scope::Local)?.is_empty() {
        let ignore = project.join(".gitignore");
        let existing = std::fs::read_to_string(&ignore).unwrap_or_default();
        if !existing
            .lines()
            .any(|line| line.trim() == ".hotsheet2/settings.local.json")
        {
            changes.push(json!({"path": path_label(project, &ignore), "action": "write"}));
        }
    }
    Ok(changes)
}

fn worklist_refresh_changes(store: &Path, project: &Path) -> Result<Vec<Value>> {
    let registry_path = hotsheet_plugins::hotsheet_home().join("checkouts.json");
    let registry: Value = std::fs::read(&registry_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Value::Null);
    let source = store.canonicalize().unwrap_or_else(|_| store.to_path_buf());
    let project_abs = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());
    let mut checkouts: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
    for checkout in registry["checkouts"].as_array().into_iter().flatten() {
        let Some(root) = checkout["root"].as_str().map(PathBuf::from) else {
            continue;
        };
        let mut stores = checkout["stores"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        let is_project = root == project_abs;
        if is_project && !stores.iter().any(|path| path == &source) {
            stores.push(source.clone());
        }
        if is_project
            || stores
                .iter()
                .any(|path| path.canonicalize().unwrap_or_else(|_| path.clone()) == source)
        {
            checkouts.push((root, stores));
        }
    }
    if !checkouts.iter().any(|(root, _)| root == &project_abs) {
        checkouts.push((project_abs, vec![source]));
    }
    let mut changes = Vec::new();
    for (root, stores) in checkouts {
        let mut tickets: BTreeMap<Ulid, Ticket> = BTreeMap::new();
        for source in &stores {
            for ticket in FsStore::open(source)?.list_tickets()? {
                match tickets.get(&ticket.id) {
                    Some(existing) if existing.status != Status::Moved => {}
                    _ => {
                        tickets.insert(ticket.id, ticket);
                    }
                }
            }
        }
        let settings = Settings::with_legacy_stores(&root, &stores);
        let entries = hotsheet_ticketing::auto_context::effective(&settings)?;
        let rendered = hotsheet_ticketing::worklist::render_with_auto_context(
            &tickets.into_values().collect::<Vec<_>>(),
            &entries,
        );
        let worklist = root.join(hotsheet_ticketing::worklist::CHECKOUT_WORKLIST);
        if std::fs::read(&worklist).ok().as_deref() != Some(rendered.as_bytes()) {
            changes.push(json!({"path": path_label(project, &worklist), "action": "write"}));
        }
        if let Some(exclude) = local_git_exclude(&root) {
            let existing = std::fs::read_to_string(&exclude).unwrap_or_default();
            if !existing
                .lines()
                .any(|line| line.trim() == hotsheet_ticketing::worklist::CHECKOUT_WORKLIST)
            {
                changes.push(json!({"path": path_label(project, &exclude), "action": "write"}));
            }
        }
    }
    Ok(changes)
}

fn local_git_exclude(project: &Path) -> Option<std::path::PathBuf> {
    let dot_git = project.join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let pointer = std::fs::read_to_string(dot_git).ok()?;
        let raw = pointer.strip_prefix("gitdir:")?.trim();
        let path = std::path::PathBuf::from(raw);
        if path.is_absolute() {
            path
        } else {
            project.join(path)
        }
    };
    let common = std::fs::read_to_string(git_dir.join("commondir"))
        .ok()
        .map(|value| {
            let path = std::path::PathBuf::from(value.trim());
            if path.is_absolute() {
                path
            } else {
                git_dir.join(path)
            }
        })
        .unwrap_or(git_dir);
    Some(common.join("info/exclude"))
}

fn path_label(project: &Path, path: &Path) -> String {
    let canonical = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());
    path.strip_prefix(project)
        .or_else(|_| path.strip_prefix(&canonical))
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn command_on_path(command: &str) -> bool {
    let path = Path::new(command);
    if path.components().count() > 1 || path.is_absolute() {
        return path.is_file();
    }
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(command).is_file()))
        .unwrap_or(false)
}

fn registry_status(store: &Path, project: &Path) -> Value {
    let path = hotsheet_plugins::hotsheet_home().join("checkouts.json");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return json!({"status":"missing", "path":path, "fix":"Run setup --refresh to register this checkout."});
        }
        Err(error) => {
            return json!({"status":"unreadable", "path":path, "detail":error.to_string()});
        }
    };
    let data: Value = match serde_json::from_slice(&bytes) {
        Ok(data) => data,
        Err(error) => return json!({"status":"invalid", "path":path, "detail":error.to_string()}),
    };
    let root = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());
    let source = store.canonicalize().unwrap_or_else(|_| store.to_path_buf());
    let registered = data["checkouts"].as_array().is_some_and(|checkouts| {
        checkouts.iter().any(|checkout| {
            checkout["root"]
                .as_str()
                .is_some_and(|value| Path::new(value) == root)
                && checkout["sources"].as_array().is_some_and(|sources| {
                    sources.iter().any(|entry| {
                        entry["provider"] == "git"
                            && entry["locator"]
                                .as_str()
                                .is_some_and(|value| Path::new(value) == source)
                    })
                })
        })
    });
    json!({"status": if registered {"registered"} else {"missing_link"}, "path":path,
        "fix": if registered {"None."} else {"Run setup --refresh to register this checkout and store."}})
}

fn live_status(store: &Path, project: &Path) -> Value {
    let route = hotsheet_cli::external_launch::discover_running_server(
        store,
        &hotsheet_plugins::hotsheet_home(),
    )
    .ok()
    .map(|server| (server.url, server.secret))
    .or_else(hook_server_route);
    let Some((url, secret)) = route else {
        return json!({"server":"not_configured", "connection":"unknown"});
    };
    let endpoint = format!("{}/terminals", url.trim_end_matches('/'));
    let Ok(terminals) = hook_get_json(&endpoint, &secret) else {
        return json!({"server":"unreachable", "connection":"unknown"});
    };
    let terminal_id = std::env::var("HOTSHEET_TERMINAL_ID").ok();
    let root = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());
    let relevant = terminals
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| {
            let named = terminal_id.as_deref().is_some_and(|id| entry["id"] == id);
            let in_project = entry["cwd"].as_str().is_some_and(|cwd| {
                Path::new(cwd)
                    .canonicalize()
                    .unwrap_or_else(|_| Path::new(cwd).to_path_buf())
                    .starts_with(&root)
            });
            named || in_project
        })
        .map(|entry| {
            json!({
                "id": entry["id"], "tool": entry["tool"], "cwd": entry["cwd"],
                "mcp_connected": entry["mcp_connected"],
                "hooks_active": entry.get("ai_connection").is_some(),
                "last_hook_report": entry.get("last_hook_report"),
            })
        })
        .collect::<Vec<_>>();
    json!({"server":"reachable", "terminals":relevant})
}

fn print_human(result: &Value) {
    println!(
        "Setup diagnosis for {}",
        result["project"].as_str().unwrap_or("project")
    );
    println!(
        "Checkout registry: {}",
        result["registry"]["status"].as_str().unwrap_or("unknown")
    );
    println!(
        "Live server: {}",
        result["live"]["server"].as_str().unwrap_or("unknown")
    );
    if let Some(notices) = result["guidance_notices"].as_array() {
        for notice in notices {
            println!(
                "Bundled guidance review: {} section {} (installed v{}, bundled v{}). Compare the local section with the bundled guidance before editing it; refresh preserves the local body.",
                notice["path"].as_str().unwrap_or("?"),
                notice["section"].as_str().unwrap_or("?"),
                notice["installed_version"].as_u64().unwrap_or(0),
                notice["bundled_version"].as_u64().unwrap_or(0),
            );
        }
    }
    if let Some(tools) = result["tools"].as_array() {
        for tool in tools {
            println!(
                "{}: {}",
                tool["tool"].as_str().unwrap_or("tool"),
                tool["verdict"].as_str().unwrap_or("unknown")
            );
            println!("  Cause: {}", tool["cause"].as_str().unwrap_or("unknown"));
            println!("  Fix: {}", tool["fix"].as_str().unwrap_or("unknown"));
            if let Some(artifacts) = tool["artifacts"].as_array() {
                for artifact in artifacts {
                    println!(
                        "  {}: {} ({})",
                        artifact["path"].as_str().unwrap_or("?"),
                        artifact["status"].as_str().unwrap_or("?"),
                        artifact["detail"].as_str().unwrap_or("")
                    );
                }
            }
        }
    }
    if let Some(changes) = result["would_change"].as_array() {
        if changes.is_empty() {
            println!("Refresh would change no files.");
        } else {
            println!("Refresh would change:");
            for change in changes {
                println!(
                    "  {} {}",
                    change["action"].as_str().unwrap_or("change"),
                    change["path"].as_str().unwrap_or("?")
                );
            }
        }
    }
}
