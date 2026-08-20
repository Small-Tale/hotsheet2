#![no_main]
//! Fuzz the ticket file parser: arbitrary bytes must never panic (degrade-not-panic,
//! `docs/17` §17.4). The same invariant is checked in CI (offline) by the
//! `parse_never_panics_*` proptests; this target is for deeper, coverage-guided runs.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = hotsheet_model::parse_file(text);
    }
});
