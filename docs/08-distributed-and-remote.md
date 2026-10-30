# 08. Distributed Execution & Remote Access

> **Status: Open.** These are the ticket's two named open questions —
> "how projects can be run on distributed systems, orchestrated through a single
> UI" and "how mobile devices (iOS) can/should be configured and connected to one
> or more servers." This doc frames the design directions and the decisions still
> to make; it does not lock them.

## 8.1 Two axes of "distributed"

Keep two separable concerns apart:

1. **Distributed _execution_** — many AI-tool workers draining one project's Up
   Next pool in parallel. Solved at the data layer by the **claim/lease
   primitive** + git-worktree isolation + "workers never merge, the integrator
   merges." This is largely inherited from HS1 and works on the git+index
   foundation (claim state persists to ticket frontmatter, selection runs over the
   index — [05](05-ai-tool-plugins.md) §5.7). Single-machine first.
2. **Distributed _hosting_** — several servers, each owning some projects,
   surfaced together in one client. This is the "orchestrated through a single UI"
   question and is mostly a **client + transport** concern, not a data one.

## 8.2 One UI over many servers (orchestration)

An orchestration _project_ with its own tickets and explicit target-project
membership is specified separately in [26](26-orchestration-projects.md)
(HS2-4JFP0S). It uses the live-mounted projects described here; it does not
change the hosting decisions below.

The model carried from HS1's shipped remote-client work (§112) already fits:

- A client uses the **machine-global registry of remote servers** at
  `${HOTSHEET_HOME}/remotes.json` (HS2-1Y45MZ). It records reviewed server/CA identity,
  origin, label, revision and opaque credential references; bearer secrets and device
  keys stay in the existing machine-secret store. The headless boundary is below;
  a multi-server graphical picker is separate client work.
- Each project tab carries a server `(origin, secret)` — a **local** project's
  origin is this machine's own server (`https?://127.0.0.1:<port>`), a **remote**
  project's is another device's. Every tab is server-backed; there is no
  embedded-core tab kind (§9.1e). The tab machinery is generic over the origin.
- The client shows local and remote projects **side by side**, with per-project
  connectivity state (connected / reconnecting / unreachable) off the WebSocket
  reconnect.

**Orchestration is live-mount only** (maintainer decision, 2026-08-19). A client
reaches another machine's project by **attaching to that machine's running server**
over mTLS (real-time; it drives the AI tools _there_). That is the one path.

> **Decisions (resolved 2026-08-19).**
>
> - **O1 — orchestration path: live-mount only.** _Automatic cloning is **not** a
>   Hot Sheet feature and never will be._ If a user wants a local copy of a store,
>   they **clone it by hand** and run a local server against it — at which point it
>   is simply a **normal local project** (a `127.0.0.1` origin like any other), not a
>   special "clone-and-serve" mode Hot Sheet manages. This keeps the product's job
>   narrow: Hot Sheet mounts _running servers_; plain git handles copies.
> - **O2 — cross-server aggregate views: deferred past v1.** A single view spanning
>   multiple servers (e.g. "everything Up Next everywhere") is a new client-side
>   query-fan-out surface; ship per-server views first.
> - **O3 — cross-machine work: git-native decentralized self-claim, no central
>   coordinator.** See §8.5 — workers on any machine self-claim over the shared git
>   remote via an atomic-push CAS, so there is no coordinator process to build.

### 8.2.1 Headless remote registry foundation (HS2-1Y45MZ)

`hotsheet-cli remote identity` creates/reads this machine's durable random server
identity in `server-identity.json`. It is independent of PID, store instance,
checkout id, path and build revision. `/remote-identity` returns that identity only
after ordinary server authentication. An id obtained over unauthenticated health
or from a label is never sufficient to pair a server.

The machine-local version-1 `remotes.json` contains immutable registration ULIDs,
labels, normalized origins, reviewed server ids and CA fingerprints, revisions,
bounded review history and last-confirmed checkout observations. It contains no
certificate/key/bearer material. A write-ahead cleanup list records opaque secret
references before credential storage, so an interrupted registration cannot leave
an active binding with incomplete credentials. Corrupt or newer registries fail
without overwriting them. Registry changes use a stable file lock and synced
atomic replacement; concurrent reviews require the exact previous revision and
reviewed identity.

The unpublished version-1 format bounds the total of active credential references
and pending cleanup/write-ahead references to 256. Registration and review must
reserve one free obligation before writing a new secret; an exhausted registry
preserves its active bindings and performs no new secret write. Activation moves
the reserved reference to active and the prior active reference to cleanup.
Revocation only transfers an active reference to cleanup, so it remains possible
at capacity even when secret deletion fails. Later successful cleanup restores
capacity. The same validation applies when reading and before every atomic write.

