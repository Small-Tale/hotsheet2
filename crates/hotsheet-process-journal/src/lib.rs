//! Machine-local child-process provenance (HS2-GDCMXE). Records contain identifiers and
//! lifecycle facts only. A launch is a unique ID, never just a reusable OS PID.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_RECORD_BYTES: u64 = 16 * 1024;
const MAX_ENDED: usize = 1024;
const MAX_UNRESOLVED: usize = 1024;
const RETAIN_MS: u64 = 30 * 24 * 60 * 60 * 1000;
const STALE_TEMP_SECS: u64 = 60 * 60;

/// A fixed vocabulary prevents commands, arguments, and customer content entering records.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    TerminalShell,
    TerminalAi,
    TerminalBroker,
    AiChannel,
    AiAppServer,
    AiAcp,
    AiSpawn,
    AiProbe,
    CodexQueue,
    CodexDaemon,
    CliExternal,
    CliSetup,
    CliMigrator,
    CodeReviewDifftool,
    ServerCommand,
    RepositoryHelper,
    BrokerSupervisor,
    GuidanceGit,
    Rulesync,
    /// The detached watch-Job monitor started by `hotsheet-cli job watch start` (HS2-5TC1GQ).
    JobMonitor,
    /// HS2-M7EV18: the detached runner of one hosted validation step.
    ValidationStep,
}

/// The observed end of one child; a crash leaves this absent rather than inventing an exit.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    Completed,
    Failed,
    Interrupted,
    Killed,
    Unknown,
}

/// Caller-supplied identifiers. `binary_revision` describes the Hot Sheet build supervising
/// the child; it is not the child command, path, arguments, or environment.
#[derive(Clone, Debug)]
pub struct LaunchMeta {
    pub pid: u32,
    pub supervisor_pid: u32,
    pub supervisor: String,
    pub purpose: Purpose,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub binary_revision: Option<String>,
}

/// Fixed provenance shared by commands whose PID is only known after spawning. It never
/// inspects the command, arguments, environment, or output (HS2-G9TF60).
#[derive(Clone, Debug)]
pub struct LaunchContext {
    pub journal: ProcessJournal,
    pub supervisor: &'static str,
    pub purpose: Purpose,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub binary_revision: Option<String>,
}

impl LaunchContext {
    pub fn spawn(&self, command: &mut Command) -> io::Result<TrackedChild> {
        let child = command.spawn()?;
        let record = self.journal.start(LaunchMeta {
            pid: child.id(),
            supervisor_pid: std::process::id(),
            supervisor: self.supervisor.to_owned(),
            purpose: self.purpose,
            project_id: self.project_id.clone(),
            session_id: self.session_id.clone(),
            binary_revision: self.binary_revision.clone(),
        });
        let launch_id = match record {
            Ok(record) => Some(record.launch_id),
            Err(error) => {
                eprintln!(
                    "Hot Sheet process journal unavailable: {}",
                    start_error_summary(&error)
                );
                None
            }
        };
        Ok(TrackedChild {
            child,
            journal: self.journal.clone(),
            launch_id,
            killed: false,
        })
    }

    pub fn status(&self, command: &mut Command) -> io::Result<ExitStatus> {
        self.spawn(command)?.wait()
    }

    pub fn output(&self, command: &mut Command) -> io::Result<Output> {
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        self.spawn(command)?.wait_with_output()
    }

    /// The caller intentionally hands ownership to another process or the OS. The launch
    /// remains unresolved because this supervisor has not observed an exit. The child
    /// inherits no stray descriptor (see [`close_inherited_descriptors`]).
    pub fn detached(&self, command: &mut Command) -> io::Result<()> {
        self.spawn_detached(command).map(|_| ())
    }

    /// Like [`LaunchContext::spawn`] for a child that outlives this request (a broker or a
    /// monitor), keeping the handle for startup diagnostics. The child inherits no stray
    /// descriptor and gets null standard streams (see [`LaunchContext::spawn_detached_as`]).
    pub fn spawn_detached(&self, command: &mut Command) -> io::Result<DetachedChild> {
        self.spawn_detached_as(command, Detach::default())
    }

    /// [`LaunchContext::detached`] that also leaves the launcher's process group or session.
    pub fn detached_as(&self, command: &mut Command, detach: Detach) -> io::Result<()> {
        self.spawn_detached_as(command, detach).map(|_| ())
    }

    /// [`LaunchContext::spawn_detached`] that also leaves the launcher's process group or
    /// session (`detach`). The child's standard streams are null and it inherits no
    /// descriptor of the launcher's besides them (HS2-WEYR63, HS2-ETMKCW).
    ///
    /// HS2-Y4VDBK: on macOS the child is started with `posix_spawn` and its attributes
    /// (`POSIX_SPAWN_CLOEXEC_DEFAULT`, `POSIX_SPAWN_SETSID`, `POSIX_SPAWN_SETPGROUP`),
    /// never by forking this multi-threaded process, whose forked child can abort in
    /// libSystem's fork handlers before `exec` (HS2-H1EK8R). That path reads the command
    /// through its public getters: program, arguments, working directory and variable
    /// changes. A command that must not inherit this process's environment says so with
    /// [`replace_environment`], since `env_clear` is not observable there. Elsewhere the
    /// standard library spawns it as before, with `pre_exec` hooks for the session and
    /// the descriptor closing.
    pub fn spawn_detached_as(
        &self,
        command: &mut Command,
        detach: Detach,
    ) -> io::Result<DetachedChild> {
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(target_os = "macos")]
        {
            let pid = spawn_attributes::spawn(command, detach)?;
            let launch_id = self.record(pid);
            Ok(DetachedChild {
                inner: Detached::Spawned { pid, status: None },
                journal: self.journal.clone(),
                launch_id,
                killed: false,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                if detach.process_group {
                    command.process_group(0);
                }
                if detach.session {
                    // SAFETY: setsid is async-signal-safe; it only moves the forked child
                    // into its own session.
                    unsafe {
                        command.pre_exec(|| {
                            if libc::setsid() < 0 {
                                return Err(io::Error::last_os_error());
                            }
                            Ok(())
                        });
                    }
                }
            }
            #[cfg(not(unix))]
            let _ = detach;
            close_inherited_descriptors(command);
            let child = self.spawn(command)?;
            Ok(DetachedChild {
                inner: Detached::Tracked(child),
                journal: self.journal.clone(),
                launch_id: None,
                killed: false,
            })
        }
    }

    #[cfg(target_os = "macos")]
    fn record(&self, pid: u32) -> Option<String> {
        match self.journal.start(LaunchMeta {
            pid,
            supervisor_pid: std::process::id(),
            supervisor: self.supervisor.to_owned(),
            purpose: self.purpose,
            project_id: self.project_id.clone(),
            session_id: self.session_id.clone(),
            binary_revision: self.binary_revision.clone(),
        }) {
            Ok(record) => Some(record.launch_id),
            Err(error) => {
                eprintln!(
                    "Hot Sheet process journal unavailable: {}",
                    start_error_summary(&error)
                );
                None
            }
        }
    }
}

