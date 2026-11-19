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
  until the earlier of the holder's known expiry and its own deadline. A datagram
  carries no information: a woken waiter re-reads the state, so a stray datagram costs
  one read. A namespace accepts at most 256 waiters at a time, and a registration left
  by a crashed waiter is removed a minute after that waiter's own deadline.
- **Crash with a held lease.** The lease stays until its expiry and then lapses. If the
  crashed task returns with its old token after someone else acquired, it is
  `not_holder`.

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
HS2-KCEHVR).

## 36.8 Plugin form

Backends are implementations of the Rust trait. The built-ins are the local machine
file backend and the Git-ref compare-and-swap (§36.14, HS2-WJF0D8); Upstash Redis REST
is planned (HS2-K9J1RK). External backends will use a subprocess protocol (HS2-322ZSZ) modeled on notifier plugins
([32-notifier-plugins.md](32-notifier-plugins.md) §32.6). Every setup and recovery path
has an idempotent `hotsheet-cli` path shared with the graphical client.

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
for a release with `wait_until_free`. It never acquires, renews or releases; the
session that does the work does, as §36.1 prescribes.

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