Each binding also persists a private opaque `observation_generation`. Discovery,
explicit review and an idempotent registration capture that generation, binding
revision and credential reference before probing, then compare them under the
registry lock.
A superseded probe returns a conflict without overwriting newer observations or
returning live checkout authority. Concurrent creation of a previously absent id
likewise conflicts rather than reusing the earlier probe. Committed success or
failure observations, explicit review and revoke replace the generation. A fresh
retry after failure can recover normally; equal or backward wall-clock timestamps
do not determine authority. The generation is absent from public DTOs and does not
change the user-visible review revision contract.
Review still requires the exact expected revision and previous identity. A newer
observation can invalidate an in-flight review without changing that public
revision; the operator must deliberately retry, which captures a fresh internal
generation. A concurrent revocation also requires its new public revision before
an explicit review may restore access.
A repeated revoke keeps that public revision but replaces the private generation,
invalidating a pending reactivation; a fresh review at the same public revision
may then proceed.

The operator supplies the expected server id and an explicit CA/device credential
bundle through a private JSON file or stdin, never secret command arguments:

```sh
hotsheet-cli remote register --label "Remote workstation" --origin https://server.example.test --expected-server-id <paired-server-ulid> --credentials-file <private-bundle.json>
hotsheet-cli remote list
hotsheet-cli remote checkouts <registration-ulid>
hotsheet-cli remote review <registration-ulid> --expected-revision <revision> --previous-server-id <old-server-ulid> --previous-ca-fingerprint <old-ca-sha256> --label "Remote workstation" --origin https://server.example.test --expected-server-id <reviewed-server-ulid> --credentials-file <private-bundle.json>
hotsheet-cli remote revoke <registration-ulid> --expected-revision <revision>
```

The input bundle has `bearer_secret`; HTTPS additionally requires `ca_pem`,
`client_certificate_pem` and `client_key_pem` from the existing manual `cert`
enrollment workflow. `--credentials-file -` reads bounded stdin. A repeated
registration with the same origin/configuration/credentials preserves its id and
revision; `--id` permits an explicit idempotent operation. Changed credentials,
origin or identity require review. Registration probes before activating a binding;
failed verification preserves an existing reviewed binding. Secret values are stored
through the existing native-master/encrypted-envelope adapter rooted at the selected
machine home. The registry, returned DTOs, diagnostics and shared checkout files
never contain them.

Off-box origins require HTTPS with normal hostname/chain verification against the
explicit CA and a supplied device certificate/key, in addition to the server bearer.
There is no redirect, insecure TLS fallback or trust-on-first-use. Only literal
loopback IP origins may use HTTP. Remote URL normalization uses the URL parser's
canonical origin, including normal default-port canonicalization; it does not alter
the separate GitHub provider configuration identity contract. Probes bound time,
response size and checkout count. They read authenticated `/remote-identity`,
`/remote-checkouts`, then confirm the identity again. Unsupported legacy peers and
partial/truncated responses cannot become usable targets.

Listing performs no network requests and marks cached checkout observations stale.
Explicit discovery returns live checkouts only on successful authenticated identity
verification. Unreachable, unauthorized, missing-credential, unsupported, changed-id
and trust-failure observations retain last-confirmed identifiers/timestamps but return
no live checkout authority. A changed, untrusted CA is a failed trust check rather
than an authenticated claim about a new peer; accepting it requires explicit review.
Same-CA server-leaf renewal and restart with the same durable identity reconnect
without rebinding. Each discovered checkout carries an available, inaccessible or
identity-changed state and source connection/provider ids, without remote filesystem
paths or credentials.

Local revoke disables a binding before credential cleanup and is safe to repeat.
Failed cleanup remains explicit/retryable, and reactivation requires review. This
does not revoke the remote-issued device certificate: use `hotsheet-cli cert revoke`
on the issuing machine for that operation. Failed/interrupted credential creation is
recovered by repeating the registration/review; cleanup intent is persisted before
writing the secret and never deletes an active reference.

Authenticated `/remotes`, `/remotes/{id}` and `/remotes/{id}/checkouts` expose the
same registry workflow. Administration is loopback-only; an off-box mTLS client,
including a write-capable one, cannot administer this machine's other bindings.
Read-only/denied certificate roles retain their normal restrictions. The serverless,
local-HTTP and generated `--project` MCP `hotsheet_remotes` tool supports identity/
list/register/review/revoke/checkouts. Machine-wide enrollment does not inherit the
project shim's checkout selector. The shim checks its cached literal-loopback origin
without launching a server, and the HTTP adapter rechecks the actual origin before
sending. It imports a local `credentials_file` rather than placing values
in model arguments, and refuses nonlocal backends before reading the file. MCP's
new registry requests refuse redirects; HS2-0BTSX8 separately owns the pre-existing
generic MCP HTTP redirect guard gap.

