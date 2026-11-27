//! A faithful fake of the Upstash Redis REST service for HS2-K9J1RK tests: a real HTTP
//! server on loopback that speaks the documented shapes (`POST /` with a JSON command
//! array, `POST /pipeline`, `POST /subscribe/<channel>` as server-sent events), checks the
//! bearer token (401), answers a refused command with 400 `{"error"}`, and can rate limit
//! (429 with `Retry-After`), drop or fail streams.
//!
//! The fake has no Lua interpreter. It recognises each script by the `-- hs2rc:<name>:v1`
//! header, refuses any script whose text differs from the library's constant, and applies
//! the contract that script documents on a small in-memory Redis (hashes, strings, sets, a
//! clock, pub/sub). The scripts themselves are exercised only against a real database by the
//! live test in `tests/upstash_live.rs`.
#![allow(
    dead_code,
    reason = "HS2-K9J1RK: each test binary uses a different subset of the fake"
)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hotsheet_resource_claims::upstash::scripts;
use serde_json::{Value, json};

pub const TOKEN: &str = "synthetic-rest-token";

#[derive(Debug, Clone)]
enum Entry {
    Hash(BTreeMap<String, String>),
    Text(String),
    Set(BTreeSet<String>),
}

#[derive(Default)]
struct Store {
    data: HashMap<String, (Entry, Option<u64>)>,
    subscribers: Vec<(String, Sender<String>)>,
}

/// A hook the fake runs once, with itself.
pub type Hook = Box<dyn FnOnce(&Fake) + Send>;

#[derive(Default)]
pub struct Faults {
    /// Answer the next N requests with 429 and `Retry-After: 0`.
    pub rate_limit_next: AtomicU32,
    /// Answer the next N subscribe requests with HTTP 500.
    pub fail_subscribe: AtomicU32,
    /// Close the next N streams right after confirming them.
    pub close_after_confirm: AtomicU32,
    /// Added to the Redis clock, to move time without sleeping.
    pub clock_offset_ms: AtomicU64,
    /// Run once, after a subscriber registers and before it is confirmed.
    pub on_subscribe: Mutex<Option<Hook>>,
}

pub struct Fake {
    store: Mutex<Store>,
    pub faults: Faults,
    /// What each request was, in order: `command <NAME>`, `eval <script>`, `pipeline N`,
    /// `subscribe <channel>`.
    pub log: Mutex<Vec<String>>,
    /// Every EVAL's KEYS and ARGV, for what crossed the wire.
    pub evals: Mutex<Vec<Value>>,
    /// HS2-VDF7VS: the channel of each stream the client closed, in order. A stream the
    /// fake ends itself is not listed.
    pub client_closed: Mutex<Vec<String>>,
    pub url: String,
    stop: AtomicBool,
}

