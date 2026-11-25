# 36. Resource Claims

A resource claim leases one named resource to one holder for a duration. The holder
renews the claim while work continues and releases it early when done. A claim that is
not renewed lapses by itself, so a stuck or crashed holder never blocks a resource
forever.

Two uses drive the design (decided 2026-11-03, HS2-H3WPPY):

1. **Ticket claims** that show who or what holds a ticket, for any ticket data source.
2. **Non-ticket resources**, for example a local macOS VM that allows one automation
   process at a time.

This document covers the contract, the built-in local machine backend, the CLI, MCP,
and server surfaces, and backend configurations with namespace and data-source binding
(§36.13); the `hotsheet-resource-claims` crate implements them. The built-in Git-ref
backend and the routing of a bound ticket source's claims through it are §36.14; the
other backends are design only, and §36.9 names their tickets. The claim on a Git-backed
ticket ([10-assignment-and-collaboration.md](10-assignment-and-collaboration.md)) goes
through a claim backend when its store is bound to a `git_ref` configuration (§36.14);
otherwise it is unchanged.

## 36.1 Model

- A **namespace** groups resources and is bound to one backend configuration (§36.13);
  an unbound namespace is served by the machine's local backend. A machine-scoped
  namespace is shared by every project on the machine; a project-scoped one belongs to
  one project.
  A namespace name is 1 to 64 characters of `a-z`, `0-9`, `.`, `_`, and `-`, starts with
  a letter or digit, and never contains `..`.
- A **resource** is written `<namespace>/<resource>`. A resource name is 1 to 128
  characters of letters, digits, `.`, `_`, and `-`, and starts with a letter or digit.
- A resource has **at most one live claim**. A pool is several named resources.
- A grant returns:

  | Field           | Meaning                                                                                            |
  | --------------- | -------------------------------------------------------------------------------------------------- |
  | `claim_id`      | A ULID naming this claim.                                                                          |
  | `fencing_token` | An integer greater than every earlier grant of the same resource. It survives restarts.            |
  | `renewal_token` | A bearer capability for this one claim (§36.4). Returned once, at acquire.                         |
  | `expires_at_ms` | Expiry on the backend's clock, as Unix milliseconds; `expires_at` is the same instant in RFC 3339. |
  | `guarantee`     | `exclusive_with_fencing` or `advisory_only` (§36.5).                                               |
  | `metadata`      | A bounded map of encoded strings (§36.3).                                                          |

- **Expiry uses the backend's clock, never the client's.** A claim is live while the
  backend's clock is before `expires_at_ms`, and is over at that instant.
- A lease lasts at most 24 hours per acquire or renew, and one wait lasts at most 24
  hours. Longer work renews; a longer wait waits again.
- **Hot Sheet never renews on behalf of a child process.** The pattern is: acquire, hand
  the renewal token to the task, and let the task renew itself. A stuck task must let
  its lease lapse.

## 36.2 States and transitions

A resource is in one of four states. _Lapsed_ means the lease expired and nobody has
taken or released the resource since; readers already see it as free.

| From          | Event                                             | To       | Result                                                                 |
| ------------- | ------------------------------------------------- | -------- | ---------------------------------------------------------------------- |
| never claimed | acquire                                           | held     | Grant with fencing token 1                                             |
| held          | acquire (try-now)                                 | held     | `held_by`, naming the holder and its metadata; nothing changes         |
| held          | acquire with a wait                               | held     | Blocks until released or lapsed, then grants; `wait_timeout` otherwise |
| held          | renew with its token                              | held     | Expiry becomes backend now plus the duration; fencing token unchanged  |
| held          | release with its token                            | released | Claim ends, the token is revoked, waiters wake                         |
| held          | backend clock reaches expiry                      | lapsed   | Nothing is written; waiters wake by their own timer                    |
| lapsed        | renew with its token                              | lapsed   | `expired`; a lapsed lease is never revived                             |
| lapsed        | release with its token                            | released | `expired`; the slot is cleared and the token revoked                   |
| lapsed        | acquire                                           | held     | Grant with the next fencing token                                      |
| released      | acquire                                           | held     | Grant with the next fencing token                                      |
| released      | renew or release with old token                   | released | `token_invalid`                                                        |
| held (new)    | renew or release with a token of an earlier claim | held     | `not_holder`; the live claim is untouched                              |

A protected resource checks the fencing token it was handed (`verify_fencing`): the live
claim's token passes, an older token is `not_holder`, the latest token after its claim
lapsed or was released is `expired`, and a token never issued is `invalid`.

## 36.3 Metadata

Each claim carries up to 16 entries and 4096 bytes of metadata (keys plus stored
values). A key is 1 to 64 characters of `a-z`, `0-9`, `.`, `_`, and `-`. Backends store
values opaquely. Every stored value starts with a codec prefix, applied by Hot Sheet
before the value reaches the backend:

| Prefix         | Stored form                                                    | Use                                                                                         |
| -------------- | -------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `plain:`       | The text as written                                            | Values that anyone who can list the namespace may read                                      |
| `hmac-sha256:` | Hex HMAC-SHA-256 of the text under the configuration key       | Ids that must be comparable but not readable; a plain hash of a low-entropy id is guessable |
| `enc:v1:`      | Base64url of a 12-byte nonce plus ChaCha20-Poly1305 ciphertext | Labels that a holder of the key reads back                                                  |

A value typed with no prefix is stored `plain:`. A value typed with a prefix is encoded
by that codec: `--meta worker=hmac-sha256:w-1` stores the digest of `w-1`. Ticket-claim
holder fields use `enc:v1:` for the display label and `hmac-sha256:` for worker,
session, and operation ids. Metadata never carries customer-derived content.

One 32-byte key per backend configuration keys both codecs and the renewal-token digest
through separate derivations. The `KeyProvider` trait supplies it. The local machine
backend keeps its key in `<HOTSHEET_HOME>/resource-claims/key`, created on first use
with mode `0600`; a key file that other users can read is refused. Remote
configurations keep their key in the OS credential store under the account the
configuration names (`resource-claims.<id>`), shared between machines by an explicit
export and import (§36.13, HS2-JJWESD).

## 36.4 Renewal tokens

