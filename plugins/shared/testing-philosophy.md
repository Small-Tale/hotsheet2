<!-- hotsheet:begin section=testing-philosophy v=8 -->

## Testing Philosophy

- Declare risk tier in preliminary thoughts (claim note for trivial work); reviewers may raise it. **T0 trivial**: copy/docs/constants/one style token or config value; lint, typecheck, directly related tests, visuals when visible. **T1 normal**: affected tests; a fix adds a regression at the natural layer, a feature gets unit and E2E coverage. **T2 high-risk**: merge/sync, claims/leases, indexes, formats/parsers, shared components, auth/credentials, migrations and project-named risks; all requirements below plus full affected suites.
- Cover each feature with isolated unit tests (external dependencies mocked) and real-flow E2E with minimal mocking. Fakes match real shapes, optional fields and status codes. Newly composed APIs need real-server exercises as well as mocked-client tests.
- Aim for all documented features and 100% line/branch coverage; execution is not proof of behavior/sequences. Enumerate state/transition matrices; test repeated, interleaved, out-of-order, reset and empty-then-refill sequences.
- Test controls both ways: user control → app state/output and programmatic state → live controls; reset, then edit again. Assert custom-element live properties, focus/events and all derived icons/badges/counts. Exercise child actions through shipped parents; isolated demos cannot prove wiring. Explicitly disable unsupported controls.
- Maintain feature-by-feature unit/E2E/manual evidence as behavior changes and a manual plan for what cannot reliably be automated.
- Every new code package has working local lint/config. Fix lint/type errors before finishing; keep warning-free and document justified suppressions. Reviewer convention corrections get a source policy test or lint rule named with the ticket slug in the same change.
- Keep output quiet: use failure-only reporters or the project's quiet wrapper; read summaries/failure excerpts, not full logs. A background agent may report pass/fail and failures only.
- **Rerun only what failed.** Target failing tests until green, then run the affected set once. After rebase rerun only overlapping affected areas; unrelated commits need no rerun. Never repeat a passing suite just to double-check.
- Verify the rebuilt instance using the user's actual steps/command; never test against the user's running host or a stale build. `test-operations` owns isolation, build-once and flake policy when installed.

<!-- hotsheet:end section=testing-philosophy -->
