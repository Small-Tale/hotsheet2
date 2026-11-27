//! One running plugin process (docs/38 §38.5–§38.6): spawn with a cleared environment,
//! the `$/hello` handshake under a timeout, then a reader thread that routes responses
//! and stream items to waiting callers. When the process exits or breaks the protocol,
//! every pending call fails with `unavailable` and the supervisor is told.

use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel, sync_channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use hotsheet_plugin_protocol::handshake::{self, HandshakeError};
use hotsheet_plugin_protocol::wire::{MessageReader, write_message};
use hotsheet_plugin_protocol::{Message, Payload, PluginError, SHUTDOWN_METHOD, Transport, v1};

use crate::tree::ProcessTree;

/// Lines of plugin stderr kept for error reports.
const STDERR_TAIL_LINES: usize = 20;
const STDERR_LINE_MAX: usize = 500;

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a waiting caller receives.
pub(crate) enum Event {
    Item(Payload),
    Done(Result<Payload, PluginError>),
}

/// Why a call could not be sent.
pub(crate) enum StartError {
    /// The connection is shutting down on purpose (idle exit, replacement); retry on a
    /// fresh connection.
    Closing,
    /// The connection is gone.
    Failed(PluginError),
}

/// Everything needed to start one plugin process.
pub(crate) struct LaunchSpec {
    pub plugin_id: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub data_dir: PathBuf,
    pub env: Vec<(OsString, OsString)>,
    pub hello: v1::Hello,
    pub declared_capabilities: Vec<String>,
    pub handshake_timeout: Duration,
}

struct Pending {
    calls: HashMap<u64, Sender<Event>>,
    /// False once shutdown begins: no new user calls.
    accepting: bool,
    /// Set once the reader stops; never cleared.
    closed: Option<String>,
}

struct Lifecycle {
    shutdown_requested: bool,
    exited: bool,
}

pub(crate) struct Connection {
    pub(crate) plugin_id: String,
    pub(crate) pid: u32,
    pub(crate) transport: Transport,
    pub(crate) started_at: Instant,
    pub(crate) plugin_version: String,
    writer: Mutex<Option<BufWriter<ChildStdin>>>,
    pending: Mutex<Pending>,
    next_id: AtomicU64,
    lifecycle: Mutex<Lifecycle>,
    exited: Condvar,
    child: Mutex<Child>,
    tree: ProcessTree,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    last_activity: Mutex<Instant>,
}

/// The reader thread's view before the connection exists.
type ConnSlot = Receiver<Arc<Connection>>;

/// Runs on the reader thread after the connection closes.
pub(crate) type OnClose = Box<dyn FnOnce(&Arc<Connection>) + Send>;

/// The handshake outcome the reader thread hands to the starter.
type HandshakeOutcome = Result<(Transport, v1::HelloAck), HandshakeError>;

