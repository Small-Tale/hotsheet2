# 37. Remote-change notifications

Status: partial. The contract, connections, bindings, router, hub and headless workflow ship
with HS2-8X2S08; the Ably adapter is HS2-BWJWWX; waking Git ticket-source sync from a hint is
HS2-HY8ZPG; server routes and client settings are HS2-9DHKTF.

A Hot Sheet server learns that a Git remote advanced by running its own sync. An external event
service (a GitHub webhook fanned through a realtime channel, for example) can say so sooner. This
document defines the optional, inbound path: a _hint_ that a remote may have advanced, which a
consumer answers by checking the authoritative remote. It is separate from AI-tool plugins,
ticket providers and the outbound notifiers of [32-notifier-plugins.md](32-notifier-plugins.md).

## 37.1 Principles

- **Git is authoritative.** A hint is untrusted. It cannot change Git status, ticket data or a
  claim. Its only effect is to ask a consumer to _check_ a source now.
- **Optional.** An unbound project or ticket store behaves exactly as before. A machine with no
  connection runs no thread and opens no socket.
- **No secret leaves its store.** A connection's credential lives in the OS credential store,
  never in the registry, a project file, a browser response, a log or a ticket note. Its `Debug`
  output prints `<redacted>`.
- **No client polling.** A connection is a push stream. The hub's only periodic local wake is an
  adapter checking its stop flag; no request is issued on a timer.

## 37.2 The adapter contract (version 1)

`hotsheet_notify::remote_change::model::ChangeAdapter` is one external event service:

| Member                               | Meaning                                                                                                                                    |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `descriptor()`                       | id, display name, contract version, the plain settings it takes, and whether it needs a credential                                         |
| `run(context, channels, emit, stop)` | open the connection, attach `channels`, report events until `stop` is set (`Ok`) or it fails (an `AdapterError`); never retries on its own |
| `test(context)`                      | one cheap real check of the configuration and credential                                                                                   |

An adapter emits two kinds of event: `Connected { resumed }` once the channels are attached (and
again after a gap the adapter itself resumed over), and `Hint { channel, hint }`. A `ChangeHint`
is the normalized form: the repository identity (`id` and/or `full_name`), the full ref name, the
tip the event advertised, the delivery's event id and a time. A failure is `Auth` (retrying cannot
help), `Config` (it cannot work) or `Transient` (reconnect).

## 37.3 Connections and bindings

`<HOTSHEET_HOME>/remote-change/connections.json` (machine-local, mode 0600, replaced atomically
under a lock, secret-free) holds two lists.

- A **connection** is reusable: `{id, plugin, settings, credential}`. `settings` are plain values
  the plugin's descriptor names (a token pasted as a setting is refused); `credential` names the
  credential-store account `remote-change.<id>`.
