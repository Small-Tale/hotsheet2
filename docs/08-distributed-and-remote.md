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
  the desktop picker uses this registry for selected remote projects (HS2-BT4KN6).
- Each project tab carries a server `(origin, secret)` — a **local** project's
  origin is this machine's own server (`https?://127.0.0.1:<port>`), a **remote**
  project's is another device's. Every tab is server-backed; there is no
  embedded-core tab kind (§9.1e). The tab machinery is generic over the origin.
- The client shows local and remote projects **side by side**, with per-project
  connectivity state (connected / reconnecting / unreachable) off the WebSocket
  reconnect. A selected remote project whose host is unreachable keeps its tab as a
  reconnecting placeholder and opens when the host answers (HS2-EJ5K09, HS2-WNK73S;
  `docs/06-clients.md`).

**Orchestration is live-mount only** (maintainer decision, 2026-08-19). A client
reaches another machine's project by **attaching to that machine's running server**
over an explicitly selected HTTP, HTTPS, or mTLS listener (real-time). Remote
ticket data shipped first; remote terminals follow (O6 below, HS2-PPHJMZ).

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
hotsheet-cli remote select-project <registration-ulid> <checkout-id>
hotsheet-cli remote selected-projects
hotsheet-cli remote unselect-project <registration-ulid> <checkout-id>
hotsheet-cli remote review <registration-ulid> --expected-revision <revision> --previous-server-id <old-server-ulid> --previous-ca-fingerprint <old-ca-sha256> --label "Remote workstation" --origin https://server.example.test --expected-server-id <reviewed-server-ulid> --credentials-file <private-bundle.json>
hotsheet-cli remote revoke <registration-ulid> --expected-revision <revision>
```

**Headless pairing (HS2-FZXXTG).** The serving machine writes a private pairing
bundle instead of printing PEMs, and the other machine imports it:

```sh
# On the serving machine (after `cert init --host <name>`; serve with mTLS):
hotsheet-cli cert issue home-orchestrator -C <store> --bundle <file> --origin https://<name>:8787 --role read-write
# On the other machine:
hotsheet-cli remote register --label "Second Mac" --bundle <file>
```

The bundle (`format: "hotsheet-pairing-bundle/v1"`) carries the serving machine's
`server_id`, the device name, an optional suggested `origin` and the CA, device
certificate and key; it is created with mode 0600 and never overwrites a file.
`remote register` and `remote review` read it through the same reader as a plain
credential file (`--bundle` is an alias of `--credentials-file`), as do the desktop's
private CLI call and the `hotsheet_remotes` MCP tool. The bundle's server id is
verified, and an explicit `--expected-server-id` must equal it; an explicit
`--origin` overrides the suggestion, since the importer may reach the server by
another name. Re-importing with the same origin keeps the same registration.

The input bundle keeps `bearer_secret` for stored-format compatibility. It may be
empty or absent only for an HTTPS device identity (certificate and key, no paired
session), which never sends it (HS2-FZXXTG). A
paired session uses `session:<opaque-session-token>` in that field. The
machine's server-to-server calls send it as an `Authorization: Bearer` header,
which the peer accepts for writes without browser Origin and CSRF proof, and
also as the session cookie so a peer that predates bearer sessions still reads;
for such a peer's writes they add the stored Origin and CSRF proof
(HS2-AW2AKF). A paired session keeps its approved
`session_origin` and `session_csrf` in the private secret record for writes and
WebSocket upgrades; neither appears in `remotes.json` or a browser response.
HTTPS requires `ca_pem` to verify the reviewed server;
`client_certificate_pem` and `client_key_pem` optionally select mTLS
together. Legacy mTLS bundles remain readable, but the verified client
certificate, not their old bearer, authorizes off-box requests.
`--credentials-file -` reads bounded stdin. A repeated
registration with the same origin/configuration/credentials preserves its id and
revision; `--id` permits an explicit idempotent operation. Changed credentials,
origin or identity require review. Registration probes before activating a binding;
failed verification preserves an existing reviewed binding. Secret values are stored
through the existing native-master/encrypted-envelope adapter rooted at the selected
machine home. The registry, returned DTOs, diagnostics and shared checkout files
never contain them.

Off-box HTTP origins require a paired session and an explicit HTTP URL. HTTPS
verifies the hostname and chain against the reviewed CA, with either a paired
session or a verified mTLS device certificate. Without a reviewed CA, HTTPS
verifies against the public web roots instead (a Tailscale Serve name or another
publicly trusted certificate) and pins no fingerprint; a reviewed CA is never mixed
with public roots (HS2-H4REBS). There is no redirect, insecure TLS fallback or
trust-on-first-use. A powerful machine bearer is sent only to
a literal loopback HTTP origin. Remote URL normalization uses the URL parser's
canonical origin, including normal default-port canonicalization; it does not alter
the separate GitHub provider configuration identity contract. Probes bound time,
response size and checkout count. They read authenticated `/remote-identity`,
`/remote-checkouts`, then confirm the identity again. Unsupported legacy peers and
partial/truncated responses cannot become usable targets.

**Peer gateway (HS2-H4REBS).** A remote-mode browser host is often reachable only
through an HTTPS proxy that exposes the browser host, not its machine server. That
host therefore passes the machine server's native top-level routes
(`clients/web/src/peer-gateway.ts`, pinned to the router by a policy test), and
their WebSocket upgrades, straight through to its own machine server. It strips
`X-Hotsheet-Secret`, `Authorization` and a `secret` query parameter and never adds
its own bearer, so the machine server authenticates the peer's paired session,
Origin and CSRF exactly as on a direct connection. This grants nothing beyond
`/__hotsheet/project-api/*`, which already forwards any machine path for a browser
session. A pairing peer learns the server id from the `/remote-auth/pair` answer;
`/remote-identity` stays authenticated, and registration refuses a peer whose
authenticated identity differs.

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

HS2-4J6V46 adds a separately versioned, path-free checkout resolution projection
and read-confirm-bind target workflow ([26](26-orchestration-projects.md) §26.4).
Its optional discovery advertisement lets old peers report unsupported metadata
explicitly. Inspect/bind/resolve use existing paired trust and credentials without
exporting them; cached discovery and technical capabilities confer no target grant.
The target adapters are authored source pending verification at this checkpoint.

This foundation supplies reviewed, authenticated identities for HS2-4J6V46 target
bindings and HS2-42B2FD aggregate views. It adds no automatic clone, UI picker,
orchestration manifest, queue fan-out or second claim coordinator.

### 8.2.2 Host-approved browser sessions (HS2-7DBJMS)

The reviewed remote-server registry above belongs to a native client connecting
to another server. Browser device pairing is a separate machine-server
authorization path. A remote browser requests a random 15-minute challenge
under a device name proposed from its user agent and low-entropy client hints
(for example "Safari on iPhone"; HS2-DXM0YN). The host owner verifies its server
id, origin, label and request id, then approves read-only or read-write access
from the trusted local CLI or from App Settings → Devices on the host or an
admin device. A session approves devices only with the **admin** grant; the
separate **local** grant marks the host owner's own browser, which may use
host-only actions such as the folder chooser (HS2-85W6BE). The server enforces
the same grant on its own host-acting routes (difftool, file and attachment
open/reveal/path, and routes that name an arbitrary host path), because the
bridge forwards every project API path (HS2-S8R3PD). Trust never comes
from the hostname, peer address or forwarded headers, because Tailscale Serve
reaches the host through the same loopback port the owner opens locally. See
[04](04-core-server-cli.md) §4.6.1 for the grant rules. A grant changed or
revoked elsewhere reaches an open page through the change channel without a
reload (HS2-A3TDNH). An admin or host page learns about a new pairing request
the same way, and only device managers receive that event (HS2-BZHMEV). The
server issues a revocable,
server-side session and checks it on REST, polling and WebSocket routes;
the browser-facing bridge authenticates before proxying and never passes its
loopback secret for a remote request. The session catalog is machine-scoped,
`${HOTSHEET_HOME}/remote-auth/machine.json`, and bound to the machine's durable server
identity (see "Machine-scoped session catalog" below).
An approved session opens only projects the host has already registered,
through a separate remote open route (HS2-3HC9HT, [06](06-clients.md) §6.2). Its multiplexed
change long-poll (`/__hotsheet/change-mux/poll`) forwards only the browser's session
cookie to each project's upstream poll, so the server applies the session's role and
revocation as for any other project request (HS2-ZVSMTZ).

The browser host must opt in with `HOTSHEET_BROWSER_AUTH_MODE=remote` even when
a proxy reaches it through loopback. Its HTTPS session cookie is Secure,
HttpOnly, host-only and SameSite=Strict; the separate CSRF cookie is readable
by the browser on that same host. Direct HTTP is an explicit
`--remote-transport http` choice; HTTPS uses a server certificate without
requiring a device certificate, and mTLS retains certificate roles. Origin and
CSRF checks protect writes and upgrades. Both the server and the browser bridge
compare a write's `Origin` with the origin the device paired from, never with the
request URL, which a TLS-terminating proxy such as Tailscale Serve rewrites to plain
HTTP (HS2-CD8YNK). Host revocation ends live streams
promptly. The off-loopback default remains mTLS for existing deployments;
HTTP never becomes the default. HS2-747RTD adds
Serve/direct TLS launch, and HS2-WSGZZY owns their cross-transport acceptance.

#### Session lifetime, renewal and reminders (HS2-4B0KSD)

The maintainer set this policy: "30-day idle and 90 days total", with "a very easy way to renew
without having to re-setup connected remotes". One paired session serves both a browser device
and a desktop's remote server binding: the binding stores the session token as `session:<token>`.

- **Idle limit: 30 days.** Any authenticated request, long poll or open stream counts as use.
  `/ws/sync` and terminal attach recheck the session every second through `RemoteAuth::authorize`,
  so a device whose only activity is an open stream stays active. Use is persisted at most once a
  minute per session, which bounds catalog writes.
- **Absolute limit: 90 days** from approval or the last renewal (`renewed_at`). Use never moves it.
  `expires_at` is stored as `renewed_at` plus 90 days, so an older server enforces the same end.
- **Renewal in place.** The trusted host or an admin session renews a device from App Settings →
  Devices (row menu → Renew), with `hotsheet-cli remote sessions renew <device-id>`, or through
  `POST /remote-auth/owner/devices/{id}/renew`. Renewal works before or after expiry.
  - Only `renewed_at`, `expires_at` and the idle clock restart. The device id, token, CSRF proof,
    role, grants, desktop binding and selected projects stay.
  - A device can ask for renewal, even after it expired: `POST /remote-auth/renewal` with its
    paired Origin and CSRF proof. This is the browser's Renew button, the desktop's
    `hotsheet-cli remote renew <binding-id>`, or the remote tab's Renew button. Managers see the
    request as a Renewal Requested row, a Settings badge and a notice.
  - Approval stays with the host, so a device cannot extend its own lifetime.
- **Reminders** fall on days 45, 50, 55 and 60, then every day from 65 to 89, counted from
  `renewed_at`. Day 90 is the expiry itself, where the expired state, with its Renew action,
  replaces the reminder.
  - The host's device list marks a device as Renew Soon, Renewal Requested or Expired.
  - A remote-mode browser, and a desktop tab on a remote server, show a banner naming the browser
    or the server, with Renew and Dismiss. Dismissing a reminder hides it until the next reminder
    day.
  - Reminders are computed from timestamps the client already received: `/remote-auth/me`, or one
    `hotsheet-cli remote session <binding-id>` read when a remote tab opens. A local display timer
    that makes no request re-renders on each reminder day. Nothing polls.
- **Distinct failures.** A session refusal is a 401 with a `code`:
  - `session_expired`: renew it.
  - `session_revoked`: the host revoked it. A revoked session keeps a tombstone digest for one
    lifetime, so it fails as revoked rather than as unknown.
  - `session_invalid`: an unknown credential.

  The browser bridge forwards these codes, and answers 503 `host_unavailable` when the machine
  server does not answer. A restarting host therefore never looks like a lost approval.

- **How each client shows them.** A browser shows a renewal, revocation, pairing or "not
  answering" screen. A desktop remote tab shows:
  - Renew for an expired approval, which is never retried in a loop;
  - Pair Again for a revoked one;
  - the reconnecting backoff for an unreachable host;
  - the error text for any other failure, still retrying.

  The registry stores expired and revoked as `unauthorized`, so an older client can still read
  it. The live discovery's `failure` carries the distinction.

- **Cookie lifetime.** The browser keeps the session cookies for 400 days, the longest lifetime
  browsers honor. The server decides validity, and `/remote-auth/me` refreshes the cookies while
  the device is in use.

What a rebuild, reboot or server restart keeps:

| Credential                                                         | Lifetime                                           | Regenerated on rebuild, reboot or restart?                                                                                                                                  |
| ------------------------------------------------------------------ | -------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Paired session (browser device, or desktop binding via `session:`) | 30 days idle, 90 days absolute, renewable in place | No. The catalog `${HOTSHEET_HOME}/remote-auth/machine.json` is durable and machine-scoped, so a server started on any primary store keeps every paired device (HS2-YDDPN4). |
| Machine server id (`server-identity.json`)                         | Permanent                                          | No                                                                                                                                                                          |
| Instance secret (`X-Hotsheet-Secret` bearer)                       | One server process                                 | Yes, unless `--secret` is passed. It is used only from loopback, never by an off-box binding.                                                                               |
| mTLS project CA                                                    | 10 years                                           | No (`cert init` refuses to replace it)                                                                                                                                      |
| mTLS server leaf                                                   | 397 days                                           | No; it is renewed by hand                                                                                                                                                   |
| mTLS device certificate                                            | 90 days                                            | No; it is rotated by hand with `cert rotate` (follow-up HS2-MSXYJN)                                                                                                         |
| Desktop binding (`remotes.json` plus the private secret store)     | Until removed                                      | No                                                                                                                                                                          |

#### Machine-scoped session catalog (HS2-YDDPN4)

A paired device authorizes this machine's server, not one primary store. The server keeps
every pairing, device and revocation in one catalog, `${HOTSHEET_HOME}/remote-auth/machine.json`,
whichever primary store it was started on.

**When the primary store differs.** The desktop host, `hotsheet-mcp`, `hotsheet-cli` setup and
external launch, and the `server:rebuild*` and `prod:rebuild` scripts all start the machine server
on `${HOTSHEET_HOME}/server-bootstrap.hs2`. Its store id is the hash of the canonical path, so it
stays the same across rebuilds and reboots. The primary store differs only when someone runs
`hotsheet-cli serve` or `hotsheet-server` with `-C` naming another store, for example a
checkout's ticket store or a test store. Before HS2-YDDPN4 the catalog was
`remote-auth/<store-id>.json`, so such a server knew none of the devices the bootstrap server had
approved, and answered `session_invalid`.

**Migration.** The first open of the machine catalog merges every per-store catalog in
`remote-auth/`, under the catalog's lock:

- Devices merge by id. When two catalogs disagree about one device, a revocation on either side
  wins. Otherwise the newest renewal wins and keeps the later idle activity of the two. Each such
  conflict is logged and recorded with the merged source in the machine catalog.
- Unexpired pending pairings carry over. Nothing is pruned, so no valid session is dropped.
- Each merged source is recorded with its SHA-256 digest. Reopening never merges an unchanged
  source again; a source an older server rewrote merges again under the same rules.
- A source that cannot be read or parsed is skipped with a warning and retried at the next
  start. A source bound to another server identity is skipped once, with a warning.
- The per-store files stay in place. An older server still reads its own file, but it does not
  know devices paired after the upgrade.

**Several servers at once.** The writer lock is per store, so servers on different primary
stores may run together and share the catalog. Every write takes `machine.json.lock` and applies
its change to the file's latest content, so neither server loses the other's change. Every read
reloads the file when another process replaced it, so a revocation through one server holds in
the other at its next request. A long poll on the device list wakes on such a reload only when a
request reaches that server; follow-up HS2-Q5FFWN owns waking it on the file change itself.

### 8.2.3 Desktop local and remote projects (HS2-BT4KN6)

The trusted desktop `+` menu asks Local project or Remote project. Local keeps
the folder chooser and auto-start. A phone viewing one remote host keeps its
existing one-host project picker. The desktop remote form takes an explicit
scheme, DNS name or IP, and port. Direct HTTP is used only when entered as HTTP;
HTTPS uses the selected host CA and optionally a client certificate/key. Pairing
asks the host owner for one-time approval, then the Node host redeems the challenge
and saves the session, Origin, CSRF proof and optional TLS material in the
machine-secret store through the same headless registry workflow. The browser
receives only a temporary pairing handle and public server/project metadata.

The machine registry remembers selected `(binding_id, checkout_id)` pairs.
Each tab uses a composite browser id, so equal checkout ids on two servers
remain separate. The trusted bridge translates that id to the reviewed server's
native checkout, applies only that server's private session to REST and change
WebSocket requests, and rechecks the local binding revision before dispatch.
The picker shows server labels and schemes. A remote project tab reads
`<remote host name> · <project name>` (HS2-HGG426): `/remote-identity` adds the
peer machine's optional short `host_name` (the hostname with `.local` removed),
and a live discovery passes it on as `RemoteDiscovery.host_name`. It is display
metadata only: it never enters `remotes.json` or `server-identity.json` and is not
part of trust. When the peer predates the field or does not report a name, the tab
falls back to the binding origin's host (the DNS name or IP the user entered),
never to the label this device gave itself while pairing. Tabs also show
connection failures. Editing a binding requires fresh host approval and explicit registry review;
removing one disables its selected projects and closes that host's active desktop
streams without disturbing another server. A failed discovery has a deliberate
retry path. Remote ticket data is in this scope.

**Remote terminals (HS2-PPHJMZ, HS2-JJKVJJ).** A remote tab's terminals are that
server's PTYs. The bridge rechecks the binding, then upgrades
`/terminals/{id}/attach` on the reviewed server with the same private session,
Origin, CSRF proof and TLS material it uses for the change WebSocket; removing or
changing the binding closes the tab's terminal streams. The remote server applies
its own rules: listing needs any approved session, while creating, writing to,
renaming, killing or attaching a terminal needs a **read-write** session with its
paired Origin and CSRF proof. A read-only approval therefore shows terminals but
cannot open them, and no new grant is added: a terminal is a shell on the remote
host, so the host owner decides by approving read-write access. Revocation ends a
live attach within the server's one-second recheck and releases that viewer's size
claim, so the PTY size recomputes for the viewers that remain (HS2-3EDFG2). A viewer that
reconnected under the same viewer id before the old attach ended keeps its claim (HS2-CCNRVW). The bridge launches a
terminal in the remote checkout's root, never at the composite browser id, and
gives each tab only the terminals launched in its checkout (the most specific open
checkout on that server), so tabs on one server and equal checkout ids on two
servers stay separate. See [06](06-clients.md) §6.2.

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
> - **O6 — remote terminals over `wss://`: shipped for the desktop bridge**
>   (originally deferred past v1 on 2026-08-19; reversed by the maintainer's UX
>   review, HS2-PPHJMZ). Remote ticket data landed first. A desktop remote tab now
>   attaches to its server's PTYs through the paired session (§8.2.3); a phone
>   still drives no terminals of its own (§6.4).

## 8.4 Security posture

- Loopback stays plaintext + shared secret (Tier 0).
- An off-box bind defaults to mTLS with per-device certificates and live
  revocation. An operator may explicitly choose direct HTTP or server-only
  HTTPS with paired-device sessions. The powerful local bearer is refused for
  every accepted off-box peer, even when a forwarded header claims loopback.
  HTTP exposes session cookies to observers of that transport; HTTPS and mTLS
  protect them in transit. Optional `.p12`/QR enrollment remains client work.

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
- **Exact claims use the same markers (HS2-WJF0D8).** A ticket store bound to a
  `git_ref` claim backend takes every exact claim, renewal and release through
  `refs/hotsheet/claims/<ulid>` before writing the frontmatter
  ([36](36-resource-claims.md) §36.14), so this loop and exact claims exclude each other.
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
