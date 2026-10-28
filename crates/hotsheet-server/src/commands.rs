//! Bounded in-memory command execution/history. Exact argv comes from typed settings;
//! output is cursor-pollable while the process runs and cancellation kills the child.
//! Shell and AI commands run in a client terminal or AI session instead, so the client records
//! each launch here and the same history lists it (HS2-PWNPM2).

use hotsheet_model::Timestamp;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::commands::{CommandDefinition, CommandKind};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use time::OffsetDateTime;

const HISTORY_CAP: usize = 50;
const OUTPUT_CAP: usize = 10_000;
type ChangeCallback = Arc<dyn Fn(CommandRun) + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
pub struct OutputLine {
    pub seq: u64,
    pub stream: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LaunchKind {
    Shell,
    Ai,
}

/// Where a shell or AI command launch went: the terminal it opened, or the ticket it queued and
/// the AI tool signaled to work it (HS2-PWNPM2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandLaunch {
    pub kind: LaunchKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandRun {
    pub id: String,
    pub command_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub project: String,
    pub state: String,
    pub exit_code: Option<i32>,
    pub output: Vec<OutputLine>,
    pub started_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch: Option<CommandLaunch>,
}

struct LiveRun {
    view: CommandRun,
    child: Option<Arc<Mutex<hotsheet_process_journal::TrackedChild>>>,
    next_seq: u64,
}

type CommandWriteSequences = HashMap<(String, String), (u64, Option<String>)>;

#[derive(Clone)]
pub struct CommandManager {
    definitions: Arc<Mutex<HashMap<String, Vec<CommandDefinition>>>>,
    write_sequences: Arc<Mutex<CommandWriteSequences>>,
    roots: Arc<Mutex<HashMap<String, PathBuf>>>,
    runs: Arc<Mutex<HashMap<String, LiveRun>>>,
    order: Arc<Mutex<VecDeque<String>>>,
    on_change: Option<ChangeCallback>,
}

impl CommandManager {
    pub fn new(root: PathBuf, definitions: Vec<CommandDefinition>) -> Self {
        let definitions = HashMap::from([(String::new(), definitions)]);
        let roots = HashMap::from([(String::new(), root)]);
        Self {
            definitions: Arc::new(Mutex::new(definitions)),
            write_sequences: Default::default(),
            roots: Arc::new(Mutex::new(roots)),
            runs: Default::default(),
            order: Default::default(),
            on_change: None,
        }
    }
    pub(crate) fn with_on_change(mut self, callback: ChangeCallback) -> Self {
        self.on_change = Some(callback);
        self
    }
    pub fn definitions(&self) -> Vec<CommandDefinition> {
        self.definitions_for("")
    }
    pub fn definitions_for(&self, project: &str) -> Vec<CommandDefinition> {
        self.definitions
            .lock_or_recover()
            .get(project)
            .cloned()
            .unwrap_or_default()
    }
    pub fn replace_definitions(&self, definitions: Vec<CommandDefinition>) {
        self.definitions
            .lock_or_recover()
            .insert(String::new(), definitions);
    }
    pub fn with_settings_lock<T>(&self, read: impl FnOnce() -> T) -> T {
        let _guard = self.write_sequences.lock_or_recover();
        read()
    }
    /// Serialize an editor session's writes through persistence and live-state replacement.
    /// A delayed older request must not commit after a newer sequence for the same project.
    pub fn write_if_newer<T, E>(
        &self,
        project: &str,
        order: Option<(&str, u64)>,
        write: impl FnOnce() -> Result<T, E>,
    ) -> Result<Option<T>, E> {
        let mut sequences = self.write_sequences.lock_or_recover();
        if let Some((session, sequence)) = order {
            let key = (project.to_owned(), session.to_owned());
            if sequences
                .get(&key)
                .is_some_and(|current| current.0 >= sequence)
            {
                return Ok(None);
            }
            let result = write()?;
            sequences.insert(key, (sequence, None));
            Ok(Some(result))
        } else {
            write().map(Some)
        }
    }

