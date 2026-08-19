//! Ports — the adapter traits through which the ticketing engine reaches the outside
//! world (`docs/12-code-organization-and-testing.md` §12.1, ports & adapters). Real
//! implementations live in the binaries; tests inject fakes. The engine reads no
//! global clock/rng and touches no real filesystem or git directly.

/// A source of wall-clock time, in milliseconds since the Unix epoch.
pub trait Clock {
    /// Current time in milliseconds since the Unix epoch.
    fn now_ms(&self) -> u64;
}

/// A source of randomness for ULID minting.
pub trait Rng {
    /// The next 128-bit random value (ULID uses the low 80 bits).
    fn next_u128(&mut self) -> u128;
}

// Further ports land with their implementations:
//   FileSystem / GitLocal (gix) / GitRemote (git CLI)  — store + reindex (HS2-4/HS2-5)
//   ProcessSpawner                                      — terminals + drives (HS2-9/HS2-10)
// See docs/09 §9.13 (git access) and docs/12 §12.4.
