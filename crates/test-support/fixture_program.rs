//! A portable fixture program for tests that spawn a real process (HS2-6EW369).
//!
//! Tests used to spawn `/bin/sh -c <script>`, `/bin/echo` or `/bin/sleep`. Those paths do not
//! exist on Windows, where the product rejects `/bin/sh` as neither a binary name nor an
//! absolute path, so those tests never exercised Windows. [`path`] returns the workspace's
//! deterministic `hs-fake-agent` (`crates/hotsheet-terminals/src/bin/fake_agent.rs`) instead,
//! which behaves the same on every OS. Its flags form a left-to-right script:
//!
//! | Unix fixture | Portable flags |
//! | --- | --- |
//! | `/bin/echo hello` | `--print hello` |
//! | `/bin/sleep 10` | `--hold-ms 10000` |
//! | `sh -c 'exit 7'` | `--exit 7` |
//! | `sh -c '[ "$FOO" = bar ]'` | `--require-env FOO bar` |
//! | `sh -c 'printf "x:%s" "$VAR"'` | `--print-template 'x:{env:VAR}'` |
//! | `sh -c '… "$1"' fake` plus appended args | `… {arg:1} … --` |
//! | `sh -c <one appended argument>` | `--run <one appended argument>` |
//! | `printf ok >&2` | `--print-stderr ok` (HS2-5E0D96) |
//! | `echo x >> runs` | `--append runs x` |
//! | `pwd > here` | `--write-cwd here` |
//! | `while [ ! -f go ]; do sleep 0.05; done` | `--wait-file go` |
//! | `[ -f allow ]` | `--require-file allow` |
//! | `if [ -f seen ]; then exit 0; else touch seen; exit 3; fi` | `--fail-once seen 3` |
//! | `while :; do echo x; done` | `--flood x` |
//! | `head -c 4096 /dev/zero` | `--print-zeros 4096` |
//! | `sleep 30 & …` | `--spawn-hold-ms 30000 …` |
//! | `test -z "${NAME+x}"` / `test -n "$NAME"` | `--require-unset NAME` / `--require-set NAME` |
//!
//! The binary is built with the workspace (`cargo nextest run --workspace` builds it). Include
//! this file with `#[path = "../../test-support/fixture_program.rs"] mod fixture_program;`;
//! it brings its own copies of `workspace_bin` and `first_launch`.

// A crate that includes these helpers itself compiles a second private copy here; each copy
// works on its own, so the duplicate is harmless and keeps this helper self-contained.
#[allow(
    clippy::duplicate_mod,
    reason = "HS2-6EW369: self-contained fixture helper; a second private copy is harmless"
)]
#[path = "first_launch.rs"]
mod first_launch;
#[allow(
    clippy::duplicate_mod,
    reason = "HS2-6EW369: self-contained fixture helper; a second private copy is harmless"
)]
#[path = "workspace_bin.rs"]
mod workspace_bin;

/// The fixture program's absolute path, launched once to completion first so that a fresh
/// executable's first-launch assessment never counts against a test's own windows.
pub fn path() -> String {
    first_launch::launched(workspace_bin::path("hs-fake-agent"), &["--exit", "0"])
        .into_os_string()
        .into_string()
        .expect("UTF-8 target directory")
}