This foundation supplies reviewed, authenticated identities for HS2-4J6V46 target
bindings and HS2-42B2FD aggregate views. It adds no automatic clone, UI picker,
orchestration manifest, queue fan-out or second claim coordinator.

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
>
> - **O4 — local stores on a phone.** Since clients don't embed the core and iOS
>   can't run an independent background server (§9.1e, §6.4), **iOS is a remote
>   client** — it does not host a local store. (A phone-local store would require an
>   on-device server iOS won't allow.) Recommend **remote-only on iOS**; revisit
>   only if Apple's background-execution story changes. Android, which _can_ run a
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

> **Validated (HS2-63 spike, 2026-08-19)** — see §8.5.3 for the proof and the
> remote-support finding.

- **One claim marker per ticket** under a **reserved ref namespace** —
  `refs/hotsheet/claims/<ulid>`. To claim, a worker **pushes to create that ref**.
  The remote serializes pushes, so the **first push wins and the second is rejected**
  (the ref exists / not a fast-forward). That rejection _is_ a distributed
  **compare-and-swap**: the git remote is the arbiter, and no Hot Sheet coordinator
  process exists.
- **Metadata in the marker's payload, not its name.** The ref points at a tiny
  object (an orphan commit / annotated-tag object) whose message carries
  `{ worker, expires_at }`. The name stays stable and CAS-able; encoding
  worker/timestamp in the _name_ (an early sketch) would break that.
- **Renew** = fast-forward the marker with `--force-with-lease` (itself a CAS: it
  only succeeds if the marker is still what you last saw, so two workers can't both
  renew/steal).
- **Reclaim a dead worker** = once `expires_at` has passed, steal via
  `--force-with-lease=<ref>:<seen-oid>` after the expiry check — so of two
  simultaneous stealers, **exactly one wins** (the other's lease is stale).
- **Enumerate** = `git ls-remote origin 'refs/hotsheet/claims/*'` lists live claims
  without fetching objects.
- **Cleanup** = a periodic sweep deletes expired markers. Custom refs live **outside
  `main`'s history, the working tree, AND normal git surfaces** (`git tag`/`branch`
  and GitHub's UI don't show `refs/hotsheet/*`), so they never clutter anything.

This makes multi-machine claiming **fully decentralized** — any worker on any box
self-claims against the shared remote — which is why **no central coordinator is
needed** (§8.2 O3).

### 8.5.2 Properties

- **Online by nature.** Claiming requires reaching the remote (a push round-trip).
  Fine for multi-machine; the single-server local case never pays this cost. (Claim
  refs aren't fetched by the default refspec, so the protocol explicitly
  `ls-remote`s / fetches the `refs/hotsheet/claims/*` namespace.)
- **True mutual exclusion.** Unlike a file-based claim (both workers could commit
  locally and only discover the clash on sync), push-CAS rejects the second claimant
  _at claim time_ — so two paid AI workers never both start the same ticket.
- **Same trust boundary as editing.** Anyone with push access can claim; that's the
  same access that edits tickets.

### 8.5.3 Spike results (HS2-63, 2026-08-19)

Empirically validated, so this is no longer a bet:

- **Mechanism (generic remote, bare-repo proof):** custom-ref CAS ✅ (2nd claimant
  rejected "fetch first"), tag CAS ✅ ("already exists"), `--force-with-lease` renew
  ✅, **two-stealer race → exactly one wins** ✅ (the stale stealer rejected "stale
  info"), enumerate + sweep ✅.
- **GitHub support (live test against a real repo):** GitHub **accepts custom ref
  namespaces** — `refs/hotsheet/claims/*` pushed, listed, and deleted cleanly. This
  contradicts the older "GitHub only allows heads/tags" lore; current behavior
  accepts them. Tags also work.
- **Decision: custom ref (`refs/hotsheet/claims/<ulid>`) is the primary marker**
  (invisible to normal git + the GitHub UI, bulk-prunable), with **reserved-prefix
  tags as a documented fallback** for any remote that rejects custom refs. The
  CAS/lease semantics are identical either way — a git server-side guarantee.
- **Not stress-tested:** a truly concurrent push race _against GitHub specifically_
  (vs. the local proof) — it's a git protocol guarantee, but worth a load test if we
  ever see contention anomalies.

## 8.6 Cross-references

- Client tabs/registry: [06-clients.md](06-clients.md)
- Claim/lease execution primitive (single-server regime): [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.7
- Git-native multi-machine claim/lease: §8.5
- **Automatic sync engine** (hands-off team sharing): [02-ticket-storage.md](02-ticket-storage.md) §2.12
- **Human assignment / in-the-loop** across a team/distributed setting:
  [10-assignment-and-collaboration.md](10-assignment-and-collaboration.md)
- **Remote testing** of built apps from phones and cloud build VMs (streaming tools,
  region/RTT guidance, launch-link integration): [22-remote-testing.md](22-remote-testing.md)