impl Connection {
    /// Starts the process and completes the handshake. `on_close` runs on the reader
    /// thread once the connection has closed and every pending call has failed.
    pub(crate) fn start(
        spec: LaunchSpec,
        on_close: OnClose,
    ) -> Result<Arc<Connection>, PluginError> {
        std::fs::create_dir_all(&spec.data_dir).map_err(|e| {
            PluginError::unavailable(format!(
                "plugin data directory {}: {e}",
                spec.data_dir.display()
            ))
        })?;
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .env_clear()
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .current_dir(&spec.data_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let tree = ProcessTree::prepare(&mut command);
        let mut child = command.spawn().map_err(|e| {
            PluginError::unavailable(format!("could not start plugin {}: {e}", spec.plugin_id))
        })?;
        let tree = tree.started(&mut child).map_err(PluginError::unavailable)?;
        let pid = child.id();
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            tree.kill(pid);
            // Unreachable with piped stdio; reap so no zombie remains.
            let _ = child.wait();
            return Err(PluginError::internal("plugin stdio was not captured"));
        };

        let stderr_tail: Arc<Mutex<VecDeque<String>>> = Arc::default();
        spawn_stderr_reader(spec.plugin_id.clone(), stderr, stderr_tail.clone());

        let mut writer = BufWriter::new(stdin);
        if let Err(e) = handshake::write_hello(&mut writer, &spec.hello) {
            tree.kill(pid);
            let _ = child.wait(); // reaping only; the write error is what is reported
            return Err(PluginError::unavailable(format!(
                "plugin {} closed its input before the handshake: {e}{}",
                spec.plugin_id,
                tail_suffix(&stderr_tail)
            )));
        }

        let (ack_tx, ack_rx) = sync_channel::<HandshakeOutcome>(1);
        let (conn_tx, conn_rx) = sync_channel::<Arc<Connection>>(1);
        let hello = spec.hello.clone();
        let declared = spec.declared_capabilities.clone();
        let plugin_id = spec.plugin_id.clone();
        std::thread::Builder::new()
            .name(format!("plugin-{plugin_id}-reader"))
            .spawn(move || reader_thread(stdout, hello, declared, ack_tx, conn_rx, on_close))
            .map_err(|e| PluginError::internal(format!("reader thread: {e}")))?;

        let failed = |child: &mut Child, why: String| {
            tree.kill(pid);
            let _ = child.wait(); // reaping only; `why` is the report
            Err(PluginError::unavailable(format!(
                "plugin {} failed its handshake: {why}{}",
                spec.plugin_id,
                tail_suffix(&stderr_tail)
            )))
        };
        let (transport, ack) = match ack_rx.recv_timeout(spec.handshake_timeout) {
            Ok(Ok(answer)) => answer,
            Ok(Err(e)) => return failed(&mut child, e.to_string()),
            Err(_) => {
                return failed(
                    &mut child,
                    format!("no answer within {} ms", spec.handshake_timeout.as_millis()),
                );
            }
        };

        let conn = Arc::new(Connection {
            plugin_id: spec.plugin_id,
            pid,
            transport,
            started_at: Instant::now(),
            plugin_version: ack.plugin.map(|p| p.version).unwrap_or_default(),
            writer: Mutex::new(Some(writer)),
            pending: Mutex::new(Pending {
                calls: HashMap::new(),
                accepting: true,
                closed: None,
            }),
            next_id: AtomicU64::new(1),
            lifecycle: Mutex::new(Lifecycle {
                shutdown_requested: false,
                exited: false,
            }),
            exited: Condvar::new(),
            child: Mutex::new(child),
            tree,
            stderr_tail,
            last_activity: Mutex::new(Instant::now()),
        });
        // The reader thread waits for this before routing any message.
        conn_tx
            .send(conn.clone())
            .map_err(|_| PluginError::unavailable("plugin reader stopped during the handshake"))?;
        Ok(conn)
    }

    /// Whether new calls can be sent.
    pub(crate) fn is_open(&self) -> bool {
        let pending = lock(&self.pending);
        pending.accepting && pending.closed.is_none()
    }

    /// Whether the supervisor stopped this connection on purpose.
    pub(crate) fn shutdown_requested(&self) -> bool {
        lock(&self.lifecycle).shutdown_requested
    }

    /// Why the connection closed, once it has.
    pub(crate) fn close_reason(&self) -> Option<String> {
        lock(&self.pending).closed.clone()
    }

    /// How long the connection has had no call in flight, or `None` while one is.
    pub(crate) fn idle_for(&self) -> Option<Duration> {
        if !lock(&self.pending).calls.is_empty() {
            return None;
        }
        Some(lock(&self.last_activity).elapsed())
    }

    fn touch(&self) {
        *lock(&self.last_activity) = Instant::now();
    }

    fn send(&self, message: &Message) -> std::io::Result<()> {
        let mut writer = lock(&self.writer);
        let Some(writer) = writer.as_mut() else {
            return Err(std::io::ErrorKind::BrokenPipe.into());
        };
        write_message(writer, self.transport, message)?;
        writer.flush()
    }

    /// Sends a call and returns its id and event channel.
    pub(crate) fn start_call(
        &self,
        method: &str,
        payload: Payload,
        deadline_ms: u64,
    ) -> Result<(u64, Receiver<Event>), StartError> {
        self.send_request(method, payload, deadline_ms, false)
    }

