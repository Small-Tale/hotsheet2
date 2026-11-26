//! `hs-fake-agent` (HS2-1GJY50) — a **deterministic PTY-byte emulator** for terminal E2E
//! tests. It emits the exact escape sequences a real AI tool would (OSC-133 command markers
//! for busy/idle, OSC-7 cwd, OSC-9 progress, braille spinner glyphs), prints text, holds, and
//! exits with a chosen code — so a test can drive the terminal-inference stack (busy detector,
//! OSC scanner, scrollback, exit) without a real tool or brittle `cat`/`printf` tricks.
//!
//! Flags are a **script**, executed left-to-right:
//!
//! ```text
//! hs-fake-agent \
//!   --cwd /work/proj      # OSC 7  (report cwd)
//!   --busy                # OSC 133;C (command output begins → busy)
//!   --print "building…"   # write a line of output
//!   --spinner             # a braille spinner glyph
//!   --progress 42         # OSC 9;4 progress = 42%
//!   --hold-ms 500         # stay alive (still "busy") this long
//!   --idle                # OSC 133;D (finished → idle)
//!   --exit 0              # exit with this status
//!   --permission Bash "rm x" # ask HOTSHEET_SERVER and print its allow/deny decision
//!   --probe AGENTS.md     # note whether this file exists in the working directory
//!   --report /tmp/seen    # write what the agent saw at this point, as JSON (HS2-WEFJV8)
//!   --report-dir /tmp/r   # the same, to <dir>/<HOTSHEET_TERMINAL_ID>.json
//!   --serve-opencode 1500 # after this many ms, answer as an OpenCode session API (HS2-2DPGDY)
//!   --version             # report v1 for the push-capable fleet fixture (HS2-2TT0YG)
//!   --prompt "text"       # a first prompt given at launch, recorded like a pushed one
//!   -i "text"             # the same, spelled as Antigravity's `agy -i` (HS2-KK3CCE)
//!   --print-template "t"  # write t without a newline, expanding {env:NAME} and {arg:N} (HS2-6EW369)
//!   --require-env N v     # exit 1 at once unless environment variable N equals v (HS2-6EW369)
//!   --run "--exit 3"      # run the whitespace-separated flags of one argument (HS2-6EW369)
//!   --wait-line           # block until one line (or end of input) arrives on stdin (HS2-YM55P3)
//!   --print-stderr "t"    # write t to stderr without a newline (HS2-5E0D96)
//!   --append f "t"        # append the line t to file f, created if missing (HS2-5E0D96)
//!   --write-cwd f         # write the working directory to file f (HS2-5E0D96)
//!   --wait-file f         # block until file f exists (HS2-5E0D96)
//!   --require-file f      # exit 1 at once unless file f exists (HS2-5E0D96)
//!   --fail-once f 3       # if file f is missing, create it and exit 3 at once (HS2-5E0D96)
//!   --flood "t"           # write the line t forever, until the output is closed (HS2-5E0D96)
//!   --print-zeros 4096    # write this many zero bytes to stdout (HS2-5E0D96)
//!   --spawn-hold-ms 500   # start a child that holds the inherited streams this long (HS2-5E0D96)
//!   --require-unset N     # exit 1 at once if environment variable N is set (HS2-5E0D96)
//!   --require-set N       # exit 1 at once unless environment variable N is set and non-empty
//!   --                    # end of the script; later arguments are positional ({arg:1}, …)
//! ```
//!
//! HS2-6EW369: tests that spawned `/bin/sh -c …`, `/bin/echo` or `/bin/sleep` use this program
//! instead, through `crates/test-support/fixture_program.rs`, so they run unchanged on Windows.
//! `--run` stands in for `sh -c <script>` where a host appends one argument (a spawn drive's
//! prompt), and `--` stands in for `"$@"`, the arguments a host appends after the configured
//! ones (a push mechanism's `--port <p>`).
//!
//! HS2-5E0D96: the orchestration validation suites script their steps with the file, stream,
//! environment and process-tree flags above instead of `sh` lines, so their sequences
//! (retries, cancel, held steps, timeouts ending the whole tree) also run on Windows.
//!
//! `--serve-opencode` makes the agent push-capable the way the `opencode-http` mechanism
//! expects: it listens on the `--port` its host appends, behind HTTP Basic auth with
//! `OPENCODE_SERVER_PASSWORD`, then ends a turn (busy, then idle) so its host knows it
//! is ready. Each prompt it receives is appended to
//! `<report-dir>/<HOTSHEET_TERMINAL_ID>.prompts`, one JSON line each.

use std::io::{Read as _, Write};
use std::time::Duration;

