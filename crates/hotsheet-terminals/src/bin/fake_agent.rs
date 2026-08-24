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
//! ```

use std::io::Write;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = std::io::stdout();
    let mut w = out.lock();
    let mut exit_code = 0;

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
