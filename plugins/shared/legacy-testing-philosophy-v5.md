<!-- hotsheet:begin section=testing-philosophy v=5 -->

## Testing Philosophy

- Scale testing to risk. Declare a tier in the ticket's preliminary-thoughts note (in the claim note for trivial work); reviewers may raise it. **T0 trivial** (copy, docs, constants, one style token or config value): lint, typecheck, and directly related tests; check visuals only when something visibly changes. **T1 normal** (an ordinary feature or bug fix): the affected tests only; a fix adds one regression test at the most natural layer, and a new feature gets unit and end-to-end coverage. **T2 high-risk** (merge and sync logic, claims and leases, indexes, file formats and parsers, shared components, auth and credentials, data migrations, and any surface the project names as high-risk): everything below, plus the full affected suites.
- Cover each feature with unit tests of its logic and end-to-end tests through the real shipped flow. Mock external dependencies in unit tests; keep browser fixtures faithful to real server shapes, optional fields, and status codes. Exercise each newly composed API surface against a real server as well as mocked client tests.
- Aim to cover every documented feature and reach 100% line and branch coverage. Treat those numbers as a floor: executed lines do not prove behavior or sequences. For stateful modules, enumerate states and transitions, then test repeated, interleaved, out-of-order, reset, and empty-then-refill sequences.
- Test stateful controls in both directions: control to application state and rendered output, and programmatic state changes back to live controls. Verify resets and a further edit. For custom elements, assert live properties and relevant focus/events. Check every part of a selected presentation, including icons, badges, counts, and other derived decoration.
- Exercise child actions through each shipped parent composition. A demo or isolated preview is insufficient evidence that the real parent wires an enabled control; disable unsupported controls explicitly.
- Maintain a feature-by-feature record of unit, end-to-end, and manual coverage as behavior changes. Keep a manual test plan for behavior that cannot be reliably automated.
- Give every code package a working local lint command and configuration when it is added. Keep lint warning-free, fix lint and type errors before finishing, and document justified suppressions.

<!-- hotsheet:end section=testing-philosophy -->