    fn send_request(
        &self,
        method: &str,
        payload: Payload,
        deadline_ms: u64,
        during_shutdown: bool,
    ) -> Result<(u64, Receiver<Event>), StartError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        {
            let mut pending = lock(&self.pending);
            if let Some(reason) = &pending.closed {
                return Err(StartError::Failed(self.stopped_error(reason)));
            }
            if !pending.accepting && !during_shutdown {
                return Err(StartError::Closing);
            }
            pending.calls.insert(id, tx);
        }
        self.touch();
        let message = Message::Request {
            id,
            method: method.to_string(),
            payload,
            deadline_ms,
        };
        if let Err(e) = self.send(&message) {
            lock(&self.pending).calls.remove(&id);
            return Err(StartError::Failed(PluginError::unavailable(format!(
                "could not send to plugin {}: {e}",
                self.plugin_id
            ))));
        }
        Ok((id, rx))
    }

    /// Stops waiting for `id` and asks the plugin to stop working on it.
    pub(crate) fn cancel(&self, id: u64) {
        if lock(&self.pending).calls.remove(&id).is_some() {
            self.touch();
            // Best effort: a closed plugin has nothing left to cancel.
            let _ = self.send(&Message::Cancel { id });
        }
    }

    fn stopped_error(&self, reason: &str) -> PluginError {
        PluginError::unavailable(format!(
            "plugin {} stopped: {reason}{}",
            self.plugin_id,
            tail_suffix(&self.stderr_tail)
        ))
    }

    fn route(&self, id: u64, event: Event) {
        let mut pending = lock(&self.pending);
        match event {
            Event::Item(payload) => {
                if let Some(tx) = pending.calls.get(&id) {
                    // The caller may have dropped its receiver; it then cancels.
                    let _ = tx.send(Event::Item(payload));
                }
            }
            Event::Done(outcome) => {
                if let Some(tx) = pending.calls.remove(&id) {
                    drop(pending);
                    self.touch();
                    let _ = tx.send(Event::Done(outcome)); // see above
                }
            }
        }
    }

    /// Marks the connection closed, fails every pending call, and reaps the process.
    fn close(&self, reason: String) {
        let waiting: Vec<Sender<Event>> = {
            let mut pending = lock(&self.pending);
            if pending.closed.is_none() {
                pending.closed = Some(reason.clone());
            }
            pending.accepting = false;
            pending.calls.drain().map(|(_, tx)| tx).collect()
        };
        lock(&self.writer).take();
        let shutting_down = self.shutdown_requested();
        if !shutting_down {
            // The plugin broke its output or the protocol; nothing it does now is useful.
            self.tree.kill(self.pid);
        }
        let status = lock(&self.child).wait();
        {
            let mut lifecycle = lock(&self.lifecycle);
            lifecycle.exited = true;
            self.exited.notify_all();
        }
        if let Ok(status) = status
            && !shutting_down
        {
            let mut pending = lock(&self.pending);
            if let Some(reason) = pending.closed.as_mut() {
                reason.push_str(&format!(" ({status})"));
            }
        }
        let error = self.stopped_error(&self.close_reason().unwrap_or(reason));
        for tx in waiting {
            let _ = tx.send(Event::Done(Err(error.clone()))); // receivers may be gone
        }
    }

    /// Waits up to `timeout` for the process to exit. Returns whether it did.
    pub(crate) fn wait_exited(&self, timeout: Duration) -> bool {
        let lifecycle = lock(&self.lifecycle);
        let (lifecycle, _) = self
            .exited
            .wait_timeout_while(lifecycle, timeout, |l| !l.exited)
            .unwrap_or_else(PoisonError::into_inner);
        lifecycle.exited
    }

    /// Orderly stop (docs/38 §38.6): `$/shutdown` with `grace`, close stdin, TERM, KILL.
    pub(crate) fn shutdown(&self, grace: Duration) {
        {
            let mut lifecycle = lock(&self.lifecycle);
            if lifecycle.exited {
                return;
            }
            lifecycle.shutdown_requested = true;
        }
        lock(&self.pending).accepting = false;
        if let Ok((_, rx)) = self.send_request(
            SHUTDOWN_METHOD,
            Payload::empty(self.transport),
            grace.as_millis() as u64,
            true,
        ) {
            // The answer, a timeout or a closed plugin all lead to the same next step.
            let _ = rx.recv_timeout(grace);
        }
        lock(&self.writer).take();
        if self.wait_exited(grace) {
            return;
        }
        self.tree.terminate(self.pid);
        if self.wait_exited(grace) {
            return;
        }
        self.tree.kill(self.pid);
        if !self.wait_exited(grace) {
            // No job object or group (unusual platforms): fall back to the root only.
            if let Ok(mut child) = self.child.try_lock() {
                let _ = child.kill(); // last resort; the reader thread reaps it
            }
        }
    }
}

