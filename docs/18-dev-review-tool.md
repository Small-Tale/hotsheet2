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
- Activating it shows `New Ticket` and `Cancel`, plus a concise Option/Alt-drag hint.
- Holding Option on macOS or Alt elsewhere while dragging over the host application
  creates a numbered capture rectangle. Existing rectangles can be moved or resized
  from any corner without the modifier.
- Creation, movement, and resizing debounce PNG recapture through `html2canvas`.
  Review-tool UI is excluded from every capture.
- `New Ticket` flushes pending captures and opens a modal with every PNG as a
  selectable thumbnail, a large preview, and required feedback notes.
- Canceling the modal returns to the still-active annotation session. Canceling the
  review toolbar exits the session and discards its rectangles. Successful submission
  clears and exits the session.

## Embedding API

```ts
import { installDevReview } from './dev-review';

const review = installDevReview({
  submit: submission => ticketService.createFromReview(submission),
});

// On app teardown:
review.destroy();
```

The submission adapter receives notes, page URL, viewport dimensions, and PNG data
URLs. It returns the created ticket slug and may optionally return a ticket URL. This
keeps capture UX portable while allowing a host to use an authenticated Hot Sheet
server, Tauri command, test fake, or another ticket-provider-aware bridge.

## UX demo and security boundary

Open `/ux-demo?dev-review=1` (additional query parameters are fine) while running the
Vite development server. The UX demo adapter posts to
`POST /__hotsheet/dev-review/tickets`, which exists only in the development Hono app,
requires the `x-hotsheet-dev-review: 1` header, and is absent from production builds.

For this repository the adapter invokes `target/debug/hotsheet-cli`, creates a bug
tagged `client` and `ux-feedback` in the sibling `hotsheet2.hs2` store, then attaches
each decoded PNG. Alternate paths can be supplied with:

- `HOTSHEET_DEV_REVIEW_STORE`
- `HOTSHEET_DEV_REVIEW_CLI`

The future production/Tauri integration should submit through the authenticated Hot
Sheet server for the checkout's configured project/provider rather than expose a CLI
or filesystem path to browser code.
