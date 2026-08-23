//! Hot Sheet 2 **terminal / PTY manager** (`docs/05` §5.4/§5.6, `docs/12` §12.2.1, HS2-10)
//! — nearly standalone (deps: `portable-pty`). Per-project PTYs with a scrollback ring,
//! multi-viewer sharing, environment scrubbing, and byte-stream **busy inference** (OSC 133
//! + a spinner hint) that feeds the connection registry.
//!
//! Built here: [`Terminal`] (one PTY session, with a live output fan-out via
//! [`Terminal::subscribe`]), [`TerminalManager`] (lazy per-project terminals),
//! [`BusyDetector`] + [`contains_spinner`], the [`OscScanner`] (OSC 7/8/9 cwd/hyperlink/
//! progress → [`TermState`]), and env [`scrub_env`]. The server wires the HTTP routes
//! (`/terminals` open/list, `/terminals/{id}` read/kill, `/terminals/{id}/input`) + the live
//! WS attach (`/terminals/{id}/attach`) — HS2-A6R5QV/HS2-XTTTMV. Follow-ons: the **detached
//! broker** (survive server restart), server-arbitrated **PTY sizing** (HS2-62), and feeding
//! busy → the connection registry (HS2-4M67VN).

pub mod busy;
pub mod env;
pub mod manager;
pub mod osc;
pub mod terminal;

pub use busy::{Activity, BusyDetector, contains_spinner};
pub use env::scrub_env;
pub use manager::{TermKey, TerminalManager};
pub use osc::{OscScanner, TermState};
pub use terminal::{SCROLLBACK_BYTES, TermError, TermSpec, Terminal};