fn main() {
    let all = expand_runs(std::env::args().skip(1).collect());
    // HS2-6EW369: everything after `--` is positional, like a shell's `"$@"`.
    let (args, positional) = match all.iter().position(|arg| arg == "--") {
        Some(end) => (all[..end].to_vec(), all[end + 1..].to_vec()),
        None => (all.clone(), Vec::new()),
    };
    // Not held locked: a `--serve-opencode` thread writes to the terminal too.
    let mut w = std::io::stdout();
    let mut exit_code = 0;
    let mut probes: Vec<String> = Vec::new();
    // Flags its host appends after the script are read up front.
    let flag_value = |name: &str| {
        all.iter()
            .position(|arg| arg == name)
            .and_then(|index| all.get(index + 1).cloned())
    };
    let prompts = flag_value("--report-dir").map(|dir| {
        let id = std::env::var("HOTSHEET_TERMINAL_ID").unwrap_or_default();
        std::path::PathBuf::from(dir).join(format!("{id}.prompts"))
    });
    // HS2-2DPGDY: a first prompt its host put on the command line.
    // HS2-KK3CCE: `-i` is how Antigravity's `agy` takes it.
    if let (Some(text), Some(path)) = (
        flag_value("--prompt").or_else(|| flag_value("-i")),
        &prompts,
    ) {
        record_prompt(path, &text);
    }

    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        // Does this flag take a value?
        let val = |i: usize| args.get(i + 1).map(String::as_str).unwrap_or("");
        match flag {
            "--busy" => {
                let _ = write!(w, "\x1b]133;C\x07");
            }
            "--idle" => {
                let _ = write!(w, "\x1b]133;D\x07");
            }
            "--cwd" => {
                let _ = write!(w, "\x1b]7;file://host{}\x07", val(i));
                i += 1;
            }
            "--progress" => {
                let _ = write!(w, "\x1b]9;4;1;{}\x07", val(i));
                i += 1;
            }
            "--spinner" => {
                // A braille-pattern glyph (U+280B) — `contains_spinner` detects the block.
                let _ = write!(w, "⠋");
            }
            "--print" => {
                let _ = writeln!(w, "{}", val(i));
                i += 1;
            }
            "--hold-ms" => {
                let _ = w.flush();
                if let Ok(ms) = val(i).parse::<u64>() {
                    std::thread::sleep(Duration::from_millis(ms));
                }
                i += 1;
            }
            "--permission" => {
                let tool = val(i).to_string();
                let action = args.get(i + 2).cloned().unwrap_or_default();
                match ask_permission(&tool, &action) {
                    Ok(decision) => {
                        let _ = writeln!(w, "permission:{decision}");
                    }
                    Err(e) => {
                        eprintln!("hs-fake-agent: permission request failed: {e}");
                        exit_code = 2;
                    }
                }
                i += 2;
            }
            "--probe" => {
                probes.push(val(i).to_string());
                i += 1;
            }
            "--report" | "--report-dir" => {
                let mut path = std::path::PathBuf::from(val(i));
                if flag == "--report-dir" {
                    let id = std::env::var("HOTSHEET_TERMINAL_ID").unwrap_or_default();
                    path = path.join(format!("{id}.json"));
                }
                // Written whole and then renamed, so a reader never sees half of it.
                let partial = path.with_extension("partial");
                if let Err(e) = std::fs::write(&partial, report(&probes))
                    .and_then(|()| hotsheet_sync::files::replace(&partial, &path))
                {
                    eprintln!("hs-fake-agent: report failed: {e}");
                    exit_code = 2;
                }
                i += 1;
            }
            "--port" | "--prompt" | "-i" => i += 1,
            "--version" => {
                // HS2-2TT0YG: push-capable fleet fixtures use the v1 launch protocol.
                let _ = writeln!(w, "hs-fake-agent 1.0.0");
            }
            "--serve-opencode" => {
                let delay = val(i).parse::<u64>().unwrap_or(0);
                let port = flag_value("--port").and_then(|port| port.parse::<u16>().ok());
                let prompts = prompts.clone();
                match port {
                    Some(port) => {
                        std::thread::spawn(move || serve_opencode(port, delay, prompts));
                    }
                    None => eprintln!("hs-fake-agent: --serve-opencode needs --port"),
                }
                i += 1;
            }
            "--exit" => {
                exit_code = val(i).parse::<i32>().unwrap_or(0);
                i += 1;
            }
            "--print-template" => {
                let _ = write!(w, "{}", expand_template(val(i), &positional));
                i += 1;
            }
            "--wait-line" => {
                // HS2-YM55P3: a test decides when a turn ends by typing a line, on any OS.
                let _ = w.flush();
                let mut line = String::new();
                let _ = std::io::stdin().read_line(&mut line);
            }
            "--require-env" => {
                let expected = args.get(i + 2).map(String::as_str).unwrap_or("");
                if std::env::var(val(i)).ok().as_deref() != Some(expected) {
                    let _ = w.flush();
                    std::process::exit(1);
                }
                i += 2;
            }
            "--print-stderr" => {
                let _ = write!(std::io::stderr(), "{}", val(i));
                i += 1;
            }
            "--append" => {
                let text = args.get(i + 2).map(String::as_str).unwrap_or("");
                let appended = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(val(i))
                    .and_then(|mut file| writeln!(file, "{text}"));
                if let Err(e) = appended {
                    eprintln!("hs-fake-agent: append failed: {e}");
                    exit_code = 2;
                }
                i += 2;
            }
            "--write-cwd" => {
                let cwd = std::env::current_dir().unwrap_or_default();
                if let Err(e) = std::fs::write(val(i), cwd.to_string_lossy().as_bytes()) {
                    eprintln!("hs-fake-agent: write-cwd failed: {e}");
                    exit_code = 2;
                }
                i += 1;
            }
            "--wait-file" => {
                let _ = w.flush();
                // The fixture has no other way to learn of the file; the test creates it.
                while !std::path::Path::new(val(i)).exists() {
                    std::thread::sleep(Duration::from_millis(20));
                }
                i += 1;
            }
            "--require-file" => {
                if !std::path::Path::new(val(i)).exists() {
                    let _ = w.flush();
                    std::process::exit(1);
                }
                i += 1;
            }
            "--fail-once" => {
                if !std::path::Path::new(val(i)).exists() {
                    let _ = std::fs::write(val(i), "");
                    let _ = w.flush();
                    std::process::exit(args.get(i + 2).and_then(|c| c.parse().ok()).unwrap_or(1));
                }
                i += 2;
            }
            "--print-zeros" => {
                let count = val(i).parse::<u64>().unwrap_or(0);
                if std::io::copy(&mut std::io::repeat(0).take(count), &mut w).is_err() {
                    exit_code = 2;
                }
                i += 1;
            }
            "--flood" => {
                let line = val(i);
                while writeln!(w, "{line}").is_ok() {}
                std::process::exit(2);
            }
            "--spawn-hold-ms" => {
                let _ = w.flush();
                // The child inherits this process's streams, as a shell's `sleep &` would.
                if let Err(e) = std::env::current_exe().and_then(|program| {
                    std::process::Command::new(program)
                        .args(["--hold-ms", val(i)])
                        .spawn()
                }) {
                    eprintln!("hs-fake-agent: spawn failed: {e}");
                    exit_code = 2;
                }
                i += 1;
            }
            "--require-unset" => {
                if std::env::var_os(val(i)).is_some() {
                    let _ = w.flush();
                    std::process::exit(1);
                }
                i += 1;
            }
            "--require-set" => {
                if std::env::var_os(val(i)).is_none_or(|value| value.is_empty()) {
                    let _ = w.flush();
                    std::process::exit(1);
                }
                i += 1;
            }
            other => {
                eprintln!("hs-fake-agent: unknown flag {other}");
            }
        }
        let _ = w.flush();
        i += 1;
    }
    let _ = w.flush();
    std::process::exit(exit_code);
}

