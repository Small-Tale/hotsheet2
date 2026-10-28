# Affected local verification

HS2-RS0FFT promotes the HS2-FCCRCK dependency planner to the supported local
gate. `scripts/check.sh --plan` prints the baseline, changed paths, selected and
omitted stages, reasons, and bounded browser commands. `scripts/check.sh` executes
that plan. `scripts/check.sh --full` selects every local package stage. CI retains
the full browser shards, Rust coverage, and platform checks.

Client changes normally run web lint, typecheck, unit tests and build, shared
format/policy/contract/generated-artifact checks, real-server contract and shell
smoke flows, and affected browser files or provider test names. They omit unrelated
Rust, migrator, and spike suites. Source-derived cross-package edges remain in
`scripts/gate-plan.mjs`: public assets can affect Rust, and Rust/migrator changes
select their dependent packages. Unknown paths, missing baseline, configuration,
lockfiles, toolchain, gate scripts, or ambiguous stages select the full package gate.
High-risk changes retain full affected suites.

Run additional browser checks when an actual shared routing/rendering/state change,
auth change, data format/migration, ambiguous dependency, or concrete failure needs
them. Record the reason in the ticket. A full local browser run is warranted for an
unresolved broad browser risk, rather than being a routine duplicate of CI. Visible
changes still receive real-app visual review at wide, medium, and narrow sizes,
including resize transitions. Overlapping tickets in an authorized batch share
one review of the combined result.

## Evidence and push

The executor hashes the committed tree, baseline, toolchain evidence, selected
commands, and relevant tracked input blobs. Successful stage results can be reused
only when those relevant inputs match. Source/dependency, fixture/configuration,
lock/toolchain, or baseline changes invalidate evidence. Dirty inputs are rejected;
preserve local edits and verify the committed snapshot in a clean worktree.
Inputs are rechecked after every executed stage and at completion. Failure or
interruption stops execution and cannot produce a successful receipt.

Receipts live under the Git common directory's `hotsheet-local-gate` directory.
The existing per-worktree `hotsheet-root-gate/<tree>` compatibility marker is written
only after successful verification. Push and its hook call the same executor;
`HOTSHEET_GATE_PASSED_TREE` no longer bypasses checks. The push script runs the gate
before opening the remote connection; the hook reuses valid evidence. A successful
covering main CI run is still required before ticket completion.

## Measurement and limitations (2026-10-28)

The current UI batch's full package gate passed 2,225 Rust tests, 2,806 web unit
tests, 115 root script tests, and 539 coverage rows in roughly nine minutes on a
shared host. Its full local browser run took 35 minutes: 1,016 passed, 13 skipped,
and one obsolete assertion expected the magnified frame to differ from 5:3.
HS2-KR0C0B intentionally changed that aspect. Correcting the assertion retained
containment and PTY checks; the affected terminal suites then passed 16/16 in
1.5 minutes. This was a test-expectation mismatch, not a missed production defect.

Synthetic package selection, receipt invalidation, failure propagation, hook, and
instruction tests pass. HS2-FCCRCK's prior historical model estimated about 70
seconds per integration from package omission; this is not a measured throughput
gain. The shared-host timings are indicative wall observations, without a CPU
breakdown or a matched control. Claim-to-completion time and CI delay are recorded
on HS2-RS0FFT after shipping. HS2-DAA8AB retains quiet-host comparisons and edge
mutation validation; HS2-K1QNGC retains independent-session throughput measurement.
Neither synthetic selection proof nor a green CI run establishes zero missed
regressions. No customer content was copied.