    /// Compare a persisted content revision under the same project write lock as persistence.
    /// A newer write from the same editor may follow its own earlier in-flight save, provided
    /// no independent writer changed the resource in between.
    pub fn write_if_current<T, E>(
        &self,
        project: &str,
        order: Option<(&str, u64)>,
        expected: &str,
        new_revision: &str,
        current_revision: impl Fn() -> Result<String, E>,
        write: impl FnOnce(&str) -> Result<T, E>,
    ) -> Result<Option<(T, String)>, E> {
        let mut sequences = self.write_sequences.lock_or_recover();
        let key = order.map(|(session, _)| (project.to_owned(), session.to_owned()));
        let prior = key.as_ref().and_then(|key| sequences.get(key));
        if order.is_some_and(|(_, sequence)| prior.is_some_and(|(current, _)| *current >= sequence))
        {
            return Ok(None);
        }
        let current = current_revision()?;
        let follows_own_write =
            prior.is_some_and(|(_, revision)| revision.as_deref() == Some(&current));
        if expected != "*" && expected != current && !follows_own_write {
            return Ok(None);
        }
        let result = write(&current)?;
        if let (Some(key), Some((_, sequence))) = (key, order) {
            sequences.insert(key, (sequence, Some(new_revision.to_owned())));
        }
        Ok(Some((result, new_revision.to_owned())))
    }
    pub fn replace_project(
        &self,
        project: impl Into<String>,
        root: PathBuf,
        definitions: Vec<CommandDefinition>,
    ) {
        let project = project.into();
        self.roots.lock_or_recover().insert(project.clone(), root);
        self.definitions
            .lock_or_recover()
            .insert(project, definitions);
    }
    pub fn list(&self) -> Vec<CommandRun> {
        self.list_for("")
    }
    pub fn list_all(&self) -> Vec<CommandRun> {
        let runs = self.runs.lock_or_recover();
        self.order
            .lock_or_recover()
            .iter()
            .filter_map(|id| runs.get(id).map(|run| run.view.clone()))
            .collect()
    }
    pub fn list_for(&self, project: &str) -> Vec<CommandRun> {
        let runs = self.runs.lock_or_recover();
        self.order
            .lock_or_recover()
            .iter()
            .filter_map(|id| {
                runs.get(id)
                    .filter(|run| run.view.project == project)
                    .map(|run| run.view.clone())
            })
            .collect()
    }
    pub fn get(&self, id: &str, after: u64) -> Option<CommandRun> {
        self.get_for("", id, after)
    }
    pub fn get_for(&self, project: &str, id: &str, after: u64) -> Option<CommandRun> {
        self.runs.lock_or_recover().get(id).and_then(|r| {
            if r.view.project != project {
                return None;
            }
            let mut v = r.view.clone();
            v.output.retain(|o| o.seq > after);
            Some(v)
        })
    }
    pub fn start(&self, command_id: &str) -> Result<CommandRun, String> {
        self.start_for("", command_id)
    }
    pub fn start_for(&self, project: &str, command_id: &str) -> Result<CommandRun, String> {
        let def = self
            .definitions
            .lock_or_recover()
            .get(project)
            .and_then(|definitions| {
                definitions
                    .iter()
                    .find(|definition| definition.id == command_id)
            })
            .cloned()
            .ok_or_else(|| "unknown configured command".to_string())?;
        let root = def
            .cwd
            .as_deref()
            .map(PathBuf::from)
            .or_else(|| self.roots.lock_or_recover().get(project).cloned())
            .ok_or_else(|| "unknown command project".to_string())?;
        let (program, args) = execution(&def, &root)?;
        let mut command = hotsheet_ticketing::git::launch(program);
        command
            .args(args)
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child =
            hotsheet_process_journal::LaunchContext {
                journal: hotsheet_process_journal::ProcessJournal::at(
                    hotsheet_plugins::hotsheet_home(),
                ),
                supervisor: "hotsheet-server",
                purpose: hotsheet_process_journal::Purpose::ServerCommand,
                project_id: (!project.is_empty()).then(|| project.to_owned()),
                session_id: None,
                binary_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
            }
            .spawn(&mut command)
            .map_err(|e| e.to_string())?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let id = ulid::Ulid::new().to_string();
        let handle = Arc::new(Mutex::new(child));
        let view = CommandRun {
            id: id.clone(),
            command_id: command_id.into(),
            project: project.to_owned(),
            state: "running".into(),
            exit_code: None,
            output: vec![],
            started_at: Timestamp::from_datetime(OffsetDateTime::now_utc()),
            launch: None,
        };
        self.remember(LiveRun {
            view: view.clone(),
            child: Some(handle.clone()),
            next_seq: 0,
        });
        if let Some(out) = stdout {
            self.read_lines(id.clone(), "stdout", out);
        }
        if let Some(err) = stderr {
            self.read_lines(id.clone(), "stderr", err);
        }
        let runs = self.runs.clone();
        let on_change = self.on_change.clone();
        let run_id = id.clone();
        std::thread::spawn(move || {
            loop {
                let result = {
                    let mut c = handle.lock_or_recover();
                    c.try_wait()
                };
                match result {
                    Ok(Some(status)) => {
                        let changed = if let Some(r) = runs.lock_or_recover().get_mut(&run_id) {
                            if r.view.state == "running" {
                                r.view.state = "completed".into();
                            }
                            r.view.exit_code = status.code();
                            r.child = None;
                            Some(r.view.clone())
                        } else {
                            None
                        };
                        if let (Some(callback), Some(run)) = (&on_change, changed) {
                            callback(run);
                        }
                        break;
                    }
                    Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
                    Err(_) => {
                        let changed = if let Some(r) = runs.lock_or_recover().get_mut(&run_id) {
                            r.view.state = "failed".into();
                            r.child = None;
                            Some(r.view.clone())
                        } else {
                            None
                        };
                        if let (Some(callback), Some(run)) = (&on_change, changed) {
                            callback(run);
                        }
                        break;
                    }
                }
            }
        });
        Ok(view)
    }
    /// Record that the client launched a shell command in a terminal or queued an AI command's
    /// ticket. The launch is complete once recorded and joins the bounded history (HS2-PWNPM2).
    pub fn record_launch_for(
        &self,
        project: &str,
        command_id: &str,
        launch: CommandLaunch,
    ) -> Result<CommandRun, String> {
        let kind = self
            .definitions
            .lock_or_recover()
            .get(project)
            .and_then(|definitions| {
                definitions
                    .iter()
                    .find(|definition| definition.id == command_id)
            })
            .map(|definition| definition.kind)
            .ok_or_else(|| "unknown configured command".to_string())?;
        let expected = match kind {
            CommandKind::Shell => LaunchKind::Shell,
            CommandKind::Ai => LaunchKind::Ai,
            CommandKind::Program => {
                return Err("program commands record their own runs".to_string());
            }
        };
        if launch.kind != expected {
            return Err("launch kind does not match the command".to_string());
        }
        let view = CommandRun {
            id: ulid::Ulid::new().to_string(),
            command_id: command_id.into(),
            project: project.to_owned(),
            state: "completed".into(),
            exit_code: None,
            output: vec![],
            started_at: Timestamp::from_datetime(OffsetDateTime::now_utc()),
            launch: Some(launch),
        };
        self.remember(LiveRun {
            view: view.clone(),
            child: None,
            next_seq: 0,
        });
        Ok(view)
    }
    /// Add a run to the newest end of the bounded history and announce it.
    fn remember(&self, run: LiveRun) {
        let view = run.view.clone();
        self.runs.lock_or_recover().insert(view.id.clone(), run);
        {
            let mut order = self.order.lock_or_recover();
            order.push_front(view.id.clone());
            while order.len() > HISTORY_CAP {
                if let Some(old) = order.pop_back() {
                    self.runs.lock_or_recover().remove(&old);
                }
            }
        }
        if let Some(callback) = &self.on_change {
            callback(view);
        }
    }
    fn read_lines(&self, id: String, stream: &str, reader: impl std::io::Read + Send + 'static) {
        let runs = self.runs.clone();
        let stream = stream.to_owned();
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                if let Some(r) = runs.lock_or_recover().get_mut(&id) {
                    r.next_seq += 1;
                    let seq = r.next_seq;
                    r.view.output.push(OutputLine {
                        seq,
                        stream: stream.clone(),
                        text: line,
                    });
                    if r.view.output.len() > OUTPUT_CAP {
                        r.view.output.remove(0);
                    }
                }
            }
        });
    }
    pub fn cancel(&self, id: &str) -> Result<CommandRun, String> {
        self.cancel_for("", id)
    }
    pub fn cancel_for(&self, project: &str, id: &str) -> Result<CommandRun, String> {
        let child = {
            let runs = self.runs.lock_or_recover();
            runs.get(id)
                .filter(|run| run.view.project == project)
                .and_then(|r| r.child.clone())
                .ok_or_else(|| "run is not active".to_string())?
        };
        child.lock_or_recover().kill().map_err(|e| e.to_string())?;
        let mut runs = self.runs.lock_or_recover();
        let r = runs.get_mut(id).unwrap();
        r.view.state = "cancelled".into();
        let view = r.view.clone();
        drop(runs);
        if let Some(callback) = &self.on_change {
            callback(view.clone());
        }
        Ok(view)
    }
}

