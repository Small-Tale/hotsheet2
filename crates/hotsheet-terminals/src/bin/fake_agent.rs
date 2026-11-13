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
//! ```

use std::io::Write;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = std::io::stdout();
    let mut w = out.lock();
    let mut exit_code = 0;
    let mut probes: Vec<String> = Vec::new();

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
                    .and_then(|()| std::fs::rename(&partial, &path))
                {
                    eprintln!("hs-fake-agent: report failed: {e}");
                    exit_code = 2;
                }
                i += 1;
            }
            "--exit" => {
                exit_code = val(i).parse::<i32>().unwrap_or(0);
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

/// HS2-WEFJV8: what a launched tool sees at its start: its working directory, the
/// `HOTSHEET_*` environment its host gave it, and whether each probed file exists there.
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
    serde_json::json!({ "cwd": cwd, "env": env, "files": files }).to_string()
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