impl Fake {
    pub fn start() -> Arc<Fake> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let fake = Arc::new(Fake {
            store: Mutex::default(),
            faults: Faults::default(),
            log: Mutex::default(),
            evals: Mutex::default(),
            client_closed: Mutex::default(),
            url: format!("http://127.0.0.1:{port}"),
            stop: AtomicBool::new(false),
        });
        let server = Arc::clone(&fake);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if server.stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let handler = Arc::clone(&server);
                std::thread::spawn(move || handler.serve(stream));
            }
        });
        fake
    }

    /// Stop answering: new connections are accepted and dropped.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.url.trim_start_matches("http://"));
    }

    pub fn now_ms(&self) -> u64 {
        u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap()
            + self.faults.clock_offset_ms.load(Ordering::SeqCst)
    }

    pub fn advance_clock(&self, by: Duration) {
        self.faults
            .clock_offset_ms
            .fetch_add(u64::try_from(by.as_millis()).unwrap(), Ordering::SeqCst);
    }

    /// Streams the client has closed so far (HS2-VDF7VS).
    pub fn closed_by_client(&self) -> usize {
        self.client_closed.lock().unwrap().len()
    }

    pub fn requests(&self) -> usize {
        self.log.lock().unwrap().len()
    }

    pub fn count(&self, prefix: &str) -> usize {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.starts_with(prefix))
            .count()
    }

    /// Delete a key without publishing: the lost message a subscriber must not depend on.
    pub fn delete_silently(&self, key: &str) {
        self.store.lock().unwrap().data.remove(key);
    }

    pub fn keys(&self) -> Vec<String> {
        let now = self.now_ms();
        let store = self.store.lock().unwrap();
        let mut keys: Vec<String> = store
            .data
            .iter()
            .filter(|(_, (_, expires))| expires.is_none_or(|e| e > now))
            .map(|(k, _)| k.clone())
            .collect();
        keys.sort();
        keys
    }

    // ---- HTTP ----------------------------------------------------------------------

    fn serve(self: Arc<Self>, stream: TcpStream) {
        if self.stop.load(Ordering::SeqCst) {
            return;
        }
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
            return;
        }
        let path = request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or("/")
            .to_owned();
        let mut headers = HashMap::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap_or(0);
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                headers.insert(k.to_ascii_lowercase(), v.trim().to_owned());
            }
        }
        let length: usize = headers
            .get("content-length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body).unwrap_or(());
        let mut out = stream;

        let authorized = headers
            .get("authorization")
            .is_some_and(|v| v == &format!("Bearer {TOKEN}"));
        if !authorized {
            respond(&mut out, 401, &json!({ "error": "Unauthorized" }), &[]);
            return;
        }
        if take(&self.faults.rate_limit_next) {
            respond(
                &mut out,
                429,
                &json!({ "error": "rate limited" }),
                &[("Retry-After", "0")],
            );
            return;
        }
        if let Some(channel) = path.strip_prefix("/subscribe/") {
            self.subscribe(out, channel);
            return;
        }
        let parsed: Result<Value, _> = serde_json::from_slice(&body);
        let Ok(parsed) = parsed else {
            respond(&mut out, 400, &json!({ "error": "ERR invalid JSON" }), &[]);
            return;
        };
        match path.as_str() {
            "/" => match self.exec(&parsed) {
                Ok(result) => respond(&mut out, 200, &json!({ "result": result }), &[]),
                Err(error) => respond(&mut out, 400, &json!({ "error": error }), &[]),
            },
            "/pipeline" => {
                let Some(commands) = parsed.as_array() else {
                    respond(
                        &mut out,
                        400,
                        &json!({ "error": "ERR pipeline needs an array" }),
                        &[],
                    );
                    return;
                };
                self.log
                    .lock()
                    .unwrap()
                    .push(format!("pipeline {}", commands.len()));
                let replies: Vec<Value> = commands
                    .iter()
                    .map(|c| match self.exec_quiet(c) {
                        Ok(result) => json!({ "result": result }),
                        Err(error) => json!({ "error": error }),
                    })
                    .collect();
                respond(&mut out, 200, &Value::Array(replies), &[]);
            }
            _ => respond(&mut out, 400, &json!({ "error": "ERR unknown route" }), &[]),
        }
    }

    fn subscribe(self: &Arc<Self>, mut out: TcpStream, channel_name: &str) {
        self.log
            .lock()
            .unwrap()
            .push(format!("subscribe {channel_name}"));
        if take(&self.faults.fail_subscribe) {
            respond(&mut out, 500, &json!({ "error": "boom" }), &[]);
            return;
        }
        let (tx, rx) = channel();
        self.store
            .lock()
            .unwrap()
            .subscribers
            .push((channel_name.to_owned(), tx));
        let hook = self.faults.on_subscribe.lock().unwrap().take();
        if let Some(hook) = hook {
            hook(self);
        }
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
        if out.write_all(head.as_bytes()).is_err()
            || out
                .write_all(format!("data: subscribe,{channel_name},1\n\n").as_bytes())
                .is_err()
        {
            return;
        }
        let _ = out.flush();
        // HS2-VDF7VS: watch for the client closing the stream while the fake waits for a
        // message to send. A stream the fake ends itself is marked first, so only the
        // client's closes are recorded.
        let ended_here = Arc::new(AtomicBool::new(false));
        if let Ok(mut probe) = out.try_clone() {
            let (fake, channel_name, ended_here) = (
                Arc::clone(self),
                channel_name.to_owned(),
                ended_here.clone(),
            );
            std::thread::spawn(move || {
                let mut buffer = [0u8; 256];
                while matches!(probe.read(&mut buffer), Ok(read) if read > 0) {}
                if !ended_here.load(Ordering::SeqCst) {
                    fake.client_closed.lock().unwrap().push(channel_name);
                }
            });
        }
        if !take(&self.faults.close_after_confirm) {
            while let Ok(payload) = rx.recv() {
                let line = format!("data: message,{channel_name},{payload}\n\n");
                if out.write_all(line.as_bytes()).is_err() || out.flush().is_err() {
                    break;
                }
            }
        }
        // The probe holds a clone of the socket, so close it explicitly rather than by drop.
        ended_here.store(true, Ordering::SeqCst);
        let _ = out.shutdown(Shutdown::Both);
    }

    // ---- commands ------------------------------------------------------------------

    fn exec(&self, command: &Value) -> Result<Value, String> {
        let name = command
            .get(0)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_uppercase();
        self.log.lock().unwrap().push(if name == "EVAL" {
            let script = command.get(1).and_then(Value::as_str).unwrap_or_default();
            format!("eval {}", script_name(script).unwrap_or("?"))
        } else {
            format!("command {name}")
        });
        self.exec_quiet(command)
    }

    fn exec_quiet(&self, command: &Value) -> Result<Value, String> {
        let parts = command.as_array().ok_or("ERR a command is an array")?;
        let name = parts
            .first()
            .and_then(Value::as_str)
            .ok_or("ERR missing command")?
            .to_ascii_uppercase();
        let text = |i: usize| -> Result<String, String> {
            match parts.get(i) {
                Some(Value::String(s)) => Ok(s.clone()),
                Some(Value::Number(n)) => Ok(n.to_string()),
                _ => Err(format!("ERR wrong number of arguments for '{name}'")),
            }
        };
        match name.as_str() {
            "PING" => Ok(json!("PONG")),
            "SMEMBERS" => {
                let key = text(1)?;
                let now = self.now_ms();
                let store = self.store.lock().unwrap();
                Ok(match store.data.get(&key) {
                    Some((Entry::Set(members), expires)) if expires.is_none_or(|e| e > now) => {
                        json!(members.iter().collect::<Vec<_>>())
                    }
                    _ => json!([]),
                })
            }
            "EVAL" => {
                let script = text(1)?;
                let numkeys: usize = text(2)?.parse().map_err(|_| "ERR bad numkeys")?;
                let rest: Vec<String> = (3..parts.len()).map(&text).collect::<Result<_, _>>()?;
                if rest.len() < numkeys {
                    return Err("ERR Number of keys can't be greater than number of args".into());
                }
                let (keys, argv) = rest.split_at(numkeys);
                self.evals
                    .lock()
                    .unwrap()
                    .push(json!({ "keys": keys, "argv": argv }));
                self.run_script(&script, keys, argv)
            }
            other => Err(format!("ERR unknown command '{other}'")),
        }
    }

    fn run_script(&self, script: &str, keys: &[String], argv: &[String]) -> Result<Value, String> {
        let name = script_name(script).ok_or("ERR unknown script")?;
        let expected = match name {
            "acquire" => scripts::ACQUIRE,
            "renew" => scripts::RENEW,
            "release" => scripts::RELEASE,
            "get" => scripts::GET,
            "verify" => scripts::VERIFY,
            _ => return Err("ERR unknown script".into()),
        };
        if script != expected {
            return Err("ERR the script text differs from the library's".into());
        }
        let now = self.now_ms();
        let mut store = self.store.lock().unwrap();
        // Lapsed keys vanish, as a PEXPIRE'd key does.
        store
            .data
            .retain(|_, (_, expires)| expires.is_none_or(|e| e > now));
        let arg = |i: usize| {
            argv.get(i)
                .cloned()
                .ok_or_else(|| "ERR missing ARGV".to_owned())
        };
        let reply: Vec<String> = match name {
            "acquire" => {
                let claim = hash(&store, &keys[0]);
                if let Some(c) = claim.filter(|c| num(c, "expires") > now) {
                    vec![
                        "held".into(),
                        now.to_string(),
                        c["claim_id"].clone(),
                        c["fencing"].clone(),
                        c["acquired"].clone(),
                        c["expires"].clone(),
                        c["metadata"].clone(),
                    ]
                } else {
                    let fence = incr(&mut store, &keys[1]);
                    let duration: u64 = arg(2)?.parse().map_err(|_| "ERR bad duration")?;
                    let grace: u64 = arg(6)?.parse().map_err(|_| "ERR bad grace")?;
                    let mut fields = BTreeMap::new();
                    fields.insert("claim_id".to_owned(), arg(0)?);
                    fields.insert("digest".to_owned(), arg(1)?);
                    fields.insert("fencing".to_owned(), fence.to_string());
                    fields.insert("acquired".to_owned(), now.to_string());
                    fields.insert("expires".to_owned(), (now + duration).to_string());
                    fields.insert("metadata".to_owned(), arg(3)?);
                    store.data.insert(
                        keys[0].clone(),
                        (Entry::Hash(fields), Some(now + duration + grace)),
                    );
                    add_member(&mut store, &keys[2], &arg(4)?);
                    add_member(&mut store, &keys[3], &arg(5)?);
                    vec![
                        "ok".into(),
                        now.to_string(),
                        fence.to_string(),
                        now.to_string(),
                        (now + duration).to_string(),
                    ]
                }
            }
            "renew" | "release" => {
                let Some(mut c) = hash(&store, &keys[0]) else {
                    return Ok(json!(["token_invalid"]));
                };
                if c["claim_id"] != arg(0)? {
                    return Ok(json!(["not_holder"]));
                }
                if c["digest"] != arg(1)? {
                    return Ok(json!(["token_invalid"]));
                }
                let lapsed = num(&c, "expires") <= now;
                if name == "release" {
                    store.data.remove(&keys[0]);
                    publish(&mut store, &keys[1], "released");
                    if lapsed {
                        return Ok(json!(["expired"]));
                    }
                    vec![
                        "ok".into(),
                        now.to_string(),
                        c["claim_id"].clone(),
                        c["fencing"].clone(),
                        c["acquired"].clone(),
                        c["expires"].clone(),
                        c["metadata"].clone(),
                    ]
                } else {
                    if lapsed {
                        return Ok(json!(["expired"]));
                    }
                    let duration: u64 = arg(2)?.parse().map_err(|_| "ERR bad duration")?;
                    let grace: u64 = arg(3)?.parse().map_err(|_| "ERR bad grace")?;
                    let expires = now + duration;
                    let shortened = expires < num(&c, "expires");
                    c.insert("expires".into(), expires.to_string());
                    store.data.insert(
                        keys[0].clone(),
                        (Entry::Hash(c.clone()), Some(expires + grace)),
                    );
                    if shortened {
                        publish(&mut store, &keys[1], "shortened");
                    }
                    vec![
                        "ok".into(),
                        now.to_string(),
                        c["claim_id"].clone(),
                        c["fencing"].clone(),
                        c["acquired"].clone(),
                        expires.to_string(),
                        c["metadata"].clone(),
                    ]
                }
            }
            "get" => match hash(&store, &keys[0]).filter(|c| num(c, "expires") > now) {
                Some(c) => vec![
                    "held".into(),
                    now.to_string(),
                    c["claim_id"].clone(),
                    c["fencing"].clone(),
                    c["acquired"].clone(),
                    c["expires"].clone(),
                    c["metadata"].clone(),
                ],
                None => vec!["free".into(), now.to_string()],
            },
            "verify" => {
                let token: u64 = arg(0)?.parse().map_err(|_| "ERR bad token")?;
                let current = match store.data.get(&keys[1]) {
                    Some((Entry::Text(t), _)) => t.parse().unwrap_or(0),
                    _ => 0u64,
                };
                if current > token {
                    vec!["not_holder".into()]
                } else if current < token {
                    vec!["invalid".into()]
                } else if hash(&store, &keys[0])
                    .is_some_and(|c| num(&c, "fencing") == current && num(&c, "expires") > now)
                {
                    vec!["ok".into()]
                } else {
                    vec!["expired".into()]
                }
            }
            _ => unreachable!(),
        };
        Ok(json!(reply))
    }
}

