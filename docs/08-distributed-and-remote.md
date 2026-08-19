# 08. Distributed Execution & Remote Access

> **Status: Open.** These are the ticket's two named open questions —
> "how projects can be run on distributed systems, orchestrated through a single
> UI" and "how mobile devices (iOS) can/should be configured and connected to one
> or more servers." This doc frames the design directions and the decisions still
> to make; it does not lock them.

## 8.1 Two axes of "distributed"

Keep two separable concerns apart:

1. **Distributed *execution*** — many AI-tool workers draining one project's Up
   Next pool in parallel. Solved at the data layer by the **claim/lease
   primitive** + git-worktree isolation + "workers never merge, the integrator
   merges." This is largely inherited from HS1 and works on the git+index
   foundation (claim state persists to ticket frontmatter, selection runs over the
   index — [05](05-ai-tool-plugins.md) §5.7). Single-machine first.
2. **Distributed *hosting*** — several servers, each owning some projects,
   surfaced together in one client. This is the "orchestrated through a single UI"
   question and is mostly a **client + transport** concern, not a data one.

## 8.2 One UI over many servers (orchestration)

The model carried from HS1's shipped remote-client work (§112) already fits:

- A client holds a **machine-global registry of remote servers** (`~/.hotsheet/
  remotes.json`): `{ servers: [{ origin, label, deviceClientId, projects: [{ id,
  secret, name }] }] }`.
- Each project tab carries a server `(origin, secret)` — a **local** project's
  origin is this machine's own server (`https?://127.0.0.1:<port>`), a **remote**
  project's is another device's. Every tab is server-backed; there is no
  embedded-core tab kind (§9.1e). The tab machinery is generic over the origin.
- The client shows local and remote projects **side by side**, with per-project
  connectivity state (connected / reconnecting / unreachable) off the WebSocket
  reconnect.

**Orchestration is live-mount only** (maintainer decision, 2026-08-19). A client
reaches another machine's project by **attaching to that machine's running server**
over mTLS (real-time; it drives the AI tools *there*). That is the one path.

> **Decisions (resolved 2026-08-19).**
> - **O1 — orchestration path: live-mount only.** *Automatic cloning is **not** a
>   Hot Sheet feature and never will be.* If a user wants a local copy of a store,
>   they **clone it by hand** and run a local server against it — at which point it
>   is simply a **normal local project** (a `127.0.0.1` origin like any other), not a
>   special "clone-and-serve" mode Hot Sheet manages. This keeps the product's job
>   narrow: Hot Sheet mounts *running servers*; plain git handles copies.
> - **O2 — cross-server aggregate views: deferred past v1.** A single view spanning
>   multiple servers (e.g. "everything Up Next everywhere") is a new client-side
>   query-fan-out surface; ship per-server views first.
> - **O3 — cross-machine work: git-native decentralized self-claim, no central
>   coordinator.** See §8.5 — workers on any machine self-claim over the shared git
>   remote via an atomic-push CAS, so there is no coordinator process to build.

## 8.3 Mobile (iOS) configuration & connection

The ticket asks how mobile devices connect to one or more servers. Directions:

- **Transport:** the shipped mTLS model (§94/§97/§112). The phone holds a
  per-device client cert; it presents it natively on the TLS handshake (browser/
  WKWebView) or via the app. For the **native SwiftUI** client, the cert lives in
  the iOS keychain and the app makes mTLS calls directly (no WebView cert-handling
  problem that the Tauri desktop has).
- **Pairing:** **QR-based enrollment** (already shipped in HS1 — the desktop shows
  a QR; the phone scans it to obtain the origin + enrollment material over a
  tunnel/loopback, then provisions its device cert). This is the recommended
  primary mobile onboarding: point the phone at a Mac's running server by scanning
  a code.
- **Multiple servers:** the phone's remote registry (§8.2) holds several servers;
  each is added by its own pairing. A project picker spans all paired servers.
- **What mobile does:** view/triage tickets, search, answer permission prompts,
  watch busy/progress. Driving AI tools and hosting terminals stay desktop/server
  side ([06](06-clients.md) §6.4).

