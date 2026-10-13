//! MCP presence lease (HS2-HNXE38): once an AI tool initializes this shim, a background
//! thread registers a lease with the project's server, renews it every heartbeat, and drops it
//! when the shim's stdin closes. The server needs no goodbye after a crash: an unrenewed lease
//! lapses and a dead pid is noticed. Registration is an idempotent upsert, so a heartbeat after
//! a server restart simply registers again.
//!
//! The server is known in proxy mode (`--server`/`--secret`) or, for a serverless shim started
//! inside a Hot Sheet terminal, from the `HOTSHEET_SERVER`/`HOTSHEET_SECRET` route-back
//! environment. Without either the shim stays silent.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{Value, json};

/// How often the lease is renewed; the server's lease TTL is three heartbeats.
pub const HEARTBEAT: Duration = Duration::from_secs(15);

/// Where and what to register.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceTarget {
    pub base: String,
    pub secret: String,
    pub lease_id: String,
    pub body: Value,
}

impl PresenceTarget {
    /// The target for this shim, or `None` when no server is known. `server`/`secret` are the
    /// proxy-mode flags; `env` reads the process environment.
    pub fn resolve(
        server: Option<&str>,
        secret: Option<&str>,
        env: impl Fn(&str) -> Option<String>,
        pid: u32,
        cwd: Option<String>,
        lease_id: String,
    ) -> Option<Self> {
        let non_empty = |v: Option<String>| v.filter(|v| !v.trim().is_empty());
        let (base, secret) = match (server, secret) {
            (Some(server), Some(secret)) => (server.to_owned(), secret.to_owned()),
            _ => (
                non_empty(env("HOTSHEET_SERVER"))?,
                non_empty(env("HOTSHEET_SECRET"))?,
            ),
        };
        Some(Self {
            base: base.trim_end_matches('/').to_owned(),
            secret,
            body: json!({
                "lease_id": lease_id,
                "pid": pid,
                "tool": non_empty(env("HOTSHEET_AGENT")),
                "cwd": cwd,
                "terminal_id": non_empty(env("HOTSHEET_TERMINAL_ID")),
            }),
            lease_id,
        })
    }
}

/// The two calls the heartbeat makes; real over HTTP, faked in tests.
pub trait PresenceTransport: Send + 'static {
    /// Register or renew; failures are retried on the next heartbeat.
    fn upsert(&self, target: &PresenceTarget) -> bool;
    fn release(&self, target: &PresenceTarget);
}

/// [`PresenceTransport`] over the server's HTTP API.
pub struct HttpPresence {
    agent: ureq::Agent,
}

impl Default for HttpPresence {
    fn default() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(5))
                .build(),
        }
    }
}

impl PresenceTransport for HttpPresence {
    fn upsert(&self, target: &PresenceTarget) -> bool {
        self.agent
            .post(&format!("{}/mcp-presence", target.base))
            .set("X-Hotsheet-Secret", &target.secret)
            .set("Content-Type", "application/json")
            .send_string(&target.body.to_string())
            .is_ok()
    }

    fn release(&self, target: &PresenceTarget) {
        let _ = self
            .agent
            .delete(&format!("{}/mcp-presence/{}", target.base, target.lease_id))
            .set("X-Hotsheet-Secret", &target.secret)
            .call();
    }
}

/// A running heartbeat; [`Presence::stop`] (or drop) releases the lease.
pub struct Presence {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Presence {
    /// Register now and renew every `interval` until stopped.
    pub fn start(
        target: PresenceTarget,
        transport: impl PresenceTransport,
        interval: Duration,
    ) -> Self {
        let (stop, stopped) = mpsc::channel::<()>();
        let thread = std::thread::spawn(move || {
            loop {
                transport.upsert(&target);
                match stopped.recv_timeout(interval) {
                    Err(RecvTimeoutError::Timeout) => {}
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            transport.release(&target);
        });
        Self {
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    /// Release the lease and wait for the heartbeat thread to finish.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Whether `msg` is the MCP `initialize` request that starts presence.
pub fn is_initialize(msg: &Value) -> bool {
    msg.get("method").and_then(Value::as_str) == Some("initialize")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<String>>>);

    impl PresenceTransport for Recorder {
        fn upsert(&self, target: &PresenceTarget) -> bool {
            self.0
                .lock()
                .unwrap()
                .push(format!("upsert {}", target.lease_id));
            true
        }
        fn release(&self, target: &PresenceTarget) {
            self.0
                .lock()
                .unwrap()
                .push(format!("release {}", target.lease_id));
        }
    }

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn resolve_prefers_proxy_flags_then_terminal_env_else_none() {
        let terminal = env(&[
            ("HOTSHEET_SERVER", "http://127.0.0.1:4175/"),
            ("HOTSHEET_SECRET", "s"),
            ("HOTSHEET_TERMINAL_ID", "t1"),
            ("HOTSHEET_AGENT", "claude"),
        ]);
        let from_env =
            PresenceTarget::resolve(None, None, &terminal, 7, Some("/p".into()), "L".into())
                .unwrap();
        assert_eq!(from_env.base, "http://127.0.0.1:4175");
        assert_eq!(from_env.body["terminal_id"], "t1");
        assert_eq!(from_env.body["tool"], "claude");
        assert_eq!(from_env.body["pid"], 7);
        assert_eq!(from_env.body["lease_id"], "L");

        let proxy =
            PresenceTarget::resolve(Some("http://h"), Some("x"), &terminal, 7, None, "L".into())
                .unwrap();
        assert_eq!(
            (proxy.base.as_str(), proxy.secret.as_str()),
            ("http://h", "x")
        );

        assert!(PresenceTarget::resolve(None, None, env(&[]), 7, None, "L".into()).is_none());
        let blank = env(&[("HOTSHEET_SERVER", " "), ("HOTSHEET_SECRET", "s")]);
        assert!(PresenceTarget::resolve(None, None, blank, 7, None, "L".into()).is_none());
    }

    #[test]
    fn heartbeat_registers_renews_and_releases_once_on_stop() {
        let target =
            PresenceTarget::resolve(Some("http://h"), Some("s"), env(&[]), 1, None, "L".into())
                .unwrap();
        let calls = Recorder::default();
        let presence = Presence::start(target, calls.clone(), Duration::from_millis(10));
        while calls.0.lock().unwrap().len() < 3 {
            std::thread::sleep(Duration::from_millis(5));
        }
        presence.stop();
        let calls = calls.0.lock().unwrap().clone();
        assert!(
            calls[..calls.len() - 1].iter().all(|c| c == "upsert L"),
            "{calls:?}"
        );
        assert_eq!(calls.last().unwrap(), "release L");
    }

    #[test]
    fn dropping_the_handle_releases_the_lease() {
        let target =
            PresenceTarget::resolve(Some("http://h"), Some("s"), env(&[]), 1, None, "L".into())
                .unwrap();
        let calls = Recorder::default();
        drop(Presence::start(
            target,
            calls.clone(),
            Duration::from_secs(60),
        ));
        assert_eq!(
            calls.0.lock().unwrap().clone(),
            vec!["upsert L".to_owned(), "release L".to_owned()]
        );
    }

    #[test]
    fn only_initialize_starts_presence() {
        assert!(is_initialize(&json!({"method": "initialize", "id": 1})));
        assert!(!is_initialize(
            &json!({"method": "notifications/initialized"})
        ));
        assert!(!is_initialize(&json!({"id": 1})));
    }
}