fn execution(
    definition: &CommandDefinition,
    project_root: &std::path::Path,
) -> Result<(String, Vec<String>), String> {
    match definition.kind {
        CommandKind::Program => Ok((definition.program.clone(), definition.args.clone())),
        CommandKind::Shell => shell_execution(
            definition
                .command
                .as_deref()
                .ok_or_else(|| "shell command is missing command text".to_string())?,
        ),
        CommandKind::Ai => {
            let prompt = definition
                .prompt
                .as_deref()
                .ok_or_else(|| "AI command is missing a prompt".to_string())?;
            Ok((
                "hotsheet-cli".to_string(),
                vec![
                    "trigger".to_string(),
                    definition
                        .tool
                        .clone()
                        .unwrap_or_else(|| "claude".to_string()),
                    "--prompt".to_string(),
                    prompt.to_string(),
                    "--project".to_string(),
                    project_root.display().to_string(),
                ],
            ))
        }
    }
}

#[cfg(not(windows))]
fn shell_execution(command: &str) -> Result<(String, Vec<String>), String> {
    Ok((
        std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string()),
        vec!["-lc".to_string(), command.to_string()],
    ))
}

#[cfg(windows)]
fn shell_execution(command: &str) -> Result<(String, Vec<String>), String> {
    Ok((
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string()),
        vec![
            "/D".to_string(),
            "/S".to_string(),
            "/C".to_string(),
            command.to_string(),
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs() -> Vec<CommandDefinition> {
        serde_json::from_str(
            r#"[{"id":"build","title":"Build","kind":"shell","command":"make"},{"id":"ai","title":"AI","kind":"ai","prompt":"Go"},{"id":"echo","title":"Echo","program":"/bin/echo","args":[]}]"#,
        )
        .unwrap()
    }

    fn shell_launch() -> CommandLaunch {
        CommandLaunch {
            kind: LaunchKind::Shell,
            ticket_id: None,
            ticket_slug: None,
            tool: None,
            terminal_id: Some("t".into()),
            terminal_label: Some("Build".into()),
        }
    }

    #[test]
    fn launches_are_scoped_validated_and_bounded_like_runs() {
        let manager = CommandManager::new(PathBuf::from("/tmp"), defs());
        manager.replace_project("other", PathBuf::from("/tmp"), defs());
        let run = manager
            .record_launch_for("", "build", shell_launch())
            .unwrap();
        assert_eq!(run.state, "completed");
        assert_eq!(run.launch, Some(shell_launch()));
        // Project-scoped: the other project's history does not see it, and its own launch stays there.
        manager
            .record_launch_for("other", "build", shell_launch())
            .unwrap();
        assert_eq!(manager.list_for("").len(), 1);
        assert_eq!(manager.list_for("other").len(), 1);
        assert!(manager.get_for("other", &run.id, 0).is_none());
        assert_eq!(
            manager
                .record_launch_for("", "echo", shell_launch())
                .unwrap_err(),
            "program commands record their own runs"
        );
        assert_eq!(
            manager
                .record_launch_for("", "ai", shell_launch())
                .unwrap_err(),
            "launch kind does not match the command"
        );
        assert_eq!(
            manager
                .record_launch_for("", "missing", shell_launch())
                .unwrap_err(),
            "unknown configured command"
        );
        // Newest first, capped at the shared history size.
        for _ in 0..HISTORY_CAP + 5 {
            manager
                .record_launch_for("", "build", shell_launch())
                .unwrap();
        }
        let listed = manager.list_all();
        assert_eq!(listed.len(), HISTORY_CAP);
        assert!(listed.iter().all(|item| item.id != run.id));
    }

    #[test]
    fn ordered_writes_reject_repeats_and_older_sequences_per_project_and_session() {
        let manager = CommandManager::new(PathBuf::from("/tmp"), Vec::new());
        let mut writes = Vec::new();
        assert_eq!(
            manager.write_if_newer("a", Some(("editor", 2)), || {
                writes.push(2);
                Ok::<_, ()>(())
            }),
            Ok(Some(()))
        );
        assert_eq!(
            manager.write_if_newer("a", Some(("editor", 1)), || {
                writes.push(1);
                Ok::<_, ()>(())
            }),
            Ok(None)
        );
        assert_eq!(
            manager.write_if_newer("a", Some(("editor", 2)), || {
                writes.push(2);
                Ok::<_, ()>(())
            }),
            Ok(None)
        );
        assert_eq!(
            manager.write_if_newer("b", Some(("editor", 1)), || {
                writes.push(3);
                Ok::<_, ()>(())
            }),
            Ok(Some(()))
        );
        assert_eq!(
            manager.write_if_newer("a", Some(("other", 1)), || {
                writes.push(4);
                Ok::<_, ()>(())
            }),
            Ok(Some(()))
        );
        assert_eq!(
            manager.write_if_newer("a", Some(("editor", 3)), || Err::<(), _>("failed")),
            Err("failed")
        );
        assert_eq!(
            manager.write_if_newer("a", Some(("editor", 3)), || {
                writes.push(5);
                Ok::<_, ()>(())
            }),
            Ok(Some(()))
        );
        assert_eq!(writes, [2, 3, 4, 5]);
    }

    #[test]
    fn native_shell_and_ai_definitions_resolve_machine_details_only_at_runtime() {
        let shell: CommandDefinition = serde_json::from_value(serde_json::json!({
            "id":"lint","title":"Lint","kind":"shell","command":"npm run lint"
        }))
        .unwrap();
        let (shell_program, shell_args) =
            execution(&shell, std::path::Path::new("/code/project")).unwrap();
        assert!(!shell_program.is_empty());
        assert_eq!(shell_args.last().map(String::as_str), Some("npm run lint"));

        let ai: CommandDefinition = serde_json::from_value(serde_json::json!({
            "id":"review","title":"Review","kind":"ai","prompt":"Review the diff","tool":"codex"
        }))
        .unwrap();
        let (ai_program, ai_args) = execution(&ai, std::path::Path::new("/code/project")).unwrap();
        assert_eq!(ai_program, "hotsheet-cli");
        assert_eq!(
            ai_args,
            [
                "trigger",
                "codex",
                "--prompt",
                "Review the diff",
                "--project",
                "/code/project"
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_definition_can_override_the_store_root_working_directory() {
        let store = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let manager = CommandManager::new(
            store.path().to_path_buf(),
            vec![CommandDefinition {
                id: "pwd".into(),
                title: "Working directory".into(),
                kind: CommandKind::Program,
                program: "/bin/pwd".into(),
                args: Vec::new(),
                cwd: Some(project.path().display().to_string()),
                group: None,
                confirmation: None,
                command: None,
                prompt: None,
                tool: None,
                model: None,
                effort: None,
                icon: None,
                color: None,
            }],
        );
        let run = manager.start("pwd").unwrap();
        for _ in 0..100 {
            let view = manager.get(&run.id, 0).unwrap();
            if view.state == "completed" && !view.output.is_empty() {
                assert_eq!(
                    view.output[0].text,
                    project.path().canonicalize().unwrap().display().to_string()
                );
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("pwd command did not complete with output");
    }

    #[cfg(unix)]
    #[test]
    fn project_definitions_history_and_cancellation_are_isolated() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let manager = CommandManager::new(first.path().to_path_buf(), Vec::new());
        let command = CommandDefinition {
            id: "wait".into(),
            title: "Wait".into(),
            kind: CommandKind::Program,
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 2".into()],
            cwd: None,
            group: None,
            confirmation: None,
            command: None,
            prompt: None,
            tool: None,
            model: None,
            effort: None,
            icon: None,
            color: None,
        };
        manager.replace_project("first", first.path().to_path_buf(), vec![command]);
        manager.replace_project("second", second.path().to_path_buf(), Vec::new());

        let run = manager.start_for("first", "wait").unwrap();
        assert_eq!(manager.list_for("first").len(), 1);
        assert!(manager.list_for("second").is_empty());
        assert!(manager.get_for("second", &run.id, 0).is_none());
        assert!(manager.cancel_for("second", &run.id).is_err());
        assert_eq!(
            manager.cancel_for("first", &run.id).unwrap().state,
            "cancelled"
        );
    }
}