/// How a detached child leaves its launcher (HS2-Y4VDBK).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Detach {
    /// Lead its own process group, so a terminal's Ctrl-C does not reach it.
    pub process_group: bool,
    /// Lead its own session, so stopping the launcher's job never signals it.
    pub session: bool,
}

/// HS2-Y4VDBK: give `command` exactly `vars` as its environment, in a way every spawn path
/// can read. On macOS each inherited variable not in `vars` is removed by name, because
/// the `posix_spawn` path of [`LaunchContext::spawn_detached_as`] cannot observe
/// `env_clear`; elsewhere the environment is cleared as before.
pub fn replace_environment<K, V>(command: &mut Command, vars: impl IntoIterator<Item = (K, V)>)
where
    K: AsRef<std::ffi::OsStr>,
    V: AsRef<std::ffi::OsStr>,
{
    let vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = vars
        .into_iter()
        .map(|(key, value)| (key.as_ref().to_owned(), value.as_ref().to_owned()))
        .collect();
    #[cfg(target_os = "macos")]
    for (key, _) in std::env::vars_os() {
        if !vars.iter().any(|(kept, _)| *kept == key) {
            command.env_remove(key);
        }
    }
    #[cfg(not(target_os = "macos"))]
    command.env_clear();
    command.envs(vars);
}

/// A detached child (HS2-Y4VDBK): its pid, its observed exit, and its journal record.
/// Dropping it neither waits nor kills.
pub struct DetachedChild {
    inner: Detached,
    journal: ProcessJournal,
    launch_id: Option<String>,
    killed: bool,
}

enum Detached {
    /// Spawned by the standard library (off macOS); the tracked child keeps the record.
    #[cfg_attr(target_os = "macos", allow(dead_code))] // HS2-Y4VDBK: the std path is off macOS
    Tracked(TrackedChild),
    /// Spawned with `posix_spawn` (macOS); reaped here by pid.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))] // HS2-Y4VDBK: macOS only
    Spawned {
        pid: u32,
        status: Option<ExitStatus>,
    },
}

impl DetachedChild {
    /// The child's process id.
    pub fn id(&self) -> u32 {
        match &self.inner {
            Detached::Tracked(child) => child.id(),
            Detached::Spawned { pid, .. } => *pid,
        }
    }

    /// The child's exit once it has ended, reaping it; `None` while it runs.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        match &mut self.inner {
            Detached::Tracked(child) => child.try_wait(),
            Detached::Spawned { pid, status } => {
                if status.is_none() {
                    *status = reap(*pid, false)?;
                }
                let observed = *status;
                if let Some(observed) = observed {
                    self.finish(observed);
                }
                Ok(observed)
            }
        }
    }

    /// Wait for the child to end and reap it.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        match &mut self.inner {
            Detached::Tracked(child) => child.wait(),
            Detached::Spawned { pid, status } => {
                let observed = match *status {
                    Some(observed) => observed,
                    None => reap(*pid, true)?.ok_or_else(|| io::Error::other("no exit status"))?,
                };
                *status = Some(observed);
                self.finish(observed);
                Ok(observed)
            }
        }
    }

    /// Kill the child unless it was already reaped (then its pid may name another process).
    pub fn kill(&mut self) -> io::Result<()> {
        match &mut self.inner {
            Detached::Tracked(child) => child.kill(),
            Detached::Spawned { pid, status } => {
                if status.is_none() {
                    kill_spawned(*pid)?;
                    self.killed = true;
                }
                Ok(())
            }
        }
    }

    fn finish(&mut self, status: ExitStatus) {
        if let Some(id) = self.launch_id.take() {
            let reason = if self.killed {
                ExitReason::Killed
            } else if status.success() {
                ExitReason::Completed
            } else {
                ExitReason::Failed
            };
            let _ = self.journal.finish(&id, reason);
        }
    }
}

