//! Hot Sheet 2 domain model + ticket file-format logic.
//!
//! This crate is **pure**: types, ID/slug logic, and (later) the Markdown+YAML
//! parse/serialize + the semantic merge. It performs **no I/O** and has no
//! Hot-Sheet-specific dependencies, so it links cheaply into every surface and into
//! the migrator's conformance test.
//!
//! The canonical field schema this crate implements is
//! `docs/17-ticket-file-format.md`.

pub mod enums;
pub mod format;
pub mod ids;
pub mod ticket;
pub mod timestamp;

pub use enums::{CloseReason, NoteKind, Priority, ReviewKind, Status};
pub use format::{ParseError, parse_file, to_file_string};
pub use ids::{Ulid, derive_slug};
pub use ticket::{ExternalLink, Note, ReviewRequest, Ticket};
pub use timestamp::Timestamp;

/// Frontmatter format version written to `schema:` (forward-migration marker).
pub const SCHEMA_VERSION: u32 = 1;