fn script_name(script: &str) -> Option<&str> {
    let header = script.lines().next()?.strip_prefix("-- hs2rc:")?;
    header.strip_suffix(":v1")
}

fn hash(store: &Store, key: &str) -> Option<BTreeMap<String, String>> {
    match store.data.get(key) {
        Some((Entry::Hash(h), _)) => Some(h.clone()),
        _ => None,
    }
}

fn num(map: &BTreeMap<String, String>, key: &str) -> u64 {
    map.get(key).and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn incr(store: &mut Store, key: &str) -> u64 {
    let next = match store.data.get(key) {
        Some((Entry::Text(t), _)) => t.parse::<u64>().unwrap_or(0) + 1,
        _ => 1,
    };
    // The fencing counter never expires.
    store
        .data
        .insert(key.to_owned(), (Entry::Text(next.to_string()), None));
    next
}

fn add_member(store: &mut Store, key: &str, member: &str) {
    let entry = store
        .data
        .entry(key.to_owned())
        .or_insert_with(|| (Entry::Set(BTreeSet::new()), None));
    if let Entry::Set(set) = &mut entry.0 {
        set.insert(member.to_owned());
    }
}

fn publish(store: &mut Store, channel: &str, payload: &str) {
    store
        .subscribers
        .retain(|(name, tx)| name != channel || tx.send(payload.to_owned()).is_ok());
}

fn take(counter: &AtomicU32) -> bool {
    counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
        .is_ok()
}

fn respond(out: &mut TcpStream, status: u16, body: &Value, headers: &[(&str, &str)]) {
    let text = body.to_string();
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        429 => "Too Many Requests",
        _ => "Error",
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        text.len()
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = out.write_all(head.as_bytes());
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}