fn tail_suffix(tail: &Mutex<VecDeque<String>>) -> String {
    let tail = lock(tail);
    if tail.is_empty() {
        String::new()
    } else {
        format!(
            "; last stderr: {}",
            tail.iter().cloned().collect::<Vec<_>>().join(" | ")
        )
    }
}

fn spawn_stderr_reader(
    plugin_id: String,
    stderr: std::process::ChildStderr,
    tail: Arc<Mutex<VecDeque<String>>>,
) {
    // A failed thread spawn only loses stderr capture; the plugin still runs.
    let _ = std::thread::Builder::new()
        .name(format!("plugin-{plugin_id}-stderr"))
        .spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(mut line) = line else { break };
                if line.len() > STDERR_LINE_MAX {
                    let mut end = STDERR_LINE_MAX;
                    while !line.is_char_boundary(end) {
                        end -= 1;
                    }
                    line.truncate(end);
                }
                tracing::debug!(plugin = %plugin_id, "{line}");
                let mut tail = lock(&tail);
                if tail.len() == STDERR_TAIL_LINES {
                    tail.pop_front();
                }
                tail.push_back(line);
            }
        });
}

fn reader_thread(
    stdout: std::process::ChildStdout,
    hello: v1::Hello,
    declared: Vec<String>,
    ack_tx: std::sync::mpsc::SyncSender<HandshakeOutcome>,
    conn_rx: ConnSlot,
    on_close: OnClose,
) {
    let mut reader = MessageReader::new(BufReader::new(stdout), Transport::Json);
    let mut logs = Vec::new();
    let answer = handshake::read_hello_ack(&mut reader, &mut logs).and_then(|ack| {
        let transport = handshake::accept_ack(&hello, &ack, &declared)?;
        Ok((transport, ack))
    });
    let ok: Option<Transport> = answer.as_ref().ok().map(|(t, _)| *t);
    let _ = ack_tx.send(answer); // the starter may have timed out and gone
    let Some(transport) = ok else {
        return;
    };
    // The starter sends the connection, or drops the channel if it gave up.
    let Ok(conn) = conn_rx.recv() else {
        return;
    };
    for (level, message) in logs {
        log(&conn.plugin_id, &level, &message);
    }
    reader.set_transport(transport);
    let reason = loop {
        match reader.read() {
            Ok(Some(Message::Response { id, outcome })) => conn.route(id, Event::Done(outcome)),
            Ok(Some(Message::StreamItem { id, payload })) => conn.route(id, Event::Item(payload)),
            Ok(Some(Message::Log { level, message })) => log(&conn.plugin_id, &level, &message),
            Ok(Some(Message::Request { id, method, .. })) => {
                // v1 implements no host methods yet (docs/38 §38.5); answer instead of hanging.
                let _ = conn.send(&Message::Response {
                    id,
                    outcome: Err(PluginError::unsupported(method)),
                });
            }
            Ok(Some(Message::Cancel { .. })) => {}
            Ok(None) => break "it closed its output".to_string(),
            Err(e) => break format!("protocol error: {e}"),
        }
    };
    conn.close(reason);
    on_close(&conn);
}

fn log(plugin_id: &str, level: &str, message: &str) {
    match level {
        "error" => tracing::error!(plugin = %plugin_id, "{message}"),
        "warn" => tracing::warn!(plugin = %plugin_id, "{message}"),
        "info" => tracing::info!(plugin = %plugin_id, "{message}"),
        _ => tracing::debug!(plugin = %plugin_id, "{message}"),
    }
}
