# 36. Resource Claims

A resource claim leases one named resource to one holder for a duration. The holder
renews the claim while work continues and releases it early when done. A claim that is
not renewed lapses by itself, so a stuck or crashed holder never blocks a resource
forever.

Two uses drive the design (decided 2026-11-03, HS2-H3WPPY):

1. **Ticket claims** that show who or what holds a ticket, for any ticket data source.
2. **Non-ticket resources**, for example a local macOS VM that allows one automation
   process at a time.

This document covers the contract and the built-in local machine backend, which is the
`hotsheet-resource-claims` crate. Backend configurations, namespace binding, and the
other backends are design only; §36.9 names their tickets. The claim on a Git-backed
ticket ([10-assignment-and-collaboration.md](10-assignment-and-collaboration.md)) is
unchanged and is not yet routed through this contract.

## 36.1 Model

- A **namespace** groups resources and is bound to one backend. A machine-scoped
  namespace lives under `HOTSHEET_HOME` and is shared by every project on the machine.
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
configurations keep their key in the OS credential store beside the access credential
(HS2-JJWESD).

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

`ClaimBackend` is a blocking Rust trait: `capabilities`, `acquire` (try-now),
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
as exclusive. The local machine backend has no remote and is never offline. Design
only; HS2-JJWESD.

## 36.8 Plugin form

Backends are implementations of the Rust trait. The planned built-ins are the local
machine file backend (shipped), the existing Git-ref compare-and-swap, and Upstash Redis
REST. External backends will use a subprocess protocol modeled on notifier plugins
([32-notifier-plugins.md](32-notifier-plugins.md) §32.6). Every setup and recovery path
has an idempotent `hotsheet-cli` path shared with the graphical client.

## 36.9 Status

| Part                                                                                             | Status      | Ticket     |
| ------------------------------------------------------------------------------------------------ | ----------- | ---------- |
| Backend contract, error model, codecs, local machine backend                                     | Shipped     | HS2-H3WPPY |
| CLI, MCP, and server surfaces with token-only renew and release                                  | Design only | HS2-FMHH9Y |
| Backend configurations, namespace and data-source binding, offline policy, credential-store keys | Design only | HS2-JJWESD |
| AI guidance for the acquire-and-hand-off pattern                                                 | Design only | HS2-SHDW2Q |
