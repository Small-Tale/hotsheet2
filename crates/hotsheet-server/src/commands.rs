//! Bounded in-memory command execution/history. Exact argv comes from typed settings;
//! output is cursor-pollable while the process runs and cancellation kills the child.

use hotsheet_ticketing::commands::CommandDefinition;
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
    definitions: Arc<Mutex<Vec<CommandDefinition>>>,
    root: PathBuf,
    runs: Arc<Mutex<HashMap<String, LiveRun>>>,
    order: Arc<Mutex<VecDeque<String>>>,
    on_change: Option<ChangeCallback>,
}

impl CommandManager {
    pub fn new(root: PathBuf, definitions: Vec<CommandDefinition>) -> Self {
        Self {
            definitions: Arc::new(Mutex::new(definitions)),
            root,
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
        self.definitions.lock().unwrap().clone()
    }
    pub fn replace_definitions(&self, definitions: Vec<CommandDefinition>) {
        *self.definitions.lock().unwrap() = definitions;
    }
    pub fn list(&self) -> Vec<CommandRun> {
        let runs = self.runs.lock().unwrap();
        self.order
            .lock()
            .unwrap()
            .iter()
            .filter_map(|id| runs.get(id).map(|r| r.view.clone()))
            .collect()
    }
    pub fn get(&self, id: &str, after: u64) -> Option<CommandRun> {
        self.runs.lock().unwrap().get(id).map(|r| {
            let mut v = r.view.clone();
            v.output.retain(|o| o.seq > after);
            v
        })
    }
    pub fn start(&self, command_id: &str) -> Result<CommandRun, String> {
        let def = self
            .definitions
            .lock()
            .unwrap()
            .iter()
            .find(|d| d.id == command_id)
            .cloned()
            .ok_or_else(|| "unknown configured command".to_string())?;
        let mut child = Command::new(&def.program)
            .args(&def.args)
            .current_dir(&self.root)
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
        let child = {
            let runs = self.runs.lock().unwrap();
            runs.get(id)
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