Acquire returns a renewal token: a bearer capability scoped to one claim. It permits
**renew and release of that claim only**. It cannot acquire, and it cannot read or
change any other claim.

- The token names its own claim (`hsrc1.<target>.<claim id>.<secret>`), so a task that
  holds nothing else, such as one inside a VM, can renew or release with the token
  alone.
- The backend stores only a keyed HMAC-SHA-256 digest of the whole token. Changing any
  part of a token, including re-addressing its secret at another claim, fails as
  `token_invalid`.
- Release revokes the token. A token whose claim was replaced by a later grant is
  `not_holder`.
- The token is shown once. Hot Sheet cannot recover a lost token; the lease lapses.

## 36.5 Backend contract

`ClaimBackend` is a blocking Rust trait: `capabilities`, `codecs`, `acquire` (try-now),
`acquire_wait` (with a timeout), `renew`, `release`, `get`, `list` (one namespace),
`wait_free` (wait without acquiring), and `verify_fencing`. `get` and `list` return live
claims only and never include a renewal token.

A backend advertises its **capabilities** before use:

- `guarantee`: `exclusive_with_fencing` when acquire is an atomic compare-and-swap and
  the fencing token is monotonic; otherwise `advisory_only`. An advisory or provisional
  claim is never reported as exclusive.
- `wait`: `push_signal`, `long_poll`, or `none`.
- `max_metadata_entries`, `max_metadata_bytes`, and `max_lease_ms`.

**Waiting.** Acquire fails immediately by default. A wait blocks until a release or an
expiry, through a backend push signal or genuine long polling. No backend or surface
polls on a fixed interval.

**Errors.** Every surface reports the same stable codes:

| Code                  | Meaning                                                                   |
| --------------------- | ------------------------------------------------------------------------- |
| `held_by`             | Another claim holds the resource; the error carries the holder's claim    |
| `wait_timeout`        | The wait ended while another claim still held it; carries the holder      |
| `expired`             | The claim's lease had lapsed                                              |
| `not_holder`          | A later claim has replaced the one the token or fencing token names       |
| `token_invalid`       | The renewal token is malformed, unknown, revoked, or for another claim    |
| `backend_unreachable` | The backend could not be reached; nothing changed                         |
| `unsupported`         | The backend does not offer the operation                                  |
| `invalid`             | The request is malformed (a name, a duration, metadata)                   |
| `capacity`            | A bounded store is full                                                   |
| `corrupt`             | Stored state is unreadable or of an unknown schema; it is not overwritten |
| `storage`             | A local file operation failed                                             |

## 36.6 Local machine backend

`LocalBackend` serves machine-scoped namespaces. It is exclusive with fencing, and its
clock is the machine's clock (injectable in tests).

- **Storage.** `<HOTSHEET_HOME>/resource-claims/namespaces/<namespace>/state.json` holds
  the namespace's resources. Each change runs under the namespace's OS file lock
  (`state.lock`), re-reads the state, and publishes a complete replacement by atomic
  rename, so a crash mid-write leaves the previous state. Directories are `0700` and
  files `0600`. Reads take no lock and never create a namespace.
- **Fencing.** Each resource record keeps the fencing token of its latest grant. The
  record stays after a release, so the counter never goes backwards. A namespace
  remembers at most 4096 resources and refuses a new one with `capacity`.