/// HS2-6EW369: replace each `--run <text>` with the whitespace-separated words of `<text>`.
fn expand_runs(args: Vec<String>) -> Vec<String> {
    let mut expanded = Vec::with_capacity(args.len());
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--run" {
            let text = args.next().unwrap_or_default();
            expanded.extend(text.split_whitespace().map(str::to_owned));
        } else {
            expanded.push(arg);
        }
    }
    expanded
}

/// HS2-6EW369: expand `{env:NAME}` (empty when unset) and `{arg:N}` (the Nth positional
/// argument, from 1; empty when absent) in `template`.
fn expand_template(template: &str, positional: &[String]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail.find('}') else {
            out.push_str(tail);
            return out;
        };
        let key = &tail[1..end];
        if let Some(name) = key.strip_prefix("env:") {
            out.push_str(&std::env::var(name).unwrap_or_default());
        } else if let Some(index) = key
            .strip_prefix("arg:")
            .and_then(|n| n.parse::<usize>().ok())
        {
            let value = index
                .checked_sub(1)
                .and_then(|index| positional.get(index))
                .map(String::as_str);
            out.push_str(value.unwrap_or(""));
        } else {
            out.push_str(&tail[..=end]);
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// HS2-WEFJV8: what a launched tool sees at its start: its working directory, the
/// `HOTSHEET_*` environment its host gave it, whether each probed file exists there, and
/// its command line (HS2-KK3CCE).
fn report(probes: &[String]) -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    let env: serde_json::Map<String, serde_json::Value> = std::env::vars()
        // The bearer secret is named, never written out.
        .filter(|(key, _)| key.starts_with("HOTSHEET_"))
        .map(|(key, value)| {
            let shown = if key == "HOTSHEET_SECRET" {
                "<set>".to_string()
            } else {
                value
            };
            (key, serde_json::Value::String(shown))
        })
        .collect();
    let files: serde_json::Map<String, serde_json::Value> = probes
        .iter()
        .map(|probe| (probe.clone(), cwd.join(probe).is_file().into()))
        .collect();
    let args: Vec<String> = std::env::args().skip(1).collect();
    serde_json::json!({ "cwd": cwd, "env": env, "files": files, "args": args }).to_string()
}

/// Append one received prompt as a JSON line.
fn record_prompt(path: &std::path::Path, text: &str) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{}", serde_json::json!({ "text": text }));
    }
}

