//! `hotsheet-cli sync`'s side of asking an AI to finish a ticket-store sync it cannot
//! finish alone (HS2-QR1K8R, `docs/02` §2.12.4): the same workflow the server runs
//! ([`hotsheet_ticketing::sync_ai::ask_for_help`]), reached headlessly.
//!
//! When a Hot Sheet server hosts the store, the CLI leaves the ask to it, so the session
//! starts with the server's permission bridge (and `session` can push into a running
//! terminal). Without a server, no session can receive a push, and `start` launches the
//! project's
//! default AI tool as a detached `hotsheet-cli -C <store> trigger <tool> --prompt …
//! --project <checkout>` that outlives this command; its output goes to
//! `<git-dir>/hotsheet-sync-ai.log`. That session runs in the tool's configured default
//! permission mode with no permission bridge (the headless trigger default).

use std::path::{Path, PathBuf};
use std::process::Stdio;

use hotsheet_ticketing::Settings;
use hotsheet_ticketing::sync_ai::{
    AiReach, AskContext, AskOutcome, SessionDone, ask_ai_mode, ask_for_help, configured_ai_tool,
    store_reference,
};

/// What `hotsheet-cli sync` did about a stall.
#[derive(Debug)]
pub enum CliAsk {
    /// A running server hosts the store and asks on its next pass.
    LeftToServer,
    Ran(AskOutcome),
}

struct Project {
    root: PathBuf,
    settings: Settings,
}

fn project_for(store: &Path, home: &Path) -> Project {
    let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let store_canonical = canonical(store);
    let registry =
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json"));
    let checkout = registry
        .list()
        .unwrap_or_default()
        .into_iter()
        .find(|checkout| {
            checkout
                .stores
                .iter()
                .any(|root| canonical(Path::new(root)) == store_canonical)
        });
    match checkout {
        Some(checkout) => Project {
            root: PathBuf::from(&checkout.root),
            settings: checkout.settings(),
        },
        None => Project {
            root: store.to_path_buf(),
            settings: Settings::for_project(store),
        },
    }
}

/// Ask for help with the store's stall after `hotsheet-cli sync`, or leave it to a running
/// server. `program` is the `hotsheet-cli` executable a started session runs.
pub fn ask_after_cli_sync(store: &Path, home: &Path, program: &Path) -> CliAsk {
    if crate::external_launch::discover_running_server(store, home).is_ok() {
        return CliAsk::LeftToServer;
    }
    let project = project_for(store, home);
    let label = store.file_name().map_or_else(
        || store.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let context = AskContext {
        mode: ask_ai_mode(&project.settings),
        label,
        store_reference: store_reference(&project.root, store),
    };
    let reach = CliReach {
        store: store.to_path_buf(),
        project,
        program: program.to_path_buf(),
        plugin_dirs: hotsheet_plugins::default_dirs(),
    };
    CliAsk::Ran(ask_for_help(store, &context, &reach))
}

struct CliReach {
    store: PathBuf,
    project: Project,
    program: PathBuf,
    plugin_dirs: Vec<PathBuf>,
}

impl AiReach for CliReach {
    /// Sessions that accept a pushed prompt live in a server; there is none.
    fn push_to_session(&self, _prompt: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn drivable_tool(&self) -> Result<String, String> {
        let tool = configured_ai_tool(&self.project.settings).ok_or_else(|| {
            "no default AI tool is configured for this project (choose one in the AI settings)"
                .to_string()
        })?;
        let plugin = hotsheet_plugins::find_in(&tool, &self.plugin_dirs)
            .ok_or_else(|| format!("the default AI tool {tool} has no Hot Sheet plugin"))?;
        let Some(drive) = plugin.manifest.drive.as_ref() else {
            return Err(format!(
                "the default AI tool {tool} cannot be driven by Hot Sheet"
            ));
        };
        if !hotsheet_plugins::detected_drivable_plugins(&self.plugin_dirs)
            .iter()
            .any(|candidate| candidate.id() == tool)
        {
            return Err(format!(
                "the default AI tool {tool} is not installed on this machine"
            ));
        }
        // The trigger refuses a project the tool was never set up in; say so now rather
        // than from a detached process nobody reads.
        if drive.transport != "app-server"
            && !self.project.root.join(&plugin.manifest.mcp.target).exists()
        {
            return Err(format!(
                "{tool} is not set up in this project; run `hotsheet-cli setup {tool}` there"
            ));
        }
        Ok(tool)
    }

    /// The detached trigger is not watched, so `done` is never called; the ask stays
    /// `started` until the store publishes again.
    fn start_session(
        &self,
        tool: &str,
        prompt: &str,
        _done: SessionDone,
    ) -> Result<String, String> {
        let log = hotsheet_ticketing::git::command()
            .arg("-C")
            .arg(&self.store)
            .args(["rev-parse", "--absolute-git-dir"])
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| {
                PathBuf::from(String::from_utf8_lossy(&out.stdout).trim_end_matches(['\n', '\r']))
                    .join("hotsheet-sync-ai.log")
            })
            .ok_or_else(|| "the store is not a Git repository".to_string())?;
        let output = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .map_err(|error| error.to_string())?;
        let errors = output.try_clone().map_err(|error| error.to_string())?;
        // Inherited GIT_DIR and friends are dropped (HS2-J79CZF).
        let mut command = hotsheet_ticketing::git::launch(&self.program);
        command
            .arg("-C")
            .arg(&self.store)
            .args(["trigger", tool, "--prompt", prompt, "--project"])
            .arg(&self.project.root)
            .stdin(Stdio::null())
            .stdout(output)
            .stderr(errors);
        // Its own process group, so the end of this command (or a Ctrl-C to it) does not
        // stop the repair.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let child = command.spawn().map_err(|error| error.to_string())?;
        Ok(format!("cli-trigger-{}", child.id()))
    }
}
