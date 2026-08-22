//! Hot Sheet 2 **terminal / PTY manager** (`docs/05` §5.4/§5.6, `docs/12` §12.2.1, HS2-10)
//! — nearly standalone (deps: `portable-pty`). Per-project PTYs with a scrollback ring,
//! multi-viewer sharing, environment scrubbing, and byte-stream **busy inference** (OSC 133
//! + a spinner hint) that feeds the connection registry.
//!
//! Built here: [`Terminal`] (one PTY session), [`TerminalManager`] (lazy per-project
//! terminals), [`BusyDetector`] + [`contains_spinner`], and env [`scrub_env`]. Follow-ons:
//! the **detached broker** (survive server restart), server-arbitrated **PTY sizing**
//! (HS2-62), OSC 7/8 cwd/hyperlink handling, and the server HTTP/WS wiring.

pub mod busy;
pub mod env;
pub mod manager;
pub mod terminal;

pub use busy::{Activity, BusyDetector, contains_spinner};
pub use env::scrub_env;
pub use manager::{TermKey, TerminalManager};
pub use terminal::{SCROLLBACK_BYTES, TermError, TermSpec, Terminal};