/// Reap child `pid` (blocking when `block`), or `None` while it still runs.
#[cfg(unix)]
fn reap(pid: u32, block: bool) -> io::Result<Option<ExitStatus>> {
    use std::os::unix::process::ExitStatusExt;
    let pid = libc::pid_t::try_from(pid).map_err(io::Error::other)?;
    loop {
        let mut status = 0;
        // SAFETY: waits only for this process's own child; `status` is a local out-parameter.
        let reaped =
            unsafe { libc::waitpid(pid, &mut status, if block { 0 } else { libc::WNOHANG }) };
        if reaped == pid {
            return Ok(Some(ExitStatus::from_raw(status)));
        }
        if reaped == 0 {
            return Ok(None);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(not(unix))]
fn reap(_pid: u32, _block: bool) -> io::Result<Option<ExitStatus>> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// SIGKILL child `pid`, which the caller has not reaped, so it cannot name another process.
#[cfg(unix)]
fn kill_spawned(pid: u32) -> io::Result<()> {
    let pid = libc::pid_t::try_from(pid).map_err(io::Error::other)?;
    // SAFETY: the pid is this process's own unreaped child, so it cannot name another process.
    if unsafe { libc::kill(pid, libc::SIGKILL) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// HS2-AS2285: only macOS launches through `posix_spawn`, so other targets never hold a
/// `Detached::Spawned` child; like [`reap`], killing one is unsupported rather than a silent
/// success that would record the launch as killed.
#[cfg(not(unix))]
fn kill_spawned(_pid: u32) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// The state a startup check reads from a child it launched (HS2-Y4VDBK): its pid and
/// whether it has exited. Implemented by both [`TrackedChild`] and [`DetachedChild`].
pub trait ObservedChild {
    fn id(&self) -> u32;
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>>;
}

impl ObservedChild for TrackedChild {
    fn id(&self) -> u32 {
        TrackedChild::id(self)
    }
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        TrackedChild::try_wait(self)
    }
}

impl ObservedChild for DetachedChild {
    fn id(&self) -> u32 {
        DetachedChild::id(self)
    }
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        DetachedChild::try_wait(self)
    }
}

/// HS2-Y4VDBK: a detached launch through `posix_spawn` and its macOS attributes.
#[cfg(target_os = "macos")]
mod spawn_attributes {
    use super::Detach;
    use std::ffi::{CString, OsStr, OsString};
    use std::io;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::process::Command;

    /// `<sys/spawn.h>`; the `libc` crate does not define it.
    const POSIX_SPAWN_SETSID: libc::c_short = 0x0400;

    unsafe extern "C" {
        /// macOS 10.15+; `posix_spawn_file_actions_addchdir` replaces it from macOS 26.
        fn posix_spawn_file_actions_addchdir_np(
            actions: *mut libc::posix_spawn_file_actions_t,
            path: *const libc::c_char,
        ) -> libc::c_int;
    }

    fn c_string(value: &OsStr) -> io::Result<CString> {
        CString::new(value.as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
    }

    fn check(code: libc::c_int) -> io::Result<()> {
        if code == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(code))
        }
    }

    /// Owns the attribute and file-action objects so every exit path destroys them.
    struct Spawn {
        attributes: libc::posix_spawnattr_t,
        actions: libc::posix_spawn_file_actions_t,
    }

    impl Drop for Spawn {
        fn drop(&mut self) {
            // SAFETY: both objects were initialized in `spawn` before this guard was built.
            unsafe {
                libc::posix_spawnattr_destroy(&mut self.attributes);
                libc::posix_spawn_file_actions_destroy(&mut self.actions);
            }
        }
    }

    /// The environment the child sees: this process's, with the command's changes.
    fn environment(command: &Command) -> Vec<(OsString, OsString)> {
        let mut vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        for (key, value) in command.get_envs() {
            vars.retain(|(name, _)| name.as_os_str() != key);
            if let Some(value) = value {
                vars.push((key.to_owned(), value.to_owned()));
            }
        }
        vars
    }

    pub(super) fn spawn(command: &Command, detach: Detach) -> io::Result<u32> {
        let vars = environment(command);
        let search = vars
            .iter()
            .rev()
            .find(|(name, _)| name == "PATH")
            .map(|(_, value)| value.as_os_str());
        let cwd = command.get_current_dir();
        // HS2-E5B0K0: resolved as execvp would for the child's own PATH and directory.
        let program = hotsheet_ticketing::git::program_path(command.get_program(), search, cwd)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        let path = c_string(program.as_os_str())?;
        let mut argv_owned = vec![c_string(command.get_program())?];
        for arg in command.get_args() {
            argv_owned.push(c_string(arg)?);
        }
        let envp_owned = vars
            .into_iter()
            .map(|(key, value)| {
                let mut pair = key.into_vec();
                pair.push(b'=');
                pair.extend(value.into_vec());
                CString::new(pair).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let mut argv: Vec<*mut libc::c_char> = argv_owned
            .iter()
            .map(|arg| arg.as_ptr().cast_mut())
            .collect();
        argv.push(std::ptr::null_mut());
        let mut envp: Vec<*mut libc::c_char> = envp_owned
            .iter()
            .map(|pair| pair.as_ptr().cast_mut())
            .collect();
        envp.push(std::ptr::null_mut());
        let null = c"/dev/null";
        let directory = cwd.map(|cwd| c_string(cwd.as_os_str())).transpose()?;

        // SAFETY: zeroed objects are immediately initialized by their init functions and
        // destroyed by the guard; every pointer passed below outlives the calls.
        unsafe {
            let mut spawn = Spawn {
                attributes: std::mem::zeroed(),
                actions: std::mem::zeroed(),
            };
            check(libc::posix_spawnattr_init(&mut spawn.attributes))?;
            if let Err(error) = check(libc::posix_spawn_file_actions_init(&mut spawn.actions)) {
                libc::posix_spawnattr_destroy(&mut spawn.attributes);
                std::mem::forget(spawn);
                return Err(error);
            }
            // Every descriptor not opened below is closed in the child.
            let mut flags = libc::POSIX_SPAWN_CLOEXEC_DEFAULT
                | libc::POSIX_SPAWN_SETSIGMASK
                | libc::POSIX_SPAWN_SETSIGDEF;
            if detach.session {
                flags |= libc::c_int::from(POSIX_SPAWN_SETSID);
            } else if detach.process_group {
                flags |= libc::POSIX_SPAWN_SETPGROUP;
                check(libc::posix_spawnattr_setpgroup(&mut spawn.attributes, 0))?;
            }
            let flags = libc::c_short::try_from(flags).map_err(io::Error::other)?;
            check(libc::posix_spawnattr_setflags(&mut spawn.attributes, flags))?;
            // As the standard library does: an empty signal mask, SIGPIPE back to default.
            let mut empty: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut empty);
            check(libc::posix_spawnattr_setsigmask(
                &mut spawn.attributes,
                &empty,
            ))?;
            let mut defaults: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut defaults);
            libc::sigaddset(&mut defaults, libc::SIGPIPE);
            check(libc::posix_spawnattr_setsigdefault(
                &mut spawn.attributes,
                &defaults,
            ))?;
            for (fd, mode) in [
                (0, libc::O_RDONLY),
                (1, libc::O_WRONLY),
                (2, libc::O_WRONLY),
            ] {
                check(libc::posix_spawn_file_actions_addopen(
                    &mut spawn.actions,
                    fd,
                    null.as_ptr(),
                    mode,
                    0,
                ))?;
            }
            if let Some(directory) = &directory {
                check(posix_spawn_file_actions_addchdir_np(
                    &mut spawn.actions,
                    directory.as_ptr(),
                ))?;
            }
            let mut pid: libc::pid_t = 0;
            check(libc::posix_spawn(
                &mut pid,
                path.as_ptr(),
                &spawn.actions,
                &spawn.attributes,
                argv.as_ptr(),
                envp.as_ptr(),
            ))?;
            u32::try_from(pid).map_err(io::Error::other)
        }
    }
}

/// HS2-WEYR63, HS2-ETMKCW: a long-lived child must hold nothing of its launcher's but
/// what it is given. On macOS the standard library creates a pipe and sets close-on-exec
/// in a second step, so a process spawned on another thread in that instant inherits the
/// pipe, and `posix_spawn` passes it on to every later child. A detached child keeps such
/// a descriptor for its whole life, and whoever reads the other end never sees its end.
/// This closes, in the child before exec, every descriptor above stdio that is not
/// close-on-exec. Close-on-exec descriptors (the standard library's own exec status pipe
/// among them) close by themselves.
///
/// HS2-8CGT4R: Windows starts every child with handle inheritance on, so it receives every
/// inheritable handle of its launcher. The launcher's standard handles are inheritable
/// whenever its own launcher gave them, such as the pipe a caller reads the launcher's
/// output from, and a detached child would hold that pipe open for its whole life. So
/// on Windows this marks the launcher's standard handles not inheritable. A later child
/// given `Stdio::inherit` still gets them: the standard library passes it inheritable
/// duplicates. Other handles Hot Sheet opens are not inheritable to begin with.
pub fn close_inherited_descriptors(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: sysconf only reads a system limit, before the fork.
        let limit = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
        let limit = i32::try_from(limit.clamp(256, 65_536)).unwrap_or(65_536);
        // SAFETY: runs in the forked child before exec and makes only fcntl and close
        // calls, which are async-signal-safe.
        unsafe {
            command.pre_exec(move || {
                for fd in 3..limit {
                    let flags = libc::fcntl(fd, libc::F_GETFD);
                    if flags >= 0 && flags & libc::FD_CLOEXEC == 0 {
                        libc::close(fd);
                    }
                }
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
        let _ = command;
        for handle in [
            std::io::stdin().as_raw_handle(),
            std::io::stdout().as_raw_handle(),
            std::io::stderr().as_raw_handle(),
        ] {
            if !handle.is_null() {
                // SAFETY: the handle is this process's live standard handle; only its
                // inheritance flag changes. A failure (a console pseudo-handle) is harmless.
                unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    let _ = command;
}

/// A child whose observed exit updates its journal record. Dropping a still-running child
/// deliberately leaves an unresolved record for later diagnostics.
pub struct TrackedChild {
    child: Child,
    journal: ProcessJournal,
    launch_id: Option<String>,
    killed: bool,
}

impl TrackedChild {
    fn finish(&mut self, status: ExitStatus) {
        if let Some(id) = self.launch_id.take() {
            let reason = if self.killed {
                ExitReason::Killed
            } else if status.success() {
                ExitReason::Completed
            } else {
                ExitReason::Failed
            };
            let _ = self.journal.finish(&id, reason);
        }
    }

    /// The child's process id.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.child.wait()?;
        self.finish(status);
        Ok(status)
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if let Some(status) = status {
            self.finish(status);
        }
        Ok(status)
    }

    pub fn wait_with_output(self) -> io::Result<Output> {
        let Self {
            child,
            journal,
            launch_id,
            killed,
        } = self;
        let output = child.wait_with_output()?;
        if let Some(id) = launch_id {
            let reason = if killed {
                ExitReason::Killed
            } else if output.status.success() {
                ExitReason::Completed
            } else {
                ExitReason::Failed
            };
            let _ = journal.finish(&id, reason);
        }
        Ok(output)
    }

    pub fn kill(&mut self) -> io::Result<()> {
        self.child.kill()?;
        self.killed = true;
        Ok(())
    }
}

impl std::ops::Deref for TrackedChild {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        &self.child
    }
}

impl std::ops::DerefMut for TrackedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessRecord {
    pub launch_id: String,
    pub pid: u32,
    pub supervisor_pid: u32,
    pub supervisor: String,
    pub purpose: Purpose,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub binary_revision: Option<String>,
    pub started_ms: u64,
    pub ended_ms: Option<u64>,
    pub exit_reason: Option<ExitReason>,
}

/// A directory explicitly rooted in the caller's machine home, so tests never touch a user's
/// diagnostic store. Different supervisors can write distinct launch IDs concurrently.
#[derive(Clone, Debug)]
pub struct ProcessJournal {
    dir: PathBuf,
}

impl ProcessJournal {
    pub fn at(machine_home: impl AsRef<Path>) -> Self {
        Self {
            dir: machine_home.as_ref().join("processes"),
        }
    }

    pub fn start(&self, meta: LaunchMeta) -> io::Result<ProcessRecord> {
        self.prepare_dir()?;
        let record = ProcessRecord {
            launch_id: ulid::Ulid::new().to_string(),
            pid: meta.pid,
            supervisor_pid: meta.supervisor_pid,
            supervisor: bounded_id("supervisor", &meta.supervisor)?,
            purpose: meta.purpose,
            project_id: optional_id("project_id", meta.project_id.as_deref())?,
            session_id: optional_id("session_id", meta.session_id.as_deref())?,
            binary_revision: optional_id("binary_revision", meta.binary_revision.as_deref())?,
            started_ms: now_ms(),
            ended_ms: None,
            exit_reason: None,
        };
        let mut temp = tempfile::NamedTempFile::new_in(&self.dir)?;
        serde_json::to_writer(&mut temp, &record)?;
        temp.flush()?;
        temp.as_file().sync_all()?;
        temp.persist_noclobber(self.path(&record.launch_id))
            .map_err(|e| e.error)?;
        if self.prune().is_err() {
            eprintln!("Hot Sheet process journal retention needs repair");
        }
        Ok(record)
    }

    /// Only the launch's owner should finish it. A missing record is an error, so a stale
    /// supervisor cannot silently overwrite a pruned or replaced launch.
    pub fn finish(&self, launch_id: &str, reason: ExitReason) -> io::Result<ProcessRecord> {
        ulid::Ulid::from_string(launch_id)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid launch ID"))?;
        let path = self.path(launch_id);
        let mut record = read_record(&path)?;
        if record.launch_id != launch_id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "launch ID mismatch",
            ));
        }
        if record.ended_ms.is_some() {
            return Ok(record);
        }
        record.ended_ms = Some(now_ms());
        record.exit_reason = Some(reason);
        let mut temp = tempfile::NamedTempFile::new_in(&self.dir)?;
        serde_json::to_writer(&mut temp, &record)?;
        temp.flush()?;
        temp.as_file().sync_all()?;
        hotsheet_sync::files::persist(temp, path)?;
        Ok(record)
    }

    /// Read recent records in start order. An unended record after its supervisor disappears
    /// is evidence for investigation, not proof that the child exited. Launches that start in
    /// the same millisecond list in launch-id order (HS2-ZKP7HW): deterministic, but a random
    /// same-millisecond ULID does not record which of them started first.
    pub fn list(&self) -> io::Result<Vec<ProcessRecord>> {
        self.reject_non_directory()?;
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut records = Vec::new();
        for entry in entries {
            let entry = entry?;
            if entry.path().extension().is_some_and(|ext| ext == "json") {
                records.push(read_record(&entry.path())?);
            }
        }
        records.sort_by_key(|r| (r.started_ms, r.launch_id.clone()));
        Ok(records)
    }

    /// Retain the most recent 1024 ended and 1024 unresolved launches for up to 30 days.
    /// Every start performs this sweep; no background timer or network request is needed.
    pub fn prune(&self) -> io::Result<()> {
        self.reject_non_directory()?;
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let mut records = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                match read_record(&path) {
                    Ok(record) => records.push(record),
                    Err(_) => {
                        // A damaged record cannot establish any child identity. Remove it so
                        // future starts remain bounded; the read-only inventory reports the
                        // corruption until the next successful launch performs this sweep.
                        fs::remove_file(path)?;
                    }
                }
            } else if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".tmp"))
                && fs::symlink_metadata(&path)?.file_type().is_file()
                && fs::metadata(&path)?
                    .modified()?
                    .elapsed()
                    .is_ok_and(|age| age.as_secs() > STALE_TEMP_SECS)
            {
                // A crash can leave an atomic-write temporary file. Never remove a
                // concurrent writer's fresh file; sweep abandoned ones on later starts.
                fs::remove_file(path)?;
            }
        }
        records.sort_by_key(|r| (std::cmp::Reverse(r.started_ms), r.launch_id.clone()));
        let mut ended = 0;
        let mut unresolved = 0;
        let cutoff = now_ms().saturating_sub(RETAIN_MS);
        for record in records {
            let count = if record.ended_ms.is_some() {
                ended += 1;
                ended
            } else {
                unresolved += 1;
                unresolved
            };
            let cap = if record.ended_ms.is_some() {
                MAX_ENDED
            } else {
                MAX_UNRESOLVED
            };
            if count > cap || record.started_ms < cutoff {
                match fs::remove_file(self.path(&record.launch_id)) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(())
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    fn prepare_dir(&self) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        self.reject_non_directory()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    fn reject_non_directory(&self) -> io::Result<()> {
        match fs::symlink_metadata(&self.dir) {
            Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "process journal path is not a directory",
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// HS2-NNA91A: a refused identifier names its field with a fixed string. The refused
/// value is never part of the error, because it may be a machine-local path or content.
#[derive(Debug)]
struct RefusedIdentifier {
    field: &'static str,
}

impl std::fmt::Display for RefusedIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid diagnostic identifier: {}", self.field)
    }
}

impl std::error::Error for RefusedIdentifier {}

/// The text a caller logs when [`ProcessJournal::start`] fails: the refused field for an
/// identifier refusal, otherwise only the error kind. Other start errors (for example a
/// temporary-file error) can carry the machine-local journal path, so they are not echoed.
pub fn start_error_summary(error: &io::Error) -> String {
    match error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<RefusedIdentifier>())
    {
        Some(refused) => refused.to_string(),
        None => error.kind().to_string(),
    }
}

fn optional_id(field: &'static str, value: Option<&str>) -> io::Result<Option<String>> {
    value.map(|value| bounded_id(field, value)).transpose()
}

fn bounded_id(field: &'static str, value: &str) -> io::Result<String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            RefusedIdentifier { field },
        ));
    }
    Ok(value.to_owned())
}

