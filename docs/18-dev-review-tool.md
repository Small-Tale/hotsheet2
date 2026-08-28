# Dev Review Ticket Capture Tool

Status: **Shipped initial web implementation** (`HS2-ME9EB6`).

## Purpose

Dev Review lets a reviewer file a Hot Sheet ticket without leaving the web UI being
reviewed. It is development tooling, never ordinary application chrome. The initial
implementation lives in `clients/web/src/dev-review/` behind a framework-neutral
`installDevReview({ submit })` entry point so it can later move into a separately
published npm package without importing Kerf, Web Awesome, or Hot Sheet client state.

## Interaction contract

- An explicit development-review mode adds a very small fixed `Feedback` launcher in
  the top-right stacking layer.
- Activating it shows `New Ticket` plus a concise Option/Alt-drag hint that fades
  after a few seconds.
- `Feedback` is also the mode toggle. It exits immediately when there are no captures
  and asks before discarding an annotated session.
- Holding Option on macOS or Alt elsewhere while dragging over the host application
  shows a crosshair cursor and creates a numbered capture rectangle. Existing
  rectangles can be moved or resized from any side or corner without the modifier;
  handles retain fixed dimensions as their rectangle changes size.
- Holding Option/Alt+Shift over a rectangle changes to a deletion cursor; clicking
  removes the rectangle and its captured PNG.
- A rectangle anchors to the underlying content at its top-left pixel. Window or
  nested-container scrolling moves the overlay with that content, including when the
  rectangle was first drawn after the interface had already scrolled.
- Creation, movement, and resizing debounce PNG recapture through `html2canvas`.
  Pointer movement updates the existing rectangle node without rebuilding handles;
  pending rectangles share one viewport render and stale async results are discarded.
  Review-tool UI is excluded from every capture.
- `New Ticket` opens its modal immediately, prepares any uncached PNGs asynchronously,
  then presents every capture as a selectable thumbnail and large preview.
- The dialog accepts additional files through both drag/drop and a native browse
  control. Captures and uploaded attachments each expose a hover/focus removal control;
  removing a capture also removes its source rectangle from the active session.
- The dialog has one Cancel action in its top-right. Canceling it returns to the
  still-active annotation session. Successful submission clears and exits the session.

## Embedding API

```ts
import { installDevReview } from './dev-review';

const review = installDevReview({
  submit: submission => ticketService.createFromReview(submission),
});

// On app teardown:
review.destroy();
```

The submission adapter receives notes, page URL, viewport dimensions, captured PNG
data URLs, and base64 data URLs plus metadata for user-supplied attachments. It returns
the created ticket slug and may optionally return a ticket URL. This
keeps capture UX portable while allowing a host to use an authenticated Hot Sheet
server, Tauri command, test fake, or another ticket-provider-aware bridge.

## UX demo and security boundary

Open `/ux-demo?dev-review=1` (additional query parameters are fine) while running the
Vite development server. The UX demo adapter posts to
`POST /__hotsheet/dev-review/tickets`, which exists only in the development Hono app,
requires the `x-hotsheet-dev-review: 1` header, and is absent from production builds.

The browser entry point is guarded by Vite's compile-time `import.meta.env.DEV` value
and loads the tool through a dynamic import only after the query flag is present.
`html2canvas` is therefore a development dependency and is not linked into normal
Hot Sheet clients. Every production web build runs
`scripts/check-production-bundle.mjs` and fails if emitted JS or CSS contains a Dev
Review or `html2canvas` signature. Future Tauri and native client entry points must
preserve the same compile-time exclusion; a runtime-hidden button is not sufficient.

For this repository the adapter invokes `target/debug/hotsheet-cli`, creates a bug
tagged `client` and `ux-feedback` in the sibling `hotsheet2.hs2` store, then attaches
each decoded PNG. The CLI mutations defer their individual autocommits so the complete
ticket, captured images, and uploaded files receive one durable local commit; one best-effort remote push is
launched asynchronously and does not hold the dialog open. Alternate paths can be supplied with:

- `HOTSHEET_DEV_REVIEW_STORE`
- `HOTSHEET_DEV_REVIEW_CLI`

The future production/Tauri integration should submit through the authenticated Hot
Sheet server for the checkout's configured project/provider rather than expose a CLI
or filesystem path to browser code.