/// HS2-2DPGDY: the part of OpenCode's session API the `opencode-http` push adapter
/// uses, enough for a host to deliver prompts into this agent.
fn serve_opencode(port: u16, delay_ms: u64, prompts: Option<std::path::PathBuf>) {
    use std::io::{BufRead, BufReader, Read};
    std::thread::sleep(Duration::from_millis(delay_ms));
    let Ok(listener) = std::net::TcpListener::bind(("127.0.0.1", port)) else {
        eprintln!("hs-fake-agent: cannot listen on {port}");
        return;
    };
    let password = std::env::var("OPENCODE_SERVER_PASSWORD").unwrap_or_default();
    let expected = {
        use base64::Engine as _;
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("opencode:{password}"))
        )
    };
    {
        // Ready: one turn ends, which is how the host learns of it.
        let mut out = std::io::stdout().lock();
        let _ = write!(out, "\x1b]133;D\x07");
        let _ = writeln!(out, "listening");
        let _ = out.flush();
    }
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let mut reader = BufReader::new(match stream.try_clone() {
            Ok(clone) => clone,
            Err(_) => continue,
        });
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            continue;
        }
        let mut length = 0usize;
        let mut authorization = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                break;
            }
            let (name, value) = line.split_once(':').unwrap_or((&line, ""));
            match name.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = value.trim().parse().unwrap_or(0),
                "authorization" => authorization = value.trim().to_string(),
                _ => {}
            }
        }
        let mut body = vec![0; length];
        let _ = reader.read_exact(&mut body);
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts
            .next()
            .unwrap_or_default()
            .split('?')
            .next()
            .unwrap_or_default()
            .to_string();
        let (status, reply) = if authorization != expected {
            ("401 Unauthorized", String::new())
        } else {
            match (method.as_str(), path.as_str()) {
                ("GET", "/session") => ("200 OK", "[]".to_string()),
                ("POST", "/session") => ("200 OK", r#"{"id":"ses_fake"}"#.to_string()),
                ("POST", "/tui/select-session" | "/tui/show-toast") => {
                    ("200 OK", "true".to_string())
                }
                ("POST", "/session/ses_fake/prompt_async") => {
                    if let Some(path) = &prompts {
                        let text = serde_json::from_slice::<serde_json::Value>(&body)
                            .ok()
                            .and_then(|value| value["parts"][0]["text"].as_str().map(str::to_owned))
                            .unwrap_or_default();
                        record_prompt(path, &text);
                    }
                    ("204 No Content", String::new())
                }
                _ => ("404 Not Found", String::new()),
            }
        };
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
            reply.len()
        );
        let _ = stream.flush();
    }
}

fn ask_permission(tool: &str, action: &str) -> Result<String, Box<dyn std::error::Error>> {
    let url = std::env::var("HOTSHEET_SERVER")?;
    let secret = std::env::var("HOTSHEET_SECRET")?;
    let connection =
        std::env::var("HOTSHEET_CONNECTION").unwrap_or_else(|_| "hs-fake-agent".to_string());
    let project = std::env::var("HOTSHEET_PROJECT").unwrap_or_default();
    let body = serde_json::json!({
        "project": project,
        "connection": connection,
        "tool": tool,
        "action": action,
    })
    .to_string();
    let text = ureq::post(&format!("{}/permissions/ask", url.trim_end_matches('/')))
        .set("X-Hotsheet-Secret", &secret)
        .set("Content-Type", "application/json")
        .send_string(&body)?
        .into_string()?;
    let response: serde_json::Value = serde_json::from_str(&text)?;
    response["decision"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "permission response has no decision".into())
}
