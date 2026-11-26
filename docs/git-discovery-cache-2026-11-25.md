# Git discovery cache (2026-11-25, HS2-CBZV9W)

The machine server started a new `git` process for every repository location question
and asked the same questions many times. This records what was measured, what changed
and what was left alone. The cache is in
`crates/hotsheet-ticketing/src/git_discovery.rs`.

Everything here was measured on one macOS machine (Apple Silicon) with a debug build.
Windows was not measured. Earlier Windows measurements (HS2-GDARTB) put one `git` spawn
at 90-135 ms on the Windows 11 ARM64 VM, against about 14 ms on macOS.

## Method

A `git` shim placed first on `PATH` logged the parent process id, working directory and
arguments of every call, then ran `/usr/bin/git`. The workload was one run of
`clients/web/scripts/local-host.e2e.test.mjs` (two tests), the run the ticket names. It
used synthetic data only.

## Results

| Measure                                                     | Before (`2fa5e0e83`) | After |
| ----------------------------------------------------------- | -------------------- | ----- |
| `git` processes in the whole run                            | 774                  | 635   |
| `rev-parse` processes                                       | 375                  | 236   |
| Store owner probe (`--show-toplevel --absolute-git-dir`)    | 174                  | 72    |
| Checkout exclude probe (`--git-common-dir --show-prefix`)   | 90                   | 67    |
| `git` processes from the first test's machine server        | 235                  | 188   |
| `git` processes from the second test's machine server       | 153                  | 116   |
| Owner probes from one machine server, the most repeated one | 38                   | 3     |
| Wall time of the run                                        | 33 s                 | 25 s  |

The after numbers are the same in two runs (635 and 636 processes, both 25 s). The wall
time is from single runs and includes browser work, so treat it as indicative.

At 14 ms per spawn the removed 139 processes are about 2 s of process time on macOS.
At 90-135 ms they are about 12-19 s on Windows.

## What remains

- **Short-lived processes.** The remaining discovery probes come from about 70
  processes that each probe once, such as CLI invocations. An in-process cache cannot
  help a process that asks once.
- **Repository and store status.** `git status` for the checkout status and the index's
  dirty-ticket baseline (about 160) and the store autocommit sequence (`ls-files`,
  `diff --cached`, `add`; about 110) read state that changes. They are not discovery
  and are out of scope here.
- **The Windows budget.** `local-host.e2e.test.mjs` keeps its 2x Windows budget
  (HS2-GDARTB) until the Windows runners are measured inside 120 s.

## How the cache stays correct

An answer is kept per canonical directory and checked on every lookup against a stamp
taken before Git ran. There is no expiry, timer or polling.

- **Location.** The stamp holds the directory's own identity and the `.git` entry of
  the directory and every ancestor. A `.git` directory adds its `config`. A gitfile adds
  its size, mtime and the directory it points to. A deleted and recreated path, a moved
  checkout (HS2-86ES0B), a repository created or removed on the path, a linked worktree
  added, pruned or re-added, and a retargeted symlink all miss.
- **`HEAD`.** For the store owner probe, the stamp also holds the `HEAD` content and
  the metadata of the named ref, `packed-refs` and `reftable/tables.list`. Git replaces
  each by renaming a new lock file, so commits, branch switches, resets and `pack-refs`
  all miss.
- **Never cached.** A probe that fails for any reason except "not a Git repository" is
  not cached. A probe whose Git directory differs from the one the stamp inspected is
  not cached either. The `ls-files` check that decides whether a nested store is tracked
  always asks Git.

Tests: `crates/hotsheet-ticketing/src/git_discovery_tests.rs` covers repeated lookups,
`HEAD` changes, a repository created after the first lookup, a path removed and
recreated, a relocation, linked worktrees, a symlinked path, concurrent lookups, a bare
repository and the checkout exclude probe.