- A **binding** ties a **source** to a connection: `{source, connection, channel, repository,
refs}`. Source, connection and channel together are its identity. A source is
  `checkout:<checkout id>` for a project's code checkout or `store:<instance id>` for a Git ticket
  store (every clone of the store answers to it). `repository` is the identity the source expects
  events about (`owner/name` and/or the service's numeric id); a binding without one is refused.
  `refs` are exact names or prefixes ending in `*` (default `refs/heads/*`, so tags are not
  followed unless asked).

A project code repository and several Git ticket stores may bind to one connection, each with its
own channel, repository and refs. A newer-schema or damaged file is refused and left as it is.

## 37.4 Trust and replay

The `Router` decides, for each hint, whether it is for a bound source:

1. **Channel.** No binding on this connection and channel: ignored (`unknown_channel`).
2. **Identity.** The hint's repository must agree with the binding's: a field both sides state may
   not differ (names compare case-insensitively; a differing id wins over a matching name), and at
   least one field must be stated by both, so a hint that names nothing never matches
   (`wrong_repository`).
3. **Ref.** The ref must be one the binding follows (`unfollowed_ref`).
4. **Duplicates.** A delivery is a duplicate, per source, if its event id or its (ref, tip) was
   already handled. Both keys are recorded, so a delivery repeated under a new id still collapses,
   and a source reached through two bindings or channels is told once. Memory is bounded (2048
   keys, oldest forgotten first).
5. **Coalescing.** The first hint for a (source, ref) after a quiet window is delivered at once; hints
   inside the window are held, and one trailing delivery with the latest hint goes out when the window
   ends. A dispatcher thread sleeps to that deadline; nothing polls.

Delivered hints are `RemoteChange { source, ref_name, advertised_tip, event_id }`. The advertised
tip is something to compare against, never a fact to store. Out-of-order delivery is harmless:
a late older hint is, at worst, one more idempotent check. Ignored hints are counted by reason and
never logged with their content.

## 37.5 The hub

`Hub` shares one subscription per (connection, channel) however many sources are bound, runs one
supervised thread per connection, and reports a typed `Health` per connection: `idle`,
`connecting`, `connected`, `reconnecting {error, retry_in_ms}`, `auth_failed`, `misconfigured`.

- **Connect and reconnect.** After the first connect, and after any later connect that cannot vouch
  for the gap (`resumed: false`), the hub asks the listener to **recheck** every source bound to the
  connection, because a stream may have missed events. A reconnect that resumed over the gap does not.
- **Bounded backoff.** A transient failure waits 500 ms, 1 s, 2 s, … up to 60 s before reconnecting;
  a stop wakes it at once.
- **Terminal failures.** A refused credential, a bad configuration, a missing credential or a
  plugin this build lacks stops the connection until it changes; it does not loop.
- **Reload.** `reload()` re-reads the registry: an unchanged connection is left alone, one whose
  definition or channel set changed restarts, one nothing is bound to stops and reads `idle`, and a
  source that lost its bindings has its held hints forgotten.

The consumer implements `ChangeListener` (`changed`, `recheck`, `health`). Wiring a server's
listener to wake Git ticket-source sync is HS2-HY8ZPG.

## 37.6 Headless workflow

`hotsheet_notify::remote_change::workflow::Workflow` is the one implementation behind
`hotsheet-cli remote-change` and the server's settings. Every operation is idempotent and returns
the registry's change (`created`, `updated`, `unchanged`, `removed`, `absent`).

| Command                                                                                  | What it does                                                                                                                                                                                         |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `plugins`                                                                                | the adapters this build supports, with settings and credential                                                                                                                                       |
| `connection add <id> --plugin <p> [--setting k=v]`                                       | add or change; an unknown plugin is `unsupported_plugin` naming the available ones; settings are checked against the descriptor; the answer names `connection secret` when the credential is missing |
| `connection secret <id> [--delete]`                                                      | store (value on standard input), replace or delete the credential; the value is never printed                                                                                                        |
| `connection test <id>`                                                                   | one real call through the plugin; a missing credential is a failed test that names the command                                                                                                       |
| `connection show / list` / `list`                                                        | connections and bindings; a credential reads `stored` or `missing`                                                                                                                                   |
| `connection remove <id>`                                                                 | refused while bound; deletes the credential with the connection                                                                                                                                      |
| `bind <source> --connection --channel --repository [--repository-id] [--ref]` / `unbind` | add or remove a binding; `--store <path>` binds a Git store's shared identity                                                                                                                        |

## 37.7 Limits and failure semantics

At most 256 bindings and 16 refs per binding; a setting value is at most 1024 printable characters;
a credential at most 4096. A damaged registry stops every command that would write it. A service
outage never affects Git sync, which keeps its own schedule.

## 37.8 Status

| Piece                                                          | Status               |
| -------------------------------------------------------------- | -------------------- |
| Contract, registry, router, hub, workflow, CLI                 | shipped (HS2-8X2S08) |
| Ably adapter                                                   | HS2-BWJWWX           |
| Wake Git ticket-source sync from a hint; server startup wiring | HS2-HY8ZPG           |
| Server routes and client settings                              | HS2-9DHKTF           |
