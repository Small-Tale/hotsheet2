# DOM test pilot (HS2-3FQNGT, 2026-11-10)

## Question and failure evidence

Could a browser-like Vitest environment catch client behavior errors earlier than the current browser gate? The six deduplicated failures in the ticket's sampled CI/local logs were: one ambiguous project-tab locator (application state or selector), two missing OpenAPI route groups (API contract), one `networkidle` timeout (timing), one stale search-help count (test expectation), and one stale Completed-worker card expectation (test expectation). The last two were already caught by component tests or reflected intentional behavior. None is an established product logic escape. This selected sample cannot estimate a general failure rate. No customer content was copied.

## Prototype and measurement

The `dom-pilot` Vitest project uses jsdom 29.1.1 for three focused cases in `src/components/dom-pilot.dom.test.ts`: a rendered `TicketSearchField` with its real delegated help/insert action, a rendered `WorkerColumns` Started → Completed → Started transition from a synthetic server-shaped view, and a capability probe. The action test invokes the production parent wiring; the Workers test renders the production parent with the same qualified ticket shape used by Playwright. The probe exercised custom-element registration, open shadow root, focus, `MutationObserver`, composed bubbling `input`, and reset/re-edit. All three passed. The initial implementation found two test mistakes (checking committed query text instead of token values, and reading `composedPath()` after dispatch); neither was a product defect.

Installing the extra DOM dependency added 33 packages to an already installed web checkout, taking 8 seconds; a clean `npm ci` of the baseline web package took 14 seconds in this worktree. Five sequential fresh-process pilot runs took 5.19, 2.97, 2.25, 1.90, and 1.79 seconds, including Vitest startup and transformation (nearest-rank p50 2.25s, p75 2.97s). The first verbose run reported 201ms for the search case, 20ms for Workers, 4ms for the API probe, and 1.79s of environment setup. These are one busy Mac's observations, not a CI or cross-platform prediction.

The existing Playwright search-help insertion flow passed twice at each of 1280px and 390px on the same synthetic project fixture: case durations 4.400, 2.685, 2.916, and 2.805 seconds (nearest-rank p50 2.805s, p75 2.916s); the four-case command took 19.65s including Vite/browser setup. The existing Workers workspace flow passed twice with 9.009s and 6.969s case durations (p50 6.969s, p75 9.009s); its two-case command took 20.58s. These browser flows cover more interactions and real routing than the pilot, so the duration ratio is **not** time saved by removing them. The pilot reproduces no known historical product failure sooner than an existing test.

## Decision and limits

Keep the small DOM project as double coverage for delegated search-help wiring and the Workers parent transition. It adds a fast, executable check of parent DOM composition, while the established string tests continue to cover model details. Do not migrate or remove any Playwright cases on this evidence. The expected browser-gate time saved is **zero** with this decision. The continuing cost is the jsdom dependency, three test cases, and roughly 2–3 seconds of warm local Vitest process time in this sample; the installed dependency and transformed-suite cost may differ in CI.

Retain real-server Playwright contracts for API shape, routing, network and worker lifecycle, browser-specific custom-element behavior, keyboard focus, and responsive layout. Retain browser visual QA for geometry, clipping, typography, and navigation. jsdom's DOM event and focus checks are useful early signals, but it does not supply real layout or navigation evidence. A later browser failure that escapes existing unit coverage would be a reason to revisit broader adoption with a new ticket and a measured detection comparison. No broad suite migration is proposed now.

Sources: [Vitest environments](https://vitest.dev/guide/environment.html), [jsdom README](https://github.com/jsdom/jsdom/blob/main/README.md).