fn read_record(path: &Path) -> io::Result<ProcessRecord> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "process record is not a file",
        ));
    }
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > MAX_RECORD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "oversized process record",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "oversized process record",
        ));
    }
    let record: ProcessRecord = serde_json::from_slice(&bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    ulid::Ulid::from_string(&record.launch_id)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid launch ID"))?;
    for value in [
        Some(record.supervisor.as_str()),
        record.project_id.as_deref(),
        record.session_id.as_deref(),
        record.binary_revision.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        bounded_id("identifier", value).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid diagnostic identifier")
        })?;
    }
    if record.ended_ms.is_some() != record.exit_reason.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "incomplete exit record",
        ));
    }
    if !path
        .file_stem()
        .is_some_and(|stem| stem == record.launch_id.as_str())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "launch file mismatch",
        ));
    }
    Ok(record)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HS2-ETMKCW: a detached child holds no descriptor of its launcher's that is not
    /// close-on-exec; an ordinary tracked child still inherits it, as before.
    #[cfg(unix)]
    #[test]
    fn hs2_etmkcw_detached_children_inherit_no_stray_descriptor() {
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
        // HS2-2CT6AT: the fixture leaves a pipe writer inheritable on purpose, so under a shared
        // `cargo test` process a sibling test's child could inherit it and hold the pipe open. The
        // body runs in a re-executed process that runs only this test.
        const ROLE: &str = "HS2_2CT6AT_ISOLATED";
        const NAME: &str = "tests::hs2_etmkcw_detached_children_inherit_no_stray_descriptor";
        if std::env::var_os(ROLE).is_none() {
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", NAME, "--nocapture", "--test-threads=1"])
                .env(ROLE, "1")
                .stdin(std::process::Stdio::null())
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success() && stdout.contains("1 passed"),
                "{stdout}{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let leaked = || {
            let mut fds = [0; 2];
            // SAFETY: pipe fills two new descriptors, which are owned below.
            assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
            // SAFETY: both descriptors were just created by pipe and are owned once.
            let (read, write) =
                unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
            // SAFETY: plain descriptor flag calls on descriptors this test owns.
            unsafe {
                assert_eq!(libc::fcntl(write.as_raw_fd(), libc::F_SETFD, 0), 0);
                assert_eq!(
                    libc::fcntl(read.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC),
                    0
                );
                let flags = libc::fcntl(read.as_raw_fd(), libc::F_GETFL);
                assert_eq!(
                    libc::fcntl(read.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK),
                    0
                );
            }
            (read, write)
        };
        // 0 is end of file (no writer left); -1 with EAGAIN means a child still holds one.
        let read_once = |read: &OwnedFd| {
            let mut byte = 0u8;
            // SAFETY: reads at most one byte into a local buffer from an owned descriptor.
            unsafe { libc::read(read.as_raw_fd(), (&raw mut byte).cast(), 1) }
        };
        let home = tempfile::tempdir().unwrap();
        let context = LaunchContext {
            journal: ProcessJournal::at(home.path()),
            supervisor: "hotsheet-test",
            purpose: Purpose::JobMonitor,
            project_id: None,
            session_id: None,
            binary_revision: None,
        };
        let sleeper = || {
            let mut command = Command::new("sleep");
            command.arg("30");
            command
        };

        let (read, write) = leaked();
        let mut detached = context.spawn_detached(&mut sleeper()).unwrap();
        drop(write);
        assert_eq!(
            read_once(&read),
            0,
            "spawn_detached passed the descriptor on"
        );
        detached.kill().unwrap();
        detached.wait().unwrap();

        let (read, write) = leaked();
        let mut ordinary = context.spawn(&mut sleeper()).unwrap();
        drop(write);
        assert_eq!(read_once(&read), -1, "an ordinary child inherits as before");
        ordinary.kill().unwrap();
        ordinary.wait().unwrap();

        // `detached` takes the same path. Its child is found by its journal record and
        // ended here, so the test leaves nothing running.
        let (read, write) = leaked();
        context.detached(&mut sleeper()).unwrap();
        drop(write);
        let held = read_once(&read);
        let records = context.journal.list().unwrap();
        let running: Vec<u32> = records
            .iter()
            .filter(|record| record.exit_reason.is_none())
            .map(|record| record.pid)
            .collect();
        assert_eq!(running.len(), 1, "{records:?}");
        let pid = running[0];
        // SAFETY: the pid is the detached sleep this test just launched and has not reaped.
        unsafe { libc::kill(i32::try_from(pid).unwrap(), libc::SIGKILL) };
        assert_eq!(held, 0, "detached passed the descriptor on");
    }

    /// HS2-8CGT4R: a launcher whose own output is a pipe (a test's `Command::output()`, a
    /// shell capturing `$(...)`) hands its detached child none of that pipe, so the reader
    /// sees end of file when the launcher exits, not when the detached child does. On
    /// Windows every inheritable handle goes to every child, and the launcher's standard
    /// handles are inheritable pipe ends, so `job watch ci` hung until its monitor exited.
    #[test]
    fn hs2_8cgt4r_a_detached_child_keeps_no_end_of_its_launchers_output_pipe() {
        const ROLE: &str = "HS2_8CGT4R_ROLE";
        const NAME: &str =
            "tests::hs2_8cgt4r_a_detached_child_keeps_no_end_of_its_launchers_output_pipe";
        let me = std::env::current_exe().unwrap();
        let role_command = |role: &str| {
            let mut command = Command::new(&me);
            command
                .args(["--exact", NAME, "--nocapture", "--test-threads=1"])
                .env(ROLE, role);
            command
        };
        match std::env::var(ROLE).as_deref() {
            // The detached child outlives the launcher by far more than the bound below.
            Ok("sleeper") => {
                std::thread::sleep(std::time::Duration::from_secs(20));
                return;
            }
            Ok("launcher") => {
                let home = tempfile::tempdir().unwrap();
                let context = LaunchContext {
                    journal: ProcessJournal::at(home.path()),
                    supervisor: "hotsheet-test",
                    purpose: Purpose::JobMonitor,
                    project_id: None,
                    session_id: None,
                    binary_revision: None,
                };
                context
                    .detached_as(
                        &mut role_command("sleeper"),
                        Detach {
                            process_group: true,
                            session: false,
                        },
                    )
                    .unwrap();
                println!("launched");
                return;
            }
            _ => {}
        }
        let started = std::time::Instant::now();
        let output = role_command("launcher")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        let waited = started.elapsed();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("launched"));
        assert!(
            waited < std::time::Duration::from_secs(10),
            "the launcher's output stayed open for {waited:?}: the detached child holds it"
        );
    }

    /// HS2-Y4VDBK: on macOS a detached launch is a `posix_spawn` with attributes, never a
    /// fork of this process; it still leaves the requested group or session, runs in its
    /// directory with exactly the stated environment, finds a bare program on the child's
    /// own PATH, reports its exit, and journals it.
    #[cfg(target_os = "macos")]
    #[test]
    fn hs2_y4vdbk_detached_launches_spawn_with_attributes_and_never_fork() {
        use std::os::unix::fs::PermissionsExt;
        static MARKS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
        extern "C" fn child_side() {
            let fd = MARKS.load(std::sync::atomic::Ordering::SeqCst);
            // SAFETY: `write` is async-signal-safe; the descriptor stays open for the test.
            unsafe {
                libc::write(fd, b"F".as_ptr().cast(), 1);
            }
        }
        let home = tempfile::tempdir().unwrap();
        let marks = home.path().join("forks");
        let fd = std::os::fd::IntoRawFd::into_raw_fd(
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&marks)
                .unwrap(),
        );
        MARKS.store(fd, std::sync::atomic::Ordering::SeqCst);
        // SAFETY: registers a handler that only writes one byte.
        assert_eq!(
            unsafe { libc::pthread_atfork(None, None, Some(child_side)) },
            0
        );
        let context = LaunchContext {
            journal: ProcessJournal::at(home.path()),
            supervisor: "hotsheet-test",
            purpose: Purpose::BrokerSupervisor,
            project_id: None,
            session_id: None,
            binary_revision: None,
        };
        let sleeper = || {
            let mut command = Command::new("sleep");
            command.arg("30");
            command
        };
        let ours = |call: unsafe extern "C" fn(libc::pid_t) -> libc::pid_t| {
            // SAFETY: plain process-id queries.
            unsafe { call(0) }
        };
        let of = |call: unsafe extern "C" fn(libc::pid_t) -> libc::pid_t, pid: u32| {
            // SAFETY: plain process-id queries of this test's own unreaped child.
            unsafe { call(libc::pid_t::try_from(pid).unwrap()) }
        };
        let pid_t = |pid: u32| libc::pid_t::try_from(pid).unwrap();

        // Default: the launcher's group and session.
        let mut plain = context.spawn_detached(&mut sleeper()).unwrap();
        assert_eq!(of(libc::getpgid, plain.id()), ours(libc::getpgid));
        // Its own process group.
        let group = Detach {
            process_group: true,
            session: false,
        };
        let mut grouped = context.spawn_detached_as(&mut sleeper(), group).unwrap();
        assert_eq!(of(libc::getpgid, grouped.id()), pid_t(grouped.id()));
        assert_eq!(of(libc::getsid, grouped.id()), ours(libc::getsid));
        // Its own session.
        let session = Detach {
            process_group: false,
            session: true,
        };
        let mut leader = context.spawn_detached_as(&mut sleeper(), session).unwrap();
        assert_eq!(of(libc::getsid, leader.id()), pid_t(leader.id()));
        for child in [&mut plain, &mut grouped, &mut leader] {
            assert_eq!(child.try_wait().unwrap(), None);
            child.kill().unwrap();
            assert_eq!(
                std::os::unix::process::ExitStatusExt::signal(&child.wait().unwrap()),
                Some(9)
            );
            // Reaped: a later kill sends nothing.
            child.kill().unwrap();
        }

        // Its directory, exactly the stated environment, and a bare name on that PATH.
        let tools = home.path().join("tools");
        fs::create_dir_all(&tools).unwrap();
        let tool = tools.join("report");
        fs::write(
            &tool,
            "#!/bin/sh\npwd > out\n/usr/bin/env >> out\nexit 17\n",
        )
        .unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
        let mut command = Command::new("report");
        command.current_dir(&tools);
        let path = format!("{}:/usr/bin:/bin", tools.display());
        replace_environment(&mut command, [("PATH", path.as_str()), ("STEP", "y4vdbk")]);
        let mut reported = context.spawn_detached(&mut command).unwrap();
        assert_eq!(reported.wait().unwrap().code(), Some(17));
        let out = fs::read_to_string(tools.join("out")).unwrap();
        let mut lines = out.lines();
        assert_eq!(
            hotsheet_sync::paths::canonicalize(lines.next().unwrap()).unwrap(),
            hotsheet_sync::paths::canonicalize(&tools).unwrap()
        );
        let mut seen: Vec<&str> = lines
            .filter(|line| {
                !line.starts_with("PWD=") && !line.starts_with("SHLVL=") && !line.starts_with("_=")
            })
            .collect();
        seen.sort_unstable();
        assert_eq!(seen, vec![format!("PATH={path}").as_str(), "STEP=y4vdbk"]);
        // A name found nowhere on that PATH is not found, and nothing starts.
        let mut missing = Command::new("no-such-tool-y4vdbk");
        replace_environment(&mut missing, [("PATH", "/usr/bin:/bin")]);
        assert_eq!(
            context
                .spawn_detached(&mut missing)
                .err()
                .map(|error| error.kind()),
            Some(io::ErrorKind::NotFound)
        );

        // Every launch is journaled with its observed end.
        let records = context.journal.list().unwrap();
        assert_eq!(records.len(), 4, "{records:?}");
        assert_eq!(
            records
                .iter()
                .filter(|record| record.exit_reason == Some(ExitReason::Killed))
                .count(),
            3
        );
        assert_eq!(
            records
                .iter()
                .filter(|record| record.exit_reason == Some(ExitReason::Failed))
                .count(),
            1
        );
        // None of it forked this process.
        assert_eq!(
            fs::metadata(&marks).unwrap().len(),
            0,
            "a detached launch forked"
        );
        // The detector works: a pre_exec spawn forks.
        let mut forked = Command::new("/usr/bin/true");
        // SAFETY: an empty hook.
        unsafe {
            std::os::unix::process::CommandExt::pre_exec(&mut forked, || Ok(()));
        }
        assert!(forked.status().unwrap().success());
        assert!(
            fs::metadata(&marks).unwrap().len() > 0,
            "a forked spawn is seen"
        );
    }

    #[cfg(unix)]
    #[test]
    fn hs2_g9tf60_tracked_commands_record_observed_and_detached_lifecycle() {
        let home = tempfile::tempdir().unwrap();
        let context = LaunchContext {
            journal: ProcessJournal::at(home.path()),
            supervisor: "hotsheet-test",
            purpose: Purpose::RepositoryHelper,
            project_id: Some("project-1".into()),
            session_id: None,
            binary_revision: Some("test-revision".into()),
        };
        let mut success = Command::new("sh");
        success.args(["-c", "printf synthetic"]);
        assert_eq!(context.output(&mut success).unwrap().stdout, b"synthetic");
        let mut failure = Command::new("sh");
        failure.args(["-c", "exit 7"]);
        assert_eq!(context.status(&mut failure).unwrap().code(), Some(7));
        let mut detached = Command::new("sh");
        detached.args(["-c", "exit 0"]);
        context.detached(&mut detached).unwrap();
        let mut sleeping = Command::new("sleep");
        sleeping.arg("30");
        let mut killed = context.spawn(&mut sleeping).unwrap();
        killed.kill().unwrap();
        killed.wait().unwrap();

        let records = context.journal.list().unwrap();
        assert_eq!(records.len(), 4);
        for reason in [
            Some(ExitReason::Completed),
            Some(ExitReason::Failed),
            None,
            Some(ExitReason::Killed),
        ] {
            assert_eq!(
                records
                    .iter()
                    .filter(|record| record.exit_reason == reason)
                    .count(),
                1
            );
        }
        assert!(records.iter().all(|record| {
            record.project_id.as_deref() == Some("project-1")
                && record.binary_revision.as_deref() == Some("test-revision")
        }));
        let serialized = serde_json::to_string(&records).unwrap();
        assert!(!serialized.contains("synthetic"));
        assert!(!serialized.contains("exit 7"));
    }

    fn meta(pid: u32) -> LaunchMeta {
        LaunchMeta {
            pid,
            supervisor_pid: 7,
            supervisor: "test-host".into(),
            purpose: Purpose::AiChannel,
            project_id: Some("project-1".into()),
            session_id: Some("session-1".into()),
            binary_revision: Some("rev-1".into()),
        }
    }

    #[test]
    fn hs2_gdcmxe_launch_exit_restart_and_unresolved_investigation() {
        let home = tempfile::tempdir().unwrap();
        let journal = ProcessJournal::at(home.path());
        let first = journal.start(meta(42)).unwrap();
        assert_eq!(journal.list().unwrap(), vec![first.clone()]);
        let ended = journal
            .finish(&first.launch_id, ExitReason::Completed)
            .unwrap();
        assert!(ended.ended_ms.is_some());
        assert_eq!(ended.exit_reason, Some(ExitReason::Completed));
        assert_eq!(
            journal
                .finish(&first.launch_id, ExitReason::Killed)
                .unwrap(),
            ended
        );
        let restarted = journal.start(meta(42)).unwrap();
        assert_ne!(restarted.launch_id, first.launch_id);
        assert_eq!(restarted.pid, first.pid);
        let recovered = ProcessJournal::at(home.path()).list().unwrap();
        assert_eq!(recovered.len(), 2);
        // HS2-ZKP7HW: both launches can start in the same millisecond, and a random
        // same-millisecond ULID orders them either way, so find each launch by its id.
        let by_id = |id: &str| recovered.iter().find(|r| r.launch_id == id).unwrap();
        assert_eq!(by_id(&first.launch_id), &ended);
        let recovered_restart = by_id(&restarted.launch_id);
        assert_eq!(recovered_restart.exit_reason, None);
        assert_eq!(recovered_restart.session_id.as_deref(), Some("session-1"));
    }

    /// HS2-ZKP7HW: launches that start in the same millisecond list in launch-id order,
    /// whatever order the directory returns them in, so `list()` is deterministic.
    #[test]
    fn hs2_zkp7hw_same_millisecond_launches_list_by_launch_id() {
        let home = tempfile::tempdir().unwrap();
        let journal = ProcessJournal::at(home.path());
        journal.prepare_dir().unwrap();
        let started = now_ms();
        let mut ids: Vec<String> = (0..8).map(|_| ulid::Ulid::new().to_string()).collect();
        // Write in descending id order so directory order never happens to match.
        ids.sort_unstable_by(|a, b| b.cmp(a));
        for (pid, launch_id) in ids.iter().enumerate() {
            let record = ProcessRecord {
                launch_id: launch_id.clone(),
                pid: u32::try_from(pid).unwrap(),
                supervisor_pid: 7,
                supervisor: "test-host".into(),
                purpose: Purpose::AiChannel,
                project_id: None,
                session_id: None,
                binary_revision: None,
                started_ms: started,
                ended_ms: None,
                exit_reason: None,
            };
            fs::write(
                journal.path(&record.launch_id),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
        }
        // An earlier launch still lists first, ahead of any same-millisecond tie.
        let earlier = ProcessRecord {
            launch_id: ulid::Ulid::new().to_string(),
            pid: 99,
            supervisor_pid: 7,
            supervisor: "test-host".into(),
            purpose: Purpose::AiChannel,
            project_id: None,
            session_id: None,
            binary_revision: None,
            started_ms: started - 1,
            ended_ms: None,
            exit_reason: None,
        };
        fs::write(
            journal.path(&earlier.launch_id),
            serde_json::to_vec(&earlier).unwrap(),
        )
        .unwrap();
        let listed: Vec<String> = journal
            .list()
            .unwrap()
            .into_iter()
            .map(|r| r.launch_id)
            .collect();
        ids.sort_unstable();
        let mut expected = vec![earlier.launch_id];
        expected.extend(ids);
        assert_eq!(listed, expected);
    }

    #[test]
    fn hs2_gdcmxe_rejects_content_and_corrupt_records() {
        let home = tempfile::tempdir().unwrap();
        let journal = ProcessJournal::at(home.path());
        let mut unsafe_meta = meta(1);
        unsafe_meta.session_id = Some("prompt text with spaces".into());
        assert_eq!(
            journal.start(unsafe_meta).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        let record = journal.start(meta(2)).unwrap();
        fs::write(journal.path(&record.launch_id), b"not-json").unwrap();
        assert_eq!(
            journal.list().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::write(home.path().join("processes/.tmp-interrupted"), b"partial").unwrap();
        let recovered = journal.start(meta(3)).unwrap();
        assert_eq!(journal.list().unwrap(), vec![recovered]);
    }

    /// HS2-F0S155: identifiers are record content, never path components. A path-shaped,
    /// traversal-shaped, or non-ASCII id is refused with no record written; bounded ids,
    /// including `..`, stay readable because a record's file name is only its launch ULID.
    #[test]
    fn hs2_f0s155_identifier_handling_keeps_paths_out_and_records_in_place() {
        let home = tempfile::tempdir().unwrap();
        let journal = ProcessJournal::at(home.path());
        let too_long = "a".repeat(129);
        for rejected in [
            "/Users/someone/store",
            "store/.hotsheet",
            "..\\..\\escape",
            "../../escape",
            "caf\u{e9}",
            "\u{30d7}\u{30ed}\u{30b8}\u{30a7}\u{30af}\u{30c8}",
            "id\u{200b}",
            "has space",
            "",
            too_long.as_str(),
        ] {
            for field in ["project", "session", "supervisor", "revision"] {
                let mut bad = meta(1);
                match field {
                    "project" => bad.project_id = Some(rejected.into()),
                    "session" => bad.session_id = Some(rejected.into()),
                    "supervisor" => bad.supervisor = rejected.into(),
                    _ => bad.binary_revision = Some(rejected.into()),
                }
                assert_eq!(
                    journal.start(bad).unwrap_err().kind(),
                    io::ErrorKind::InvalidInput,
                    "{field} id {rejected:?} must be refused"
                );
            }
        }
        assert_eq!(journal.list().unwrap(), Vec::new());

        // The store project id (16 hex, as the broker records it), a checkout id
        // (`folder-shortpathhash`), the longest id, and dot-only ids are accepted.
        let longest = "a".repeat(128);
        for accepted in [
            "0123456789abcdef",
            "hotsheet2-1a2b3c4d",
            longest.as_str(),
            "..",
            ".",
        ] {
            let mut ok = meta(2);
            ok.project_id = Some(accepted.into());
            ok.session_id = Some(accepted.into());
            let record = journal.start(ok).unwrap();
            assert_eq!(record.project_id.as_deref(), Some(accepted));
        }
        let names: Vec<String> = fs::read_dir(home.path().join("processes"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 5);
        for name in &names {
            let stem = name.strip_suffix(".json").expect("only record files");
            ulid::Ulid::from_string(stem).expect("record file named by its launch ULID");
        }
        assert_eq!(fs::read_dir(home.path()).unwrap().count(), 1);
        assert_eq!(journal.list().unwrap().len(), 5);
    }

    /// HS2-NNA91A: a refusal names the refused field with a fixed string and never echoes
    /// the refused value; any other start error summarizes as its kind only.
    #[test]
    fn hs2_nna91a_refusal_names_the_field_without_the_value() {
        let home = tempfile::tempdir().unwrap();
        let journal = ProcessJournal::at(home.path());
        let refused = "/Users/someone/private store";
        for (field, expected) in [
            ("project", "invalid diagnostic identifier: project_id"),
            ("session", "invalid diagnostic identifier: session_id"),
            ("supervisor", "invalid diagnostic identifier: supervisor"),
            ("revision", "invalid diagnostic identifier: binary_revision"),
        ] {
            let mut bad = meta(1);
            match field {
                "project" => bad.project_id = Some(refused.into()),
                "session" => bad.session_id = Some(refused.into()),
                "supervisor" => bad.supervisor = refused.into(),
                _ => bad.binary_revision = Some(refused.into()),
            }
            let error = journal.start(bad).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert_eq!(start_error_summary(&error), expected);
            assert_eq!(error.to_string(), expected);
            for text in [start_error_summary(&error), format!("{error:?}")] {
                assert!(!text.contains("someone"), "{text} echoes the value");
            }
        }

        let other = io::Error::new(io::ErrorKind::PermissionDenied, "/Users/someone/journal");
        assert_eq!(start_error_summary(&other), "permission denied");
    }

    /// HS2-F0S155: a record the detached broker wrote before the fix still reads back,
    /// and an unended one still finishes; the in-process fix changes no file format.
    #[test]
    fn hs2_f0s155_existing_broker_records_remain_readable() {
        let home = tempfile::tempdir().unwrap();
        let journal = ProcessJournal::at(home.path());
        journal.prepare_dir().unwrap();
        let launch_id = ulid::Ulid::new().to_string();
        let started = now_ms();
        let existing = format!(
            r#"{{"launch_id":"{launch_id}","pid":4242,"supervisor_pid":4241,"supervisor":"terminal-broker","purpose":"terminal_shell","project_id":"0123456789abcdef","session_id":"01M6TERMINAL","binary_revision":null,"started_ms":{started},"ended_ms":null,"exit_reason":null}}"#
        );
        fs::write(journal.path(&launch_id), existing).unwrap();
        let records = journal.list().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].purpose, Purpose::TerminalShell);
        assert_eq!(records[0].project_id.as_deref(), Some("0123456789abcdef"));
        assert_eq!(records[0].exit_reason, None);
        let ended = journal.finish(&launch_id, ExitReason::Killed).unwrap();
        assert_eq!(ended.exit_reason, Some(ExitReason::Killed));
        journal.start(meta(5)).unwrap();
        assert_eq!(journal.list().unwrap().len(), 2);
    }

    #[test]
    fn hs2_gdcmxe_prunes_old_and_excess_records() {
        let home = tempfile::tempdir().unwrap();
        let journal = ProcessJournal::at(home.path());
        journal.prepare_dir().unwrap();
        for pid in 0..(MAX_ENDED as u32 + 3) {
            let record = ProcessRecord {
                launch_id: ulid::Ulid::new().to_string(),
                pid,
                supervisor_pid: 7,
                supervisor: "test-host".into(),
                purpose: Purpose::AiChannel,
                project_id: None,
                session_id: None,
                binary_revision: None,
                started_ms: now_ms() + u64::from(pid),
                ended_ms: Some(now_ms() + u64::from(pid)),
                exit_reason: Some(ExitReason::Completed),
            };
            fs::write(
                journal.path(&record.launch_id),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
        }
        journal.prune().unwrap();
        assert_eq!(journal.list().unwrap().len(), MAX_ENDED);
    }

    #[cfg(unix)]
    #[test]
    fn hs2_gdcmxe_rejects_a_redirected_journal_directory() {
        use std::os::unix::fs::symlink;

        let home = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        symlink(elsewhere.path(), home.path().join("processes")).unwrap();
        let journal = ProcessJournal::at(home.path());
        assert_eq!(
            journal.start(meta(4)).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            journal.list().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
