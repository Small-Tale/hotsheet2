//! Bounded in-memory command execution/history. Exact argv comes from typed settings;
//! output is cursor-pollable while the process runs and cancellation kills the child.

use hotsheet_ticketing::commands::{CommandDefinition, CommandKind};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

const HISTORY_CAP: usize = 50;
const OUTPUT_CAP: usize = 10_000;
pub(crate) type ChangeCallback = Arc<dyn Fn(CommandRun) + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
pub struct OutputLine {
    pub seq: u64,
    pub stream: String,
    pub text: String,
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
}

struct LiveRun {
    view: CommandRun,
    child: Option<Arc<Mutex<Child>>>,
    next_seq: u64,
}

#[derive(Clone)]
pub struct CommandManager {
    definitions: Arc<Mutex<HashMap<String, Vec<CommandDefinition>>>>,
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
            .lock()
            .unwrap()
            .get(project)
            .cloned()
            .unwrap_or_default()
    }
    pub fn replace_definitions(&self, definitions: Vec<CommandDefinition>) {
        self.definitions
            .lock()
            .unwrap()
            .insert(String::new(), definitions);
    }
    pub fn replace_project(
        &self,
        project: impl Into<String>,
        root: PathBuf,
        definitions: Vec<CommandDefinition>,
    ) {
        let project = project.into();
        self.roots.lock().unwrap().insert(project.clone(), root);
        self.definitions
            .lock()
            .unwrap()
            .insert(project, definitions);
    }
    pub fn list(&self) -> Vec<CommandRun> {
        self.list_for("")
    }
    pub fn list_all(&self) -> Vec<CommandRun> {
        let runs = self.runs.lock().unwrap();
        self.order
            .lock()
            .unwrap()
            .iter()
            .filter_map(|id| runs.get(id).map(|run| run.view.clone()))
            .collect()
    }
    pub fn list_for(&self, project: &str) -> Vec<CommandRun> {
        let runs = self.runs.lock().unwrap();
        self.order
            .lock()
            .unwrap()
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
        self.runs.lock().unwrap().get(id).and_then(|r| {
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
            .lock()
            .unwrap()
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
            .or_else(|| self.roots.lock().unwrap().get(project).cloned())
            .ok_or_else(|| "unknown command project".to_string())?;
        let (program, args) = execution(&def, &root)?;
        let mut child = Command::new(program)
            .args(args)
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
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
        };
        self.runs.lock().unwrap().insert(
            id.clone(),
            LiveRun {
                view: view.clone(),
                child: Some(handle.clone()),
                next_seq: 0,
            },
        );
        {
            let mut order = self.order.lock().unwrap();
            order.push_front(id.clone());
            while order.len() > HISTORY_CAP {
                if let Some(old) = order.pop_back() {
                    self.runs.lock().unwrap().remove(&old);
                }
            }
        }
        if let Some(callback) = &self.on_change {
            callback(view.clone());
        }
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
                    let mut c = handle.lock().unwrap();
                    c.try_wait()
                };
                match result {
                    Ok(Some(status)) => {
                        let changed = if let Some(r) = runs.lock().unwrap().get_mut(&run_id) {
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
                        let changed = if let Some(r) = runs.lock().unwrap().get_mut(&run_id) {
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
    fn read_lines(&self, id: String, stream: &str, reader: impl std::io::Read + Send + 'static) {
        let runs = self.runs.clone();
        let stream = stream.to_owned();
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                if let Some(r) = runs.lock().unwrap().get_mut(&id) {
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
            let runs = self.runs.lock().unwrap();
            runs.get(id)
                .filter(|run| run.view.project == project)
                .and_then(|r| r.child.clone())
                .ok_or_else(|| "run is not active".to_string())?
        };
        child.lock().unwrap().kill().map_err(|e| e.to_string())?;
        let mut runs = self.runs.lock().unwrap();
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
