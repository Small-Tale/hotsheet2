<!-- hotsheet:begin section=testing-philosophy v=3 -->

## Testing Philosophy

- Cover each feature with unit tests of its logic and end-to-end tests through the real shipped flow. Mock external dependencies in unit tests; keep browser fixtures faithful to real server shapes, optional fields, and status codes. Exercise each newly composed API surface against a real server as well as mocked client tests.
- Treat coverage as a floor: executed lines do not prove behavior or sequences. For stateful modules, enumerate states and transitions, then test repeated, interleaved, out-of-order, reset, and empty-then-refill sequences.
- Test stateful controls in both directions: control to application state and rendered output, and programmatic state changes back to live controls. Verify resets and a further edit. For custom elements, assert live properties and relevant focus/events. Check every part of a selected presentation, including icons, badges, counts, and other derived decoration.
- Exercise child actions through each shipped parent composition. A demo or isolated preview is insufficient evidence that the real parent wires an enabled control; disable unsupported controls explicitly.
- Maintain a feature-by-feature record of unit, end-to-end, and manual coverage as behavior changes. Keep a manual test plan for behavior that cannot be reliably automated. Broaden the affected-package test set for shared or high-risk changes.
- Give every code package a working local lint command and configuration. Fix lint and type errors before finishing; document justified suppressions.

<!-- hotsheet:end section=testing-philosophy -->
