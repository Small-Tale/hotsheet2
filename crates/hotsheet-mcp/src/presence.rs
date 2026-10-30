//! MCP presence lease (HS2-HNXE38): once an AI tool initializes this shim, a background
//! thread registers a lease with the project's server, renews it every heartbeat, and drops it
//! when the shim's stdin closes. The server needs no goodbye after a crash: an unrenewed lease
//! lapses and a dead pid is noticed. Registration is an idempotent upsert, so a heartbeat after
//! a server restart simply registers again.
//!
//! The server is known in proxy mode (`--server`/`--secret`) or, for a serverless shim started
//! inside a Hot Sheet terminal, from the `HOTSHEET_SERVER`/`HOTSHEET_SECRET` route-back
//! environment. Otherwise a serverless shim (`--path`) looks the store up in the machine-local
//! instance registry (`${HOTSHEET_HOME}/instances`, HS2-E4J72D) and reports to the live server
//! recorded there. Without any of these the shim stays silent. A failed heartbeat
//! re-consults the registry, so a stale terminal route-back or a server restarted on a new
//! port moves the lease to the live server.

use std::path::Path;
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
    /// proxy-mode flags; `env` reads the process environment; `discover` is consulted last,
    /// for a serverless shim outside a Hot Sheet terminal (see [`discover_server`]).
    pub fn resolve(
        server: Option<&str>,
        secret: Option<&str>,
        env: impl Fn(&str) -> Option<String>,
        discover: impl FnOnce() -> Option<(String, String)>,
        pid: u32,
        cwd: Option<String>,
        lease_id: String,
    ) -> Option<Self> {
        let non_empty = |v: Option<String>| v.filter(|v| !v.trim().is_empty());
        let (base, secret) = match (server, secret) {
            (Some(server), Some(secret)) => (server.to_owned(), secret.to_owned()),
            _ => match (
                non_empty(env("HOTSHEET_SERVER")),
                non_empty(env("HOTSHEET_SECRET")),
            ) {
                (Some(server), Some(secret)) => (server, secret),
                _ => discover()?,
            },
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

/// The `(url, secret)` of the server the machine-local instance registry records for
/// `store` (HS2-E4J72D). Every `*.json` record under `instances_dir` is read; one whose
/// `store_path` names the same canonical directory matches, and the most recently started
/// match wins. Unreadable or foreign records are skipped, so a missing registry or a store
/// no server hosts simply yields `None`. A stale record from a crashed server is harmless:
/// its upserts fail and are retried silently, exactly like a server that went away.
pub fn discover_server(instances_dir: &Path, store: &Path) -> Option<(String, String)> {
    let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let wanted = canonical(store);
    let mut best: Option<(String, String, String)> = None;
    for entry in std::fs::read_dir(instances_dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Some(record) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        else {
            continue;
        };
        let field = |key: &str| {
            record
                .get(key)
                .and_then(Value::as_str)
                .filter(|v| !v.trim().is_empty())
                .map(str::to_owned)
        };
        let (Some(url), Some(secret), Some(store_path)) =
            (field("url"), field("secret"), field("store_path"))
        else {
            continue;
        };
        if canonical(Path::new(&store_path)) != wanted {
            continue;
        }
        let started = field("started_at").unwrap_or_default();
        if best.as_ref().is_none_or(|(_, _, seen)| started > *seen) {
            best = Some((url, secret, started));
        }
    }
    best.map(|(url, secret, _)| (url, secret))
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
                .redirects(0)
                .timeout(Duration::from_secs(5))
                .build(),
        }
    }
}

// HS2-081PXN: classify headers only. A no-follow 3xx is still Ok in ureq, but
// cannot establish a lease or successful cleanup and its body has no authority.
fn request_succeeded(result: Result<ureq::Response, ureq::Error>) -> bool {
    result.is_ok_and(|response| !(300..400).contains(&response.status()))
}

impl PresenceTransport for HttpPresence {
    fn upsert(&self, target: &PresenceTarget) -> bool {
        request_succeeded(
            self.agent
                .post(&format!("{}/mcp-presence", target.base))
                .set("X-Hotsheet-Secret", &target.secret)
                .set("Content-Type", "application/json")
                .send_string(&target.body.to_string()),
        )
    }

    fn release(&self, target: &PresenceTarget) {
        let _ = request_succeeded(
            self.agent
                .delete(&format!("{}/mcp-presence/{}", target.base, target.lease_id))
                .set("X-Hotsheet-Secret", &target.secret)
                .call(),
        );
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
        Self::start_relocating(target, transport, interval, || None)
    }

