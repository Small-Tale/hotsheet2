<!-- hotsheet:begin section=testing-philosophy v=2 -->

## Testing Philosophy

- **Double coverage**: every feature covered by both unit tests AND E2E tests. Unit = logic in isolation; E2E = real user flows through the running app with minimal mocking.
- **Unit tests**: Mock external deps (filesystem, network), test real logic.
- **E2E tests**: As much as possible, use test automation tools to run realistic, user-facing flows. Minimize mocks.
- **Mock the exact transport contract**: browser fixtures must use the server's real wire
  shape, including flattening/envelopes, optional fields, and status codes. Do not invent a
  more convenient response shape for a client test. For each newly composed real API
  surface, run at least one integration or opt-in local-browser flow against the actual
  server; a mocked UI test alone cannot validate the adapter boundary.
- **Coverage**: Merge all test coverage (e.g. unit, E2E server, E2E browser) into one report. Low-coverage files should get more of both test types. Aim for 100% coverage of code lines, 100% coverage of branches, and 100% of features described in the requirements documentation.
- **Coverage is a floor, not a ceiling**: 100% line/branch coverage shows every line _ran_, not that every _behavior_ — or every _sequence_ of behaviors — is _asserted_. It is structurally blind to a **missing state transition**: a bug living in an untested interaction sails through a green 100% report because the individual lines still get hit by isolated, single-operation tests.
- **Transition-matrix testing for stateful modules**: for anything with modes / multiple code paths / a cache / a state machine, enumerate the states AND the transitions between them, then write tests that walk realistic multi-step sequences crossing state boundaries — not just each operation from a clean initial state.
- **Adversarial pass on stateful changes**: when adding or altering a stateful code path, deliberately try to break it with out-of-order / interleaved / repeated / empty-then-refill sequences; pin any that would have failed as permanent regression tests.
- **Client state synchronization is bidirectional**: for every stateful control, test
  both control → application state/rendered output and programmatic application state
  → the live control. A stateful client interaction test must establish the initial
  state, change every exposed setting, verify output, reset/replace state, verify every
  control and output again, then make another edit after reset. For custom elements,
  assert live properties such as `value` and `checked` (plus focus and emitted events
  when relevant), not attributes alone. Exercise every rendered action such as Reset,
  Save, Cancel, Apply, Remove, and Undo. Prefer one tested binding/synchronization
  abstraction over per-component repair code.
- **Assert the complete selected presentation after transitions**: when a composite
  control renders a label plus icon, color, badge, checkmark, count, or other derived
  decoration, changing its value must assert every visible facet in the closed/current
  state—not only the label, initial render, or popup options. Exercise the transition
  through the real owning surface (for example an inspector), then verify stale
  decorations disappeared and the new label and decorations agree with application
  state. A menu containing correct icons does not prove its selected-value projection.
- **Exercise child actions through every shipped composition**: a component demo proving
  an action works does not prove a parent composition projects the changed state back
  into that child. For each interactive child used by a composite surface, trigger its
  real action through the composite, assert the owning state changed, and assert the
  composite rerendered the correct child view. Never hardcode a controlled child prop
  in a composition when a shared signal/controller owns that state.
- **Manual test plan**: keep a manual test plan doc (e.g. `docs/manual-test-plan.md`) for features that can't be reliably automated. **Keep it up to date** — add such features there; when you add automated coverage for a previously-manual item, remove it and note it in an "Automated Coverage Summary".
- **Feature coverage matrix**: update [`docs/TEST-COVERAGE.md`](docs/TEST-COVERAGE.md)
  in the same change whenever a feature is added, shipped, changed, deferred, or gains
  or loses unit, E2E, or manual coverage. `node scripts/check-test-coverage.mjs` is the
  CI gate for valid statuses and live evidence paths; line coverage is not a substitute
  for recording both behavioral layers.
- **Always fix lint and type errors before finishing**: Fix as you go, don't batch.
  Every client, server, tool, spike, and other code package must ship with a real
  lint configuration and a package-local lint command from the moment code is added.
  The web/TypeScript baseline is the shared Glassbox ESLint stack (ESLint recommended,
  typed TypeScript rules, import ordering, TSDoc, and Kerf rules); Rust uses the pinned
  toolchain's `rustfmt` and `cargo lint` alias (Clippy for the workspace/all targets/all
  features, warnings plus debug/TODO/unimplemented macros denied). Do not push with a
  lint warning or error. Suppress a rule only at a documented compatibility or external
  boundary—not merely to make the command green—and tighten transitional exceptions as
  touched code is made safe.

<!-- hotsheet:end section=testing-philosophy -->