> **Open decisions.**
> - **O4 — local stores on a phone.** Since clients don't embed the core and iOS
>   can't run an independent background server (§9.1e, §6.4), **iOS is a remote
>   client** — it does not host a local store. (A phone-local store would require an
>   on-device server iOS won't allow.) Recommend **remote-only on iOS**; revisit
>   only if Apple's background-execution story changes. Android, which *can* run a
>   background service, could host a local server later — a separate question.
> - **O5 — push notifications: deferred past v1** (resolved 2026-08-19). A
>   backgrounded-iPhone push (permission waiting / worker finished) needs APNs — new
>   infrastructure (a push relay). In-app + on-sync notifications first; **design the
>   event bus so a push relay can attach later** without rework.
> - **O6 — remote terminals over `wss://`: deferred past v1** (resolved 2026-08-19).
>   Streaming a remote project's PTYs is heavier than data sync (HS1 §112.9 O4
>   deferred it too). Land remote **ticket data** first; a remote project shows
>   tickets before it shows terminals.

## 8.4 Security posture (unchanged, carried over)

- Loopback stays plaintext + shared secret (Tier 0).
- Any off-box bind requires mTLS + per-device certs + ACLs, or refuses to start
  (Tier 1). Per-project CA, `.p12`/QR enrollment, per-connect revocation checks.
  This entire model is shipped in HS1 and is orthogonal to the storage rewrite —
  we carry it forward as-is.

## 8.5 Git-native distributed claim/lease (no central coordinator)

> **Decision (maintainer, 2026-08-19):** make claim/lease work over **git itself**,
> so multiple machines coordinate with no central coordinator. Build: HS2-11;
> de-risking spike: **HS2-63**.

Two regimes, kept distinct:

- **Single shared server** (the common local/team case — everyone talks to one Hot
  Sheet server): the **server is the arbiter**. Claims live in the index + ticket
  frontmatter and are atomic in-process ([05](05-ai-tool-plugins.md) §5.7). No git
  coordination is involved.
- **Multiple independent machines over a shared git remote, no single server:**
  coordination happens **through git**, via the mechanism below.

### 8.5.1 The mechanism — a claim marker + atomic-push compare-and-swap

- **One claim marker per ticket** under a reserved namespace — a git ref/tag named
  `hs-claim/<ulid>`. To claim, a worker **pushes to create that marker**. The remote
  serializes pushes, so the **first push wins and the second is rejected** (the ref
  already exists / not a fast-forward). That rejection *is* a distributed
  **compare-and-swap**: the git remote is the arbiter, and no Hot Sheet coordinator
  process exists.
- **Metadata in the marker's payload, not its name.** The annotated-tag message / a
  tiny object carries `{ worker, expires_at }`. The name stays stable and CAS-able;
  encoding worker/timestamp in the *name* (an early sketch) would break that.
- **Renew** = fast-forward the marker with `--force-with-lease` (itself a CAS: it
  only succeeds if the marker is still what you last saw, so two workers can't both
  renew/steal).
- **Reclaim a dead worker** = once `expires_at` has passed, steal via
  `--force-with-lease` after the expiry check.
- **Cleanup** = the reserved namespace is enumerable, so a periodic sweep deletes
  expired `hs-claim/*` markers. They live outside `main`'s history and the working
  tree, so they never clutter the repo or the merge story.

This makes multi-machine claiming **fully decentralized** — any worker on any box
self-claims against the shared remote — which is why **no central coordinator is
needed** (§8.2 O3).

### 8.5.2 Properties & the one thing to validate

- **Online by nature.** Claiming requires reaching the remote (a push round-trip).
  Fine for multi-machine; the single-server local case never pays this cost.
- **True mutual exclusion.** Unlike a file-based claim (both workers could commit
  locally and only discover the clash on sync), push-CAS rejects the second claimant
  *at claim time* — so two paid AI workers never both start the same ticket.
- **Same trust boundary as editing.** Anyone with push access can claim; that's the
  same access that edits tickets.
- **⚠ Validate per remote (HS2-63 spike).** Some remotes — notably GitHub —
  restrict pushing arbitrary `refs/*` namespaces. The **fallback is tags** (a
  reserved prefix like `hs-claim/<ulid>`), which are universally supported, still
  atomic-on-create, still prefix-namespaced, and bulk-deletable. The mechanism is
  solid; whether the marker is a custom ref or a reserved tag is what the spike
  settles.

## 8.6 Cross-references
- Client tabs/registry: [06-clients.md](06-clients.md)
- Claim/lease execution primitive (single-server regime): [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.7
- Git-native multi-machine claim/lease: §8.5
- **Automatic sync engine** (hands-off team sharing): [02-ticket-storage.md](02-ticket-storage.md) §2.12
- **Human assignment / in-the-loop** across a team/distributed setting:
  [10-assignment-and-collaboration.md](10-assignment-and-collaboration.md)