    /// Like [`Presence::start`], but after a failed upsert `relocate` names the server to
    /// report to now (HS2-HNXE38 reopen). A Hot Sheet terminal outlives a server restart, so
    /// its `HOTSHEET_SERVER` route-back can name a dead port while the instance registry
    /// records the live one; a restarted server also moves to a new port. When `relocate`
    /// yields a different endpoint the lease moves there and registers at once.
    pub fn start_relocating(
        mut target: PresenceTarget,
        transport: impl PresenceTransport,
        interval: Duration,
        mut relocate: impl FnMut() -> Option<(String, String)> + Send + 'static,
    ) -> Self {
        let (stop, stopped) = mpsc::channel::<()>();
        let thread = std::thread::spawn(move || {
            loop {
                if !transport.upsert(&target)
                    && let Some((base, secret)) = relocate()
                {
                    let base = base.trim_end_matches('/').to_owned();
                    if base != target.base || secret != target.secret {
                        target.base = base;
                        target.secret = secret;
                        transport.upsert(&target);
                    }
                }
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
        let from_env = PresenceTarget::resolve(
            None,
            None,
            &terminal,
            || None,
            7,
            Some("/p".into()),
            "L".into(),
        )
        .unwrap();
        assert_eq!(from_env.base, "http://127.0.0.1:4175");
        assert_eq!(from_env.body["terminal_id"], "t1");
        assert_eq!(from_env.body["tool"], "claude");
        assert_eq!(from_env.body["pid"], 7);
        assert_eq!(from_env.body["lease_id"], "L");

        let proxy = PresenceTarget::resolve(
            Some("http://h"),
            Some("x"),
            &terminal,
            || None,
            7,
            None,
            "L".into(),
        )
        .unwrap();
        assert_eq!(
            (proxy.base.as_str(), proxy.secret.as_str()),
            ("http://h", "x")
        );

        assert!(
            PresenceTarget::resolve(None, None, env(&[]), || None, 7, None, "L".into()).is_none()
        );
        let blank = env(&[("HOTSHEET_SERVER", " "), ("HOTSHEET_SECRET", "s")]);
        assert!(PresenceTarget::resolve(None, None, blank, || None, 7, None, "L".into()).is_none());
    }

    #[test]
    fn registry_discovery_is_the_last_resort_and_matches_the_canonical_store() {
        let home = tempfile::tempdir().unwrap();
        let instances = home.path().join("instances");
        std::fs::create_dir(&instances).unwrap();
        let store = home.path().join("store");
        let other = home.path().join("other");
        std::fs::create_dir(&store).unwrap();
        std::fs::create_dir(&other).unwrap();
        let record = |file: &str, url: &str, secret: &str, store: &Path, started: &str| {
            std::fs::write(
                instances.join(file),
                json!({"pid": 1, "url": url, "secret": secret,
                       "store_path": store.display().to_string(),
                       "index_path": "", "started_at": started})
                .to_string(),
            )
            .unwrap();
        };
        // No registry directory at all, then an empty one: silent.
        assert_eq!(discover_server(&home.path().join("missing"), &store), None);
        assert_eq!(discover_server(&instances, &store), None);
        record("a.json", "http://old", "s1", &store, "2026-01-01T00:00:00Z");
        record(
            "b.json",
            "http://new",
            "s2",
            &store.join("."),
            "2026-02-01T00:00:00Z",
        );
        record(
            "c.json",
            "http://other",
            "s3",
            &other,
            "2026-03-01T00:00:00Z",
        );
        std::fs::write(instances.join("broken.json"), "{").unwrap();
        std::fs::write(instances.join("a.lock"), "1").unwrap();
        // The newest record for the same canonical store wins; other stores are ignored.
        assert_eq!(
            discover_server(&instances, &store),
            Some(("http://new".into(), "s2".into()))
        );

        // Terminal env still wins over discovery, and discovery fills in when env is absent.
        let terminal = env(&[("HOTSHEET_SERVER", "http://t"), ("HOTSHEET_SECRET", "e")]);
        let found = || discover_server(&instances, &store);
        let from_env = PresenceTarget::resolve(None, None, terminal, found, 1, None, "L".into());
        assert_eq!(from_env.unwrap().base, "http://t");
        let discovered =
            PresenceTarget::resolve(None, None, env(&[]), found, 1, None, "L".into()).unwrap();
        assert_eq!(
            (discovered.base.as_str(), discovered.secret.as_str()),
            ("http://new", "s2")
        );
        assert!(discovered.body["terminal_id"].is_null());
    }

    #[test]
    fn heartbeat_registers_renews_and_releases_once_on_stop() {
        let target = PresenceTarget::resolve(
            Some("http://h"),
            Some("s"),
            env(&[]),
            || None,
            1,
            None,
            "L".into(),
        )
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
        let target = PresenceTarget::resolve(
            Some("http://h"),
            Some("s"),
            env(&[]),
            || None,
            1,
            None,
            "L".into(),
        )
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

    /// Records `upsert <base>` / `release <base>`; upserts to bases in `dead` fail.
    #[derive(Clone)]
    struct Endpoints {
        calls: Arc<Mutex<Vec<String>>>,
        dead: Arc<Mutex<Vec<String>>>,
    }

    impl Endpoints {
        fn new(dead: &[&str]) -> Self {
            Self {
                calls: Arc::default(),
                dead: Arc::new(Mutex::new(dead.iter().map(|d| (*d).to_owned()).collect())),
            }
        }
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl PresenceTransport for Endpoints {
        fn upsert(&self, target: &PresenceTarget) -> bool {
            self.calls
                .lock()
                .unwrap()
                .push(format!("upsert {} {}", target.base, target.secret));
            !self.dead.lock().unwrap().contains(&target.base)
        }
        fn release(&self, target: &PresenceTarget) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("release {}", target.base));
        }
    }

    fn proxy_target(base: &str) -> PresenceTarget {
        PresenceTarget::resolve(
            Some(base),
            Some("old"),
            env(&[]),
            || None,
            1,
            None,
            "L".into(),
        )
        .unwrap()
    }

    #[test]
    fn a_dead_route_back_relocates_to_the_registry_server_at_once() {
        // The terminal env names a server that restarted; the registry names the live one.
        let wire = Endpoints::new(&["http://dead"]);
        drop(Presence::start_relocating(
            proxy_target("http://dead"),
            wire.clone(),
            Duration::from_secs(60),
            || Some(("http://live/".to_owned(), "new".to_owned())),
        ));
        assert_eq!(
            wire.calls(),
            vec![
                "upsert http://dead old".to_owned(),
                "upsert http://live new".to_owned(),
                "release http://live".to_owned(),
            ]
        );
    }

    #[test]
    fn relocation_is_skipped_when_healthy_unknown_or_unchanged() {
        // Healthy: relocate is never consulted.
        let wire = Endpoints::new(&[]);
        drop(Presence::start_relocating(
            proxy_target("http://ok"),
            wire.clone(),
            Duration::from_secs(60),
            || panic!("a healthy heartbeat must not relocate"),
        ));
        assert_eq!(
            wire.calls(),
            vec!["upsert http://ok old", "release http://ok"]
        );
        // Down with no registry record, or the registry names the same endpoint: retry later
        // without a duplicate upsert.
        for relocated in [None, Some(("http://down/".to_owned(), "old".to_owned()))] {
            let wire = Endpoints::new(&["http://down"]);
            drop(Presence::start_relocating(
                proxy_target("http://down"),
                wire.clone(),
                Duration::from_secs(60),
                move || relocated.clone(),
            ));
            assert_eq!(
                wire.calls(),
                vec!["upsert http://down old", "release http://down"]
            );
        }
    }

    #[test]
    fn a_server_restart_on_a_new_port_moves_a_running_lease() {
        // Heartbeats succeed, the server dies and restarts elsewhere, the lease follows it
        // and stays there; the release goes to the server that holds the lease.
        let wire = Endpoints::new(&[]);
        let registry = Arc::new(Mutex::new(("http://a".to_owned(), "s".to_owned())));
        let presence = Presence::start_relocating(
            PresenceTarget::resolve(
                Some("http://a"),
                Some("s"),
                env(&[]),
                || None,
                1,
                None,
                "L".into(),
            )
            .unwrap(),
            wire.clone(),
            Duration::from_millis(5),
            {
                let registry = Arc::clone(&registry);
                move || Some(registry.lock().unwrap().clone())
            },
        );
        while wire.calls().len() < 2 {
            std::thread::sleep(Duration::from_millis(2));
        }
        *registry.lock().unwrap() = ("http://b".to_owned(), "t".to_owned());
        wire.dead.lock().unwrap().push("http://a".to_owned());
        while !wire
            .calls()
            .iter()
            .filter(|c| *c == "upsert http://b t")
            .nth(1)
            .is_some()
        {
            std::thread::sleep(Duration::from_millis(2));
        }
        presence.stop();
        let calls = wire.calls();
        let moved = calls.iter().position(|c| c == "upsert http://b t").unwrap();
        assert!(
            calls[..moved]
                .iter()
                .all(|c| c.starts_with("upsert http://a")),
            "{calls:?}"
        );
        assert!(
            calls[moved..calls.len() - 1]
                .iter()
                .all(|c| c == "upsert http://b t"),
            "{calls:?}"
        );
        assert_eq!(calls.last().unwrap(), "release http://b");
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

#[cfg(test)]
#[path = "presence/redirect_tests.rs"]
mod redirect_tests;