- **Waiting.** A waiter binds a loopback datagram socket and publishes its port in the
  namespace's `waiters` directory before it inspects state. A release, or a renewal
  that shortens a lease, publishes the new state and then sends one datagram to each
  registered waiter. Either the waiter's inspection sees the freed state or the
  releaser sees the waiter, so no wake-up is lost. Between signals a waiter sleeps once,
  until the earlier of the holder's known expiry and its own deadline. An OS timer
  that returns before that instant (Windows ARM64's socket timeout did) waits out the
  remainder instead of re-reading state, so only a datagram or the planned instant
  costs a read (HS2-H9DNG6). A datagram carries no information: a woken waiter re-reads the state, so a stray datagram costs
  one read. A namespace accepts at most 256 waiters at a time, and a registration left
  by a crashed waiter is removed a minute after that waiter's own deadline.
- **Crash with a held lease.** The lease stays until its expiry and then lapses. If the
  crashed task returns with its old token after someone else acquired, it is
  `not_holder`.

**A stopped wait cleans up (HS2-QGY1NN).** On unix, `hotsheet-cli resource acquire --wait` and `resource
wait` hold an `InterruptGuard` while they wait: SIGINT, SIGTERM and SIGHUP write a byte to a self-pipe
that the waiter's `park` polls beside its socket, so a signal ends the wait at once and without a race
(a signal that lands before the poll leaves its byte waiting). The wait fails `interrupted` (HTTP 503 if
a surface ever served it), the waiter drops and removes its registration, and the guard then restores
the default disposition and re-raises the signal, so the process still dies by the signal it was sent. A
stopped wait therefore no longer counts against the namespace's 256 registrations until its deadline
and the 60 s grace. A SIGKILL cannot be caught and still leaves its registration until then. Outside a
guard signals keep their default action. Windows has no guard yet (HS2-995FNQ covers Windows evidence).

## 36.7 Offline behavior

What happens when a remote backend is unreachable is a per-configuration choice: fail
closed (the default, `backend_unreachable`) or provisional advisory local claims that
are reconciled and conflict-flagged on reconnect. A provisional claim is never reported
as exclusive. The local machine backend has no remote and is never offline, so a local
configuration refuses `provisional`. The choice is recorded in the configuration
(§36.13, HS2-JJWESD) and read through `hotsheet_resource_claims::offline_policy(home,
project, namespace)`, or the `offline` field of `resolve`. The orchestration
scheduler acts on either choice, per namespace, as the machine's capacity read
reports it (§26.20 of [26-orchestration-projects.md](26-orchestration-projects.md),
HS2-KCEHVR), with the hosted checkout's project bindings winning over the machine's
as they do for the sessions there (HS2-AP8KPT).

## 36.8 Plugin form

Backends are implementations of the Rust trait. The built-ins are the local machine
file backend, the Git-ref compare-and-swap (§36.14, HS2-WJF0D8) and Upstash Redis REST
(§36.17, HS2-K9J1RK). External backends use the subprocess protocol of §36.16
(HS2-322ZSZ), modeled on notifier plugins ([32-notifier-plugins.md](32-notifier-plugins.md)
§32.6). Every setup and recovery path has an idempotent `hotsheet-cli` path shared with the
graphical client.

## 36.9 Status

| Part                                                                                             | Status                                                                                      | Ticket     |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------- | ---------- |
| Backend contract, error model, codecs, local machine backend                                     | Shipped                                                                                     | HS2-H3WPPY |
| CLI, MCP, and server surfaces with token-only renew and release (§36.10 to §36.12)               | Shipped for machine-scoped namespaces on the local backend                                  | HS2-FMHH9Y |
| Backend configurations, namespace and data-source binding, offline policy, credential-store keys | Shipped in the CLI and library (§36.13); server and web settings are HS2-JJWESD's follow-up | HS2-JJWESD |
| Git-ref backend; exact ticket claims of a bound Git store through it                             | Shipped (§36.14)                                                                            | HS2-WJF0D8 |
| AI guidance for the acquire-and-hand-off pattern                                                 | Design only                                                                                 | HS2-SHDW2Q |

The first consumer outside tickets is the machine-wide `browser-engines` pool that caps local
Playwright WebKit browsers (HS2-M6TZB5,
[12](12-code-organization-and-testing.md)): one resource per slot, acquired with a wait,
renewed by the browser's own process, and released when the browser closes.

`hotsheet_resource_claims::resolve_backend(home, namespace)` is the machine-scoped
lookup the server and MCP use; `resolve(home, project, namespace)` also consults a
project's registry, which the CLI does with `--project`.

## 36.10 Surfaces

The CLI, the MCP tool, and the server routes are thin adapters over one workflow,
`hotsheet_resource_claims::ClaimService`, so they return the same JSON. The CLI and the
serverless MCP shim need no running server.

| Operation                 | CLI                                                                                   | MCP `hotsheet_resource_claim`                                 | Server route                                                   |
| ------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------- | -------------------------------------------------------------- |
| Acquire                   | `hotsheet-cli resource acquire <ns>/<res> --duration 30m [--wait 5m] [--meta k=v]...` | `action: "acquire"`, `target`, `duration`, `wait`, `metadata` | `POST /resource-claims/{namespace}/{resource}/acquire`         |
| Renew                     | `hotsheet-cli resource renew [<ns>/<res>] --token <token> --duration 30m`             | `action: "renew"`, `renewal_token`, `duration`                | `POST /resource-claims/{namespace}/{resource}/renew`           |
| Release                   | `hotsheet-cli resource release [<ns>/<res>] --token <token>`                          | `action: "release"`, `renewal_token`                          | `POST /resource-claims/{namespace}/{resource}/release`         |
| Show                      | `hotsheet-cli resource show <ns>/<res> [--wait 5m]`                                   | `action: "show"`, `target`, `wait`                            | `GET /resource-claims/{namespace}/{resource}`; `POST .../wait` |
| List a namespace          | `hotsheet-cli resource list <namespace>`                                              | `action: "list"`, `namespace`                                 | `GET /resource-claims/{namespace}`                             |
| Renew or release by token | The renew and release commands above with no target                                   | The renew and release actions above                           | `POST /resource-claim-token/renew` and `/release` (§36.11)     |

- **Durations** are written `500ms`, `90s`, `30m`, `1h30m`, or `2d` in the CLI and MCP
  tool, and as `duration_ms` and `wait_ms` in a request body.
- **Acquire** answers with the claim fields of §36.1 plus `acquired: true`. When the
  resource is held, the answer is `{ "acquired": false, "code": "held_by", "error",
"holder" }`, or `code: "wait_timeout"` after a wait. This is an ordinary answer (HTTP
  200 and a successful MCP result). The CLI prints it on standard output and exits
  non-zero, so a script can read the holder and a shell can branch.
- **Renew and release** need only the renewal token. The CLI reads it from `--token`
  or, when that is absent, from `HOTSHEET_RESOURCE_CLAIM_TOKEN`, so a task need not put
  it on a command line. An optional target is a cross-check: a token for any other claim
  is `token_invalid`. Renew answers `{ "claim" }`; release answers `{ "released": true,
"claim" }`.
- **Show** answers `{ "target", "held", "claim", "capabilities" }` and **list** answers
  `{ "namespace", "capabilities", "claims" }`. Neither ever includes a renewal token.
  `show --wait` waits for the resource to become free without claiming it.
- **Metadata** values are encoded as §36.3 describes before they are stored.
- **Errors** carry the codes of §36.5. HTTP statuses: `invalid` 400, `token_invalid` 403,
  `not_holder` 409, `expired` 410, `unsupported` 501, `backend_unreachable` 503,
  `capacity` 507, `corrupt` and `storage` 500. The error text starts with the code.
- **Actors.** The MCP tool accepts `actor_role` and `actor_id` like every mutating tool,
  but a claim records its holder only through metadata.

**Waiting over the server.** `acquire` with `wait_ms` and the `wait` route are genuine
long polls: one request parks on the backend's release signal and answers when the
resource changes hands, when the holder's lease lapses, or when the wait ends. The
server parks one request for at most 55 seconds; a client with a later deadline asks
again as soon as the previous answer arrives, and sends nothing in between. The MCP tool
does this for a `wait` of up to 10 minutes per call. Only a request the server held for
its whole capped wait is followed by another; the answer to the request that carried the
rest of the wait is final, even when the server's clock ends it a millisecond before the
caller's own deadline (HS2-MP402X). At most 64 requests park at a time;
another is refused with 503 `capacity`. A parked request does not hold an API request
slot. If the client disconnects while parked and the request then wins the resource,
the server releases that grant at once, because nobody received its token. A parked
`acquire` or `wait` is an in-flight mutation, so a safe server restart waits for it, for
at most those 55 seconds.

## 36.11 The renewal-token route and guest VMs

`POST /resource-claim-token/renew` and `POST /resource-claim-token/release` accept
`Authorization: Bearer <renewal token>` as the **only** credential. Renew takes
`{ "duration_ms" }`; release takes no body. This is how a task that has no server secret,
for example one inside a guest VM, keeps or ends its own lease.

- The token is read only from the header. The route renews or releases that token's own
  claim and does nothing else: any other action is 501, and it cannot acquire, list, or
  read another claim.
- A renewal token is not a credential anywhere else. Every other route, including the
  `/resource-claims/...` routes, still requires the server's ordinary authentication,
  and the server secret is not accepted in place of a token here.
- A missing or malformed header is 401; a well-formed token that does not match is 403.

**Bind and exposure.** The token route is served by the same listener as every other
route; there is no second port. By default the server binds `127.0.0.1`, which a guest
cannot reach, so the route is loopback-only until the operator chooses otherwise:

| Server start                                                 | Guest reaches the token route? | Notes                                                                                         |
| ------------------------------------------------------------ | ------------------------------ | --------------------------------------------------------------------------------------------- |
| Default (`--bind 127.0.0.1:<port>`)                          | No                             | Use the CLI on the host instead.                                                              |
| `--bind <host-only address>:<port> --remote-transport https` | Yes, with the token alone      | Recommended. The guest trusts the server certificate from `hotsheet-cli cert init`.           |
| `--bind <host-only address>:<port> --remote-transport http`  | Yes, with the token alone      | Only on a host-only network that no other machine can reach; the token crosses in clear text. |
| `--bind <address>:<port>` with the default `mtls` transport  | Only with a client certificate | The TLS handshake requires an enrolled client certificate before any route is reached.        |

Bind the address of the hypervisor's host-only or shared network interface, not a
wildcard, so the listener is not exposed on other networks. An off-loopback listener
already refuses the server secret from a non-loopback peer, so exposing the listener
for the token route does not let a guest use the secret-authenticated routes. Inside
the guest, a task needs only an HTTP client:

```sh
curl -fsS -X POST "https://<host-address>:<port>/resource-claim-token/renew" \
  -H "Authorization: Bearer $HOTSHEET_RESOURCE_CLAIM_TOKEN" \
  -H "Content-Type: application/json" -d '{"duration_ms": 600000}'
```

**Failed attempts are throttled per peer (HS2-JFEMAR).** The token route is outside server
authentication, and each attempt took the namespace lock and read a state file on the async
runtime. Measured on loopback in a debug build, a wrong token cost 3.4 ms; one flooding peer slowed a
valid renewal from 9 ms to 15 ms (p99 43 ms), and eight parallel flooders held the route to about 80
attempts a second and slowed it to 110 ms at the median, so a flood also held up unrelated requests.
The server now keeps a failure budget per peer address: 30 failed attempts per 10-second window. A
peer over budget is answered `429 rate_limited` with `Retry-After` and `retry_after_seconds` before
its token is even read, so the work one peer can cause is bounded, and checking a token runs on the
blocking pool. A valid token never counts against the budget and a peer that has not been failing is
never delayed, so a guest keeping its lease is unaffected by another peer's flood; a peer that spent
its own budget waits out the window (its valid token included), which is short next to a lease.
A token that matched but whose lease lapsed (`expired`) proved itself and does not count; a missing
or malformed bearer, a token naming no claim and one naming another claim do. Up to 1024 peers are
remembered, the oldest window dropped first. `AppState::with_token_failure_budget` changes it.

**From the CLI (HS2-7H1Y4E).** A guest with `hotsheet-cli` need not hand-write the request:

```sh
hotsheet-cli resource renew   --server https://<host-address>:<port> --duration 10m
hotsheet-cli resource release --server https://<host-address>:<port>
```

The token comes from `--token` or `$HOTSHEET_RESOURCE_CLAIM_TOKEN`, and an optional
`<namespace>/<resource>` argument must name the claim the token is for (checked before any
request). `--server` calls the token route with the token as the bearer and nothing else: no
`HOTSHEET_HOME`, no server secret, which is never read or sent on this path. The URL is
`https://host[:port]` with no credentials, path, query or fragment. A certificate is checked against
the public web PKI, or against exactly the authority given with `--ca-cert <PEM>` (the CA that
`hotsheet-cli cert init` writes; any other issuer is refused and the error names `--ca-cert`).
Plain `http://` is accepted only to a loopback address; `--allow-insecure-http` allows it to another
host for a host-only network no other machine can reach, since the token then crosses in clear text.
The reply is printed as JSON, a refusal prints its code (`token_invalid`, `expired`, `not_holder`) and
fails the command, and the token never appears in output or an error.

**AI guidance (HS2-SHDW2Q).** The shared Hot Sheet skill (version 75, installed by `hotsheet-cli setup`
for every tool that has one) has a "Resource claims" section that teaches the pattern of this document:
acquire for a duration sized to the work, wait with `--wait` instead of a retry loop, hand the renewal
token to the task through its environment only, have the task renew itself (through `--server` from a
guest with no shared home), use the fencing token and read `guarantee`, release early, and never renew
on a stuck task's behalf. It also says how ticket claims behave when a store is bound to a Git-ref
configuration (a refusal names the holder, `claim_backend_unreachable` means nothing was claimed, a
provisional claim is advisory, a missing configuration key is imported with `resource backend key`).

## 36.12 Rust API for other crates

A crate that schedules work, such as orchestration, treats a held resource as
unavailable capacity through `hotsheet_resource_claims`:

- `held(home, &ResourceRef) -> Result<Option<Claim>, ClaimError>` reports the live claim,
  with its `expires_at_ms`, without acquiring.
- `wait_until_free(home, &ResourceRef, timeout) -> Result<WaitOutcome, ClaimError>` blocks
  until the resource is free or the timeout passes, woken by the backend signal.
- `ClaimService` offers typed `acquire`, `renew`, `release`, `show`, `wait_free`, and
  `list`, and `resolve_backend` returns the `ClaimBackend` for a namespace.

All of these block; an async caller runs them on a blocking thread.

Orchestration uses them this way
([26-orchestration-projects.md](26-orchestration-projects.md) §26.20, HS2-2FG91X): a
plan step or a target declares the resources its work must hold, the scheduler
lists a namespace's live claims and defers work whose resource is held, and it waits
for a release with `ClaimService::wait_free`. HS2-AP8KPT: for a hosted target checkout
both go through `ClaimService::new(home).with_project(checkout root)`, so a namespace
the project binds itself is read, waited on and given its offline policy from the
project's backend. It never acquires, renews or releases; the session that does the
work does, as §36.1 prescribes.

## 36.13 Backend configurations and bindings (HS2-JJWESD)

A **configuration** names one claim backend. Namespaces and ticket data sources are
**bound** to configurations; several may share one, and none has to.

| Field        | Meaning                                                                                                                      |
| ------------ | ---------------------------------------------------------------------------------------------------------------------------- |
| `id`         | 1 to 64 characters of `a-z`, `0-9`, `.`, `_`, `-`                                                                            |
| `kind`       | `local` (§36.6) or `git_ref` (the Git-ref compare-and-swap, §36.14)                                                          |
| `settings`   | Non-secret settings from a per-kind allow-list: none for `local`; `remote` (default `origin`) and `repository` for `git_ref` |
| `credential` | The OS credential-store account that holds the configuration's 32-byte key (`resource-claims.<id>`), never a value           |
| `offline`    | `fail_closed` (default) or `provisional` (§36.7); `local` takes only `fail_closed`                                           |

A setting outside the allow-list is refused, so a token typed as a setting never reaches
a file. Output never contains key material except `backend key --export`, which prints
the key only when asked.

**Scopes.** Each scope is one version-guarded file (`$hotsheetSchema` 1). A newer or
unreadable file is refused and left untouched.

| Scope   | File                                                           | Shared with                                                                                                                |
| ------- | -------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Machine | `<HOTSHEET_HOME>/resource-claims/configurations.json` (`0600`) | Every project on this machine                                                                                              |
| Project | `<project>/.hotsheet2/resource-claims.json`, checked in        | Everyone with the project. A Git ticket store is a project, and the CLI commits the file there, so every clone receives it |

A project binding wins over a machine binding and may name a configuration from either
scope. A binding to a configuration defined in neither is an error, not "unbound".

**Namespaces.** A namespace bound in a project is **project-scoped**: a `local`
configuration keeps its state under
`<HOTSHEET_HOME>/resource-claims/projects/<project hash>/`, so the same namespace name
in two projects, or in a project and on the machine, never shares a claim.

**Ticket data sources.** A source is a connection id, or `store:<instanceId>` for a Git
ticket store. A Git store's connection id is derived from its local path and differs
between clones, so bind the store identity (`--store <path>`) to cover every clone.
A Git store bound to a `git_ref` configuration routes its ticket claims through it
(§36.14); a `local` binding changes nothing for ticket claims.

**Commands.** All are idempotent, print JSON (`change` is `created`, `updated`,
`unchanged`, `removed` or `absent`), and take `--project <dir>` to use the project
scope.

| Command                                                                                                                               | Effect                                                                                                                                             |
| ------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `hotsheet-cli resource backend add <id> --kind local\|git-ref [--setting k=v]... [--credential] [--offline fail-closed\|provisional]` | Add or update a configuration; with `--credential`, print the key next steps                                                                       |
| `hotsheet-cli resource backend list` / `show <id>`                                                                                    | Configurations and what is bound to each                                                                                                           |
| `hotsheet-cli resource backend test <id>`                                                                                             | One real cheap call (a namespace listing, or `git ls-remote` of the claim refs) and a key check; exits non-zero with the next steps when not ready |
| `hotsheet-cli resource backend remove <id>`                                                                                           | Remove; refused (`in_use`) while anything in that scope is bound to it                                                                             |
| `hotsheet-cli resource backend key <id> --generate\|--import\|--export`                                                               | Create the key, store one exported from another machine (standard input), or print it                                                              |
| `hotsheet-cli resource namespace bind <ns> --backend <id>` / `unbind <ns>`                                                            | Serve a namespace through a configuration, or return it to the machine's local backend                                                             |
| `hotsheet-cli resource source bind <source>\|--store <path> --backend <id>\|--like <source>` / `unbind`                               | Bind a ticket data source; `--like` reuses another source's configuration                                                                          |

`acquire`, `renew`, `release`, `show` and `list` take `--project <dir>` too, and `show`
and `list` report the namespace's `configuration` and `offline` policy.

**Rust API.** `hotsheet_ticketing::claim_backends` holds the registry (`Scope`,
`ClaimRegistry`, `Registries::namespace` and `::source`, `git_store_sources`).
`hotsheet_resource_claims` adds `resolve(home, project, namespace) -> ResolvedBackend`
(`backend`, `configuration`, `binding_scope`, `offline`), `offline_policy(home,
project, namespace)`, `CredentialKeyProvider` over any `SecretStore`, `generate_key`,
`import_key`, `export_key`, and `test_configuration`. The server and web settings call
these functions (HS2-JJWESD follow-up); tests inject an in-memory `SecretStore` and run
with `HOTSHEET_NATIVE_SECRETS=deny`.

## 36.14 The Git-ref backend and ticket claims across machines (HS2-WJF0D8)

The `git_ref` backend keeps each claim as a marker ref on a Git remote that every
machine shares, and lets the remote serialize every change. It is how one ticket store
cloned on several paired machines guarantees that two machines never hold one ticket
(HS2-ZKJX16).

**Markers.** A marker is a ref pointing at an orphan commit whose message is the claim
as JSON: `worker`, `expires_at`, `claimed_at` (the fields of the `distclaim` markers of
[08](08-distributed-and-remote.md) §8.5, so the distributed work loop and exact claims
exclude each other), plus `claim_id`, `fencing`, `acquired_at_ms`, `expires_at_ms`,
`host`, and for a resource claim the SHA-256 of its renewal token and its metadata.

| Claim                | Ref                                                    |
| -------------------- | ------------------------------------------------------ |
| A ticket             | `refs/hotsheet/claims/<ticket ULID>`                   |
| A namespace resource | `refs/hotsheet/resource-claims/<namespace>/<resource>` |

**Operations** (`hotsheet_ticketing::gitref_cas`):

- **Acquire.** With no marker, a plain push creates the ref and only the first push
  wins. A lapsed marker is replaced by a push with `--force-with-lease=<ref>:<seen>`,
  so of two takers one wins. A live marker of another claim refuses. A live marker of
  the same claim is an idempotent retry that keeps its fencing token.
- **Renew** rewrites the claim's own marker under the same lease check.
- **Release** deletes the marker under the same lease check, so it never removes a
  claim that replaced the one being released.
- **Fencing** is `max(previous + 1, acquire time in ms)`, which keeps increasing across
  a deleted marker while clocks move forward.
- **Clock.** Git has no clock of its own, so expiry uses the claimer's clock. Machines
  sharing a remote need roughly synchronized clocks.
- **Waiting.** The remote cannot signal a release, so the backend advertises
  `wait: none`: a try-now acquire works and a wait answers `unsupported` (a genuine wait is HS2-CR4STA).

**Configuration.** `remote` (default `origin`) and `repository`. For a ticket store the
repository is the store itself. For a project-scoped namespace it is the project
directory; a machine-scoped namespace needs `repository`. No secret is involved beyond
the remote's own Git authentication. A configuration with `--credential` keys `hmac` and
`enc` metadata from the OS credential store; without one only `plain:` metadata is
accepted.

**Ticket claims of a bound store.** When a Git ticket store's source (normally
`store:<instanceId>`, committed in the store's `.hotsheet2/resource-claims.json`) is
bound to a `git_ref` configuration, every claim path in `ops` goes through the backend
before the ticket's frontmatter changes: `hotsheet-cli claim`, the MCP claim tools, the
hosted `/checkouts/{checkout}/target-claims` route and orchestration runs, coordination
assignments, workers, and claim-next (which skips a ticket another machine holds and
takes the next one).

| Event                                   | Effect                                                                                                                                                 |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Claim, marker free or lapsed            | The marker is pushed for the new claim attempt (the claim event's id), then the frontmatter is written                                                 |
| Claim, another machine holds it         | Refused as `WrongWorker` naming the holder; the local ticket is repaired to name the holder with the marker's claim id                                 |
| Renew                                   | The attempt's marker is extended; a missing marker (provisional, or a claim from before the binding) is acquired now                                   |
| Renew after losing the ticket           | Refused naming the holder, and the local ticket is repaired. A worker whose own claim a merge overwrote in the frontmatter is restored from the marker |
| Release, completion, any terminal state | The write that ends the claim deletes the attempt's marker (best effort; a marker left behind lapses at its lease)                                     |
| Sync that pulled                        | Every live claim this host holds is checked against its marker; one another machine holds is repaired and recorded as a conflict                       |

An unbound store, or one bound to a `local` configuration, behaves exactly as before.

**Offline.** With `fail_closed`, a claim, claim-next or renew while the remote cannot be
reached answers `claim_backend_unreachable` (HTTP 503) and nothing is written. With
`provisional`, the claim is written locally and recorded as provisional in the clone's
Git directory (`hotsheet-provisional-claims.json`); `hotsheet-cli claim` says it is
advisory, and `claim_gate::provisional_claims` lists such claims. The next renew or sync
that reaches the remote confirms it, or, when another machine took the ticket meanwhile,
refuses it naming that holder. A release always succeeds locally.

**Conflicts.** A claim this host lost is appended to the clone's
`hotsheet-claim-conflicts.json` (at most 256 entries: ticket, slug, the local worker,
the holder and the time), which `claim_gate::take_conflicts` returns once. The losing
session's next renew is refused naming the holder. Pushing a conflict to the session as
a notification is HS2-1639Q0.

**Namespace resources.** A namespace bound to a `git_ref` configuration is served by
`GitRefBackend`: exclusive with fencing; renew and release by token alone from any
machine sharing the remote; `provisional` grants come from a local fallback under
`<HOTSHEET_HOME>/resource-claims/provisional/<configuration>/` and report
`advisory_only`; reconciling them when the remote returns is HS2-1SXBK9.

**Waiting is not offered by the Git-ref backend (HS2-CR4STA).** A Git remote cannot signal a release, and
a wait must not poll on a timer, so a namespace served by a `git_ref` configuration answers a wait
with a non-zero timeout `unsupported`, naming the way out: bind the namespace to a backend whose wait is
a push signal (the local backend for machine-scoped resources, Upstash Redis §36.17, or an external
plugin with a blocking wait §36.16). A zero-timeout wait is an ordinary try-now and still names the
holder. The designs considered and rejected for now: a wake from the sync loop would still depend on
the remote advancing a branch (claims live in `refs/hotsheet/claims/*`, which no push hint announces);
long-polling a Hot Sheet server that hosts the remote would make that server a required claim service
for a backend whose point is that Git alone suffices.

**Telling the losing session (HS2-1639Q0).** After every sync pass that reaches the remote, the
server reads the conflicts `reconcile` recorded (`claim_gate::pending_conflicts`) and pushes each into
the live terminal whose claim worker id is the loser, through the session-push path every
server-initiated prompt uses: "your claim on `<slug>` was lost; another machine holds it now
(`<holder>`); stop work on it". The holder is the readable label (or the unreadable marker of §36.15),
never a worker id or a key. A delivered conflict is acknowledged, so a session is told once; one whose
session is not reachable, or whose push failed, stays pending (the record keeps its last 256) and is
tried again after the next pass. Showing lost and provisional claims in the web client and on the wire
is HS2-ZR8B4N.

## 36.15 Holder fields and per-configuration keys (HS2-EWGPTZ)

The codecs of §36.3 live in `hotsheet_ticketing::claim_codec` (re-exported by
`hotsheet-resource-claims`), because ticket claims on a Git-ref backend use them too.

A configuration that names a credential keeps a 32-byte key in the OS credential store.
`hotsheet-cli resource backend add <id> --credential` generates it when the configuration
is created on this machine (`key.status: ready`), keeps an existing key when the same add
repeats (`kept`), and skips it with `--no-key` for a machine that will import the key
another machine exported (`skipped`). An unavailable credential store never fails the add:
the answer says `unavailable` and names the commands that finish it. `resource backend key
<id> --export` and `--import` move the key between machines headlessly.

A ticket claim taken through a keyed `git_ref` configuration never writes a plain id to
the backend:

| Marker field     | Value                                                                                         |
| ---------------- | --------------------------------------------------------------------------------------------- |
| `worker`         | `hmac-sha256:` digest of the worker id: comparable by anyone with the key, readable by no one |
| `metadata.label` | `enc:v1:` ciphertext of the claim's display label (the worker label, else the worker id)      |

- **Ownership** compares by digest (`TicketGate::owns`), so the holder's retries, renewals
  and releases keep matching its own marker. A plain `worker` (a claim taken before the key
  existed, or by the distributed work loop) still compares plain.
- **Display** decrypts the label with the key. A machine without the key, or with another
  key, shows `(holder unreadable: configuration key not available)`, and cannot tell its own
  claim from another's until the key is imported.
- **Missing key.** A configuration that names a credential whose key is absent refuses every
  claim before anything reaches the backend, naming `--generate` on the first machine and
  `--export` / `--import` on the others. A configuration with no credential keeps plain
  holder fields exactly as before.
- **Rotation shape.** The `enc:v1:` prefix versions the ciphertext, and the key is not
  stored with the claim. Replacing the key (`--import` of a new key everywhere) makes
  existing markers unreadable and foreign; they keep excluding other claimants until their
  lease lapses, and the next claim is written under the new key. There is no in-place
  re-encryption of live claims.

## 36.16 External claim-backend plugins (HS2-322ZSZ)

A third party adds a claim backend without changing Hot Sheet. The plugin is a directory
`<HOTSHEET_HOME>/claim-backends/<id>/` whose name equals its manifest `id`:

```toml
id = "vault"
display_name = "Vault claims"
command = ["claim.sh"]        # no shell; a relative file in this directory runs from here
timeout_seconds = 30          # per operation, at most 300

[capabilities]
guarantee = "exclusive_with_fencing"   # or "advisory_only"
wait = "blocking"                      # or "none" (the default)
max_metadata_entries = 8               # optional; Hot Sheet's own bounds cap these
max_lease_seconds = 3600

[[settings]]
key = "endpoint"
label = "Endpoint"
type = "string"                        # string | secret | choice
required = true

[[settings]]
key = "api_token"
label = "API token"
type = "secret"
```

**The plugin is the backend.** It owns atomic acquire (compare-and-swap), the fencing
counter and the backend clock, exactly as §36.5 requires of any backend. A plugin that
cannot guarantee that declares `advisory_only`. Hot Sheet owns what a plugin must never see:
the renewal token, the metadata key, and the approval of the plugin's own code.

**One process per operation.** Hot Sheet runs the command, writes one JSON envelope to stdin
and reads the last stdout line that is a JSON result; earlier lines are ignored, so logging is
safe. A plugin that outlives its timeout is killed. Exiting without a result line, or
reporting `ok` and then exiting non-zero, is `backend_unreachable` with the stderr tail.

```json
{
  "protocol": 1,
  "backend": "vault",
  "configuration": { "id": "team" },
  "op": "acquire",
  "settings": { "endpoint": "…", "api_token": "<resolved>" },
  "request": {
    "target": "vms/mac-1",
    "claim_id": "<ulid>",
    "token_digest": "<sha256 hex>",
    "duration_ms": 60000,
    "metadata": { "owner": "plain:build" }
  }
}
```

| `op`             | `request`                                                       | `result` on `ok: true`                       |
| ---------------- | --------------------------------------------------------------- | -------------------------------------------- |
| `acquire`        | `target`, `claim_id`, `token_digest`, `duration_ms`, `metadata` | `claim`                                      |
| `renew`          | `target`, `claim_id`, `token_digest`, `duration_ms`             | `claim`                                      |
| `release`        | `target`, `claim_id`, `token_digest`                            | `claim` (the ended one)                      |
| `get`            | `target`                                                        | `claim` or `null`                            |
| `list`           | `namespace`                                                     | `claims`: each a claim plus `resource`       |
| `namespaces`     | none                                                            | `namespaces`                                 |
| `wait`           | `target`, `timeout_ms`                                          | `free: true`, or `free: false` with `holder` |
| `verify_fencing` | `target`, `fencing_token`                                       | empty                                        |

A `claim` is `{claim_id, fencing_token, acquired_at_ms, expires_at_ms, metadata}`. The plugin
returns it from the claim's own clock; `get` and `list` return live claims only. A failure is
`{"ok": false, "error": {"code", "message", "holder"?}}` with `code` one of `held_by` (with
`holder`), `expired`, `not_holder`, `token_invalid`, `backend_unreachable`, `unsupported`,
`invalid`, `capacity`; any other code is a storage error. Hot Sheet validates what comes back:
the claim id must be the one requested, an exclusive plugin's fencing token must be above 0,
expiry cannot precede acquisition, and metadata keeps its codec prefixes and bounds.

- **Tokens.** Hot Sheet mints the renewal token (§36.4) and sends only its SHA-256 digest. The
  plugin stores the digest with the claim and compares it on `renew` and `release`; the token
  itself never crosses.
- **Waiting.** `wait = "none"` means only try-now acquire works: a wait with a timeout is
  `unsupported`. `wait = "blocking"` means the plugin implements `wait`: one invocation parks on
  the plugin's own push signal until the resource is free or `timeout_ms` passes, and Hot Sheet
  allows that process the wait plus one operation's timeout. Hot Sheet never loops on a timer.
- **Credentials only on stdin.** A `secret` setting is stored in the OS credential store under
  `resource-claims.<configuration>.<key>` (`hotsheet-cli resource backend secret <id> <key>`,
  value on standard input) and appears in the envelope's `settings` and nowhere else: not in
  argv, not in the environment, not in a configuration file, not in output. A secret-typed key
  given as a `--setting` is refused. The configuration's metadata key (§36.15) is separate.
- **Approval by directory digest.** The first `backend add` that names a plugin approves it as
  it then is, recording a SHA-256 over every file in the directory in
  `<HOTSHEET_HOME>/claim-backends/approvals.json` (a machine decision, never checked in).
  Before every run Hot Sheet recomputes the digest; an unapproved plugin, or one with any file
  added, removed, renamed or edited, is refused without running and the error names
  `hotsheet-cli resource plugin trust <id>`. A repeated `add` never re-trusts changed code.
- **Collisions and load errors.** The built-in ids (`local`, `git-ref`, `upstash-redis`,
  `external`) win a collision; a plugin that takes one fails to load. A directory that fails to
  load is listed in `load_errors` of `resource plugin list` and never hides the others.
- **Fail closed.** An unreachable plugin refuses the claim; a `provisional` offline policy is not
  offered for external backends. Ticket claims do not route through an external backend yet
  (HS2-3SJ8JX): `resource source bind` refuses one and a store bound to one refuses to claim.

`hotsheet-cli resource backend add <id> --kind external --setting plugin=<id> [--setting
key=value]` configures one; `resource plugin list` shows each plugin's capabilities, settings
and approval state; `resource plugin trust <id>` approves the current content; `resource
backend test <id>` approves nothing and makes one read-only `namespaces` call.

**Conformance.** `hotsheet_resource_claims::conformance::run` is the suite every backend must
pass, built-in or external: capabilities within Hot Sheet's bounds; a grant with a fencing token
and a token that names its own claim; a held resource refusing a second acquire with `held_by`
naming the holder; readers (`get`, `list`, `namespaces`) seeing the claim without its token;
metadata round trip; renew extending the lease and keeping the fencing token; a garbage, revoked
or earlier claim's token refused; release freeing the resource; fencing tokens strictly growing
across release and expiry; a lapsed claim reading as free and refusing renew with `expired`;
`verify_fencing` following the live claim; zero, oversized and unprefixed requests refused with
`invalid`; six concurrent acquirers with exactly one winner; and waiting that matches the
advertised mechanism. The local backend, the Git-ref backend and a fake external plugin run it.

## 36.17 The Upstash Redis REST backend (HS2-K9J1RK)

`kind = upstash_redis` serves a namespace from an Upstash Redis database over its REST API. It
is exclusive with fencing and waits by push signal. `hotsheet-cli resource backend add <id>
--kind upstash-redis --setting url=<REST URL> [--setting prefix=<key prefix>]` configures it;
`resource backend secret <id> token` stores the REST token in the OS credential store (value on
standard input); `resource backend test <id>` reads the namespace set, a real read-only call that
also proves the token. The URL must be `https://` (plain `http://` only to a loopback address, for
development), without credentials, query or fragment: the token travels in an `Authorization`
header and nowhere else. A secret never goes in a setting, and `provisional` offline is not offered
(fail closed; HS2-NYXQ3Y).

**Atomic steps on the Redis clock.** Acquire, renew and release are Lua scripts
(`upstash::scripts`), so each is one atomic step and expiry uses `TIME` on the Redis server, never
the client's clock. Each script's first line names it (`-- hs2rc:acquire:v1`).

| Key                                     | Holds                                                                                                            |
| --------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `<prefix>:v1:<namespace>:c:<resource>`  | the claim: a hash of claim id, token digest, fencing token, acquired and expiry (Redis ms) and the metadata JSON |
| `<prefix>:v1:<namespace>:f:<resource>`  | the fencing counter, `INCR`ed on every grant and never expired                                                   |
| `<prefix>:v1:<namespace>:r`             | the set of resource names Hot Sheet has claimed in the namespace (what `list` reads)                             |
| `<prefix>:v1:namespaces`                | the set of namespaces (what `namespaces` reads)                                                                  |
| `<prefix>:v1:<namespace>:ch:<resource>` | the pub/sub channel a release publishes on                                                                       |

Keys hold only the prefix and the namespace and resource names the caller chose; values hold ids, a
SHA-256 of the renewal token (the token itself is never sent), numbers and the bounded metadata
(§36.3), so no customer content reaches Redis unless a caller puts it in a name or a `plain:` value.
The counter and the two sets are small and never expire; a very large, churning namespace should be
pruned with HS2-FYXHA6.

- **Expiry and `expired`.** Liveness is the stored expiry against `TIME`. The claim hash is kept
  for a one hour grace past its expiry (`PEXPIRE lease+grace`), so a late `renew` answers `expired`
  as the other backends do; after the grace the hash is gone and the token is simply unknown
  (`token_invalid`). A grant after any gap still carries a larger fencing token.
- **Waiting.** Blocking list pops (`BLPOP`, `BRPOP`) are not available over Upstash REST, so a wait
  holds one `POST /subscribe/<channel>` server-sent-event stream open. The wait reads the holder and
  its time left, opens a stream that lives that long (or the wait's timeout, if sooner), re-reads the
  state once the service confirms the subscription (so a release in the gap is found, not waited
  for), and then reads until a message, the end of the stream (expiry) or the deadline. A read
  timeout that ends before the stream's lifetime (a Windows ARM64 socket timer returns early) sleeps
  the rest instead of reading and subscribing again (HS2-QX9QQE). A release
  publishes `released`; a renewal that shortens a lease publishes `shortened`. Nothing polls on a
  timer: a parked wait sends no request, and a stream that fails backs off 250 ms, 500 ms … up to
  5 s, at most six times, then fails with `backend_unreachable`.
- **Rate limits and errors.** A 429 is retried at most three times after the `Retry-After` delay
  (capped at 2 s); more is `backend_unreachable` naming the configuration. A 401 is `unsupported`
  and names `resource backend secret <id> token`; a refused command is a storage error; an unreachable
  service fails closed.
- **Ticket claims** do not route through this backend yet (HS2-3SJ8JX); `resource source bind`
  refuses it.

**Verification.** `tests/upstash.rs` runs the conformance suite and the cases above against a fake
that speaks the service's real HTTP shapes (401, 429 with `Retry-After`, 400 `{"error"}`, a JSON
command array, `/pipeline`, SSE) and applies each script's documented contract; it refuses a script
whose text differs from the library's. The fake has no Lua interpreter, so the scripts run against
real Redis only in `tests/upstash_live.rs`, which needs a dedicated database
(`HOTSHEET_TEST_UPSTASH_URL`, `HOTSHEET_TEST_UPSTASH_TOKEN`) and returns at once without one. Run it
after any script change ([manual-test-plan.md](manual-test-plan.md), "Upstash claim backend").
