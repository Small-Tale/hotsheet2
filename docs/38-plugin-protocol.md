# 38. Out-of-process plugin protocol

Status: **Decided (design)** — HS2-XE0ZVB, 2026-11-27. Implementation is tracked by the
`plugin-runtime` tickets listed in §38.14.

## 38.1 Why

Hot Sheet's plugins must be real plugins: loaded at runtime, with no recompile, and
written by people outside this repository (maintainer, 2026-11-27, HS2-FR0PAE).

- **What loads at runtime today:** manifest-only AI-tool plugins, which are data
  ([05](05-ai-tool-plugins.md) §5.11).
- **What is compiled into the host:**
  - tool-specific drives (Claude channel, Codex app-server) in `hotsheet-aitools`;
  - the GitHub, GitLab and Jira ticket sources in `hotsheet-extsync`;
  - the built-in notifiers in `hotsheet-notify`.
- **Existing subprocess plugins:** external notifiers and external claim backends.
  They use a one-shot protocol: one process per call, one JSON envelope on stdin,
  and the last JSON line on stdout. There is no handshake.
- **Deadline:** third-party ticket sources are expected around 2026-12-27.

This document defines one protocol for every code-bearing plugin. Rust has no stable
ABI, so a "loadable plugin" is a **separate executable** that the host starts and
speaks to over this protocol, never a `.dylib`.

## 38.2 Decisions at a glance

| Topic              | Decision                                                                                                                                                                                                                                          |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Contract language  | Protobuf (`crates/hotsheet-plugin-protocol/proto/hotsheet/plugin/v1/`) is the single source of truth. Generated Rust is checked in and byte-checked; `protoc` is vendored, never assumed.                                                         |
| Transports         | JSON-RPC 2.0 as newline-delimited JSON over stdio (always supported) and length-prefixed protobuf frames over stdio (negotiated, high-performance). Benchmark: [plugin-transport-benchmark-2026-11-27](plugin-transport-benchmark-2026-11-27.md). |
| gRPC               | Not a host transport. Revisit only for networked or remote plugins.                                                                                                                                                                               |
| Process model      | One long-lived process per plugin per host process, multiplexing connections. Requests may be in flight concurrently.                                                                                                                             |
| Capabilities       | `ticket_source` first; `ai_drive` and `notifier` next. Claim backends, remote-change and file viewers adopt it later.                                                                                                                             |
| Trust              | Directory digest, stored approval, and a re-check before every spawn, unified across all plugin kinds. No OS sandbox in v1.                                                                                                                       |
| WASM               | Dropped from scope; there is no pure-compute plugin need. Reopen with a concrete use.                                                                                                                                                             |
| Markdown/Git store | Stays in core (§38.12).                                                                                                                                                                                                                           |

## 38.3 Package layout and manifest

A plugin is a directory `${HOTSHEET_HOME:-~/.hotsheet2}/plugins/<id>/`. The id rule is
the existing one: `[a-z0-9][a-z0-9-]{0,63}`. First-party plugins ship in the
Hot Sheet install under `<install>/plugins/<id>/` and are loaded by the same loader.
Built-in ids win collisions, as they do today.

```toml
# manifest.toml
id = "github"
display_name = "GitHub Issues"
version = "1.0.0"
protocol = { min = 1, max = 1 }          # plugin protocol versions it speaks
capabilities = ["ticket_source"]

[executable]
program = "bin/hs-plugin-github"         # relative to the plugin directory
args = []
# Optional per-target overrides, keyed by Rust target triple:
[executable.targets]
"x86_64-pc-windows-msvc" = "bin/hs-plugin-github.exe"

[[settings]]                              # typed, as in notifier/claim-backend manifests
key = "api_base"
label = "API base URL"
type = "string"
required = false

[[settings]]
key = "token"
label = "Access token"
type = "secret"                           # stored in the OS keychain; sent only in-protocol
required = true
```

- **Program resolution.** A relative `program` must exist inside the plugin directory
  and is never looked up on `PATH`. The executable bit is preserved on install.
- **Manifest-only AI-tool plugins are unchanged.** A manifest without
  `[executable]` stays a data plugin; `[executable]` plus `capabilities` makes it
  code-bearing.
- **Loader fix.** The loader must copy subdirectories and binary files on install;
  today `from_fs_dir` drops them.
- **Compatibility.** The legacy `notifiers/` and `claim-backends/` directories keep
  loading through their one-shot protocol for one release. Each load prints a
  deprecation warning that names this document.

## 38.4 Contract

- **Location.** The protobuf files live in
  `crates/hotsheet-plugin-protocol/proto/hotsheet/plugin/v1/`. Generated Rust (prost types
  plus pbjson canonical-JSON impls) is checked in under `src/gen/`; regenerate with
  `cargo run -p hotsheet-plugin-protocol --features regen --bin hotsheet-plugin-protocol-regen`,
  and `tests/regen.rs` fails when it is stale (HS2-XY0VTN). The files are:
  - `envelope.proto`: frames, handshake, errors, cancellation, logs;
  - `diagnostics.proto`: the `diagnostics` transport-check capability (§38.11);
  - `ticket_source.proto`;
  - `ai_drive.proto`;
  - `notifier.proto`;
  - `host.proto`: calls a plugin makes into the host.
- **Methods.** Each capability is a protobuf `service`, and each RPC is a method named
  `<capability>.<Method>`, for example `ticket_source.Get`. Streaming responses are
  server-streaming RPCs. Client-streaming RPCs are not used; uploads are chunked
  unary calls (§38.8).
- **Versioning.**
  - The protocol major version (`v1`) changes only for breaking changes.
  - Fields are added compatibly, following proto3 rules.
  - Optional methods are guarded by capability features declared in the handshake.
  - Unknown fields are ignored.
  - An unknown method returns `unsupported`.

## 38.5 Transports and framing

Both transports run over the plugin's **stdin/stdout**. These rules apply to both:

- stdout carries protocol data only.
- stderr is the plugin's log. The host captures it, rate-limits it, redacts secrets
  from it, and records it in `tracing` under the plugin id.

### JSON-RPC 2.0 (always supported)

- **Framing.** One JSON object per line (`\n`), UTF-8, with no `Content-Length` headers.
  This matches the existing ACP, Codex and MCP code in the workspace.
- **Payloads.** `params` and `result` are the **canonical proto3 JSON mapping** of the
  request and response messages. `bytes` fields are base64, and 64-bit integers are
  strings.
- **Streams.** A streaming response is a sequence of notifications
  `{"method":"$/stream","params":{"id":<request id>,"item":{...}}}`, followed by
  the ordinary response that ends the stream.

### Protobuf frames (negotiated)

- **Framing.** Each frame is a 4-byte little-endian length followed by an encoded
  `Frame` message.
- **The `Frame` message.** It is a `oneof` of `Request{id, method, payload}`,
  `Response{id, payload | error}`, `StreamItem{id, payload}`, `Cancel{id}` and
  `Log{level, message}`. The payload is the encoded request or response message.
- **Size limits.** A frame may be at most 64 MiB. Larger data is chunked (§38.8).

### Handshake

The handshake is always JSON:

1. The host writes
   `{"jsonrpc":"2.0","id":0,"method":"$/hello","params":{"protocol_versions":[1],"transports":["protobuf","json"],"host":{"name":"hotsheet","version":"…"},"plugin_id":"github","data_dir":"…"}}`.
2. The plugin answers with
   `{"protocol_version":1,"transport":"protobuf","plugin":{"version":"1.0.0"},"capabilities":[{"name":"ticket_source","features":["notes","attachments","watch", …]}]}`.
3. If it chose `protobuf`, both sides switch to frames immediately after that line.
   A plugin that only speaks JSON simply picks `json`.

The host refuses the plugin, and reports the reason, if:

- there is no common protocol version;
- the declared capabilities differ from the manifest's;
- the handshake doesn't arrive within 10 seconds.

### Calls in both directions

- Either side may send requests. Plugin-to-host methods are defined in `host.proto`:
  - `host.StoreCredential`, used when a refreshed OAuth or app token must be written back;
  - `host.EmitChange`, used to push changes when a remote reports them;
  - `host.Progress`, for long operations.
- Request ids are per-direction integers.
- Responses may arrive out of order, and the host keeps several requests in flight.

### Cancellation and deadlines

- Every request carries `deadline_ms` in its envelope metadata. The defaults are
  30 seconds for unary calls and none for streams.
- `$/cancel` (JSON) or `Cancel` (protobuf) asks the plugin to stop. The host stops
  waiting at the deadline either way.

### Errors

Errors carry one structured `PluginError{code, message, retry_after_seconds?, details}`.
The codes map onto the host's error types; for ticket sources, that is `ProviderError`:

| Code               | Meaning                                                                                     | Host mapping                             |
| ------------------ | ------------------------------------------------------------------------------------------- | ---------------------------------------- |
| `not_found`        | object does not exist                                                                       | `NotFound`                               |
| `unsupported`      | capability or method not implemented                                                        | `Unsupported{capability}`                |
| `authentication`   | credential missing, expired or refused                                                      | `Authentication`                         |
| `conflict`         | stale concurrency token or competing write; `details.stale=true` replaces the magic strings | `Conflict`                               |
| `rate_limited`     | remote throttled; `retry_after_seconds` set                                                 | `RateLimited`                            |
| `invalid_argument` | bad input, including an unparseable native id                                               | `InvalidNativeId`/`Operation`            |
| `unavailable`      | remote or plugin temporarily unreachable (retryable)                                        | transport-unavailable error (HS2-2YAM7F) |
| `internal`         | plugin bug                                                                                  | `Operation`                              |

On the JSON transport the error is the JSON-RPC `error` object. Its `code` is
`-32000`, and `data` holds the `PluginError`.

## 38.6 Process lifecycle

- **Spawn.** The host spawns a plugin lazily, on first use, from the resolved
  executable:
  - the working directory is the plugin's data directory;
  - the environment is cleared except for an allowlist: `PATH`, `HOME`, `USER`,
    `LANG`/`LC_*`, `TMPDIR`, proxy variables, `SSL_CERT_*`, and the
    `HOTSHEET_PLUGIN_DATA_DIR` variable;
  - on Windows the process is placed in a Job Object, and on Unix in its own
    process group.
- **One process per plugin per host process.** The CLI and server each run their own
  copy.
  - Every ticket-source call names its `connection_id`.
  - Connection settings and credentials are sent by `ticket_source.Configure` before
    first use, and again after a change or credential rotation.
  - Plugins keep state only in `HOTSHEET_PLUGIN_DATA_DIR`
    (`${HOTSHEET_HOME}/plugin-data/<id>/`) and must tolerate concurrent instances
    there, using file locks.
- **Idle exit.** A plugin with no in-flight work and no open stream for 10 minutes is
  shut down. The host checks idleness on events, not on a polling timer.
- **Shutdown.** Shutdown is ordered:
  1. the host sends `$/shutdown` (2-second grace);
  2. it closes stdin;
  3. it sends TERM;
  4. it sends KILL.
     This reuses `procio::stop_process`.
- **Crash isolation.**
  - When the process exits or the stream breaks, every in-flight call fails with
    `unavailable` and open streams end.
  - The next call respawns the plugin, with exponential backoff (1 s doubling to 60 s).
  - Five crashes within 10 minutes open a circuit: affected connections report
    `disabled` with the plugin's last stderr lines, until the user re-enables them or
    the plugin's digest changes.
- **One supervisor.** The `hotsheet-plugin-host` crate owns the supervisor (spawn,
  handshake, multiplexing, backoff and shutdown) for every capability. It replaces the
  duplicated notifier and claim-backend runners and the two near-identical JSON-RPC
  engines in `hotsheet-aitools`.

## 38.7 Trust, consent and secrets

- **Digest gate for every plugin kind.**
  - Approval stores `plugin_digest` (SHA-256 of the whole directory, from
    `hotsheet-ticketing/src/plugin_digest.rs`) in
    `${HOTSHEET_HOME}/plugins/approvals.json`.
  - The digest is re-checked before every spawn; a mismatch refuses to start.
  - Re-trust is explicit: `hotsheet-cli plugin trust <id>` or the REST equivalent.
  - This extends to AI-tool plugins, which have no gate today.
- **Consent.** Before approval, consent shows:
  - the executable path and digest;
  - the capabilities;
  - the declared settings and secrets;
  - the provenance: first-party, or third-party (unsigned).
- **First-party plugins.** Their digests are pinned at build time and need no prompt.
- **Signing.** Code signing for third-party plugins is a follow-up.
- **Secrets.** Secrets stay in the OS keychain (`KeyRegistry`/`SecretStore`).
  - They are sent only inside protocol messages: `Configure`, or a per-call credential
    field. Never in argv or the environment.
  - Plugins must not persist secrets. A plugin that refreshes a token returns it
    through `host.StoreCredential`.
  - Debug and log output redact secret fields.
- **No OS sandbox in v1.** A plugin runs with the user's privileges, like any program
  the user installs. The consent text says so. OS sandboxing (macOS sandbox profiles,
  Linux landlock/seccomp, Windows AppContainer) is a follow-up.

## 38.8 Capability: `ticket_source`

This mirrors `TicketProvider` (`crates/hotsheet-ticketing/src/provider.rs`), so the
host-side adapter is a thin `TicketProvider` implementation that forwards each call.
Provider-neutral machinery stays in the host:

- routing by `(connection_id, native_id)`;
- the registry and copy/move with provenance;
- the outbox and write-behind;
- the overlay and change events;
- credentials.

| Method                                                                                                                                                                                                     | Mirrors                            | Notes                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------- | -------------------------------------------------------------------------------------------------- |
| `Configure{connection, settings, credential}`                                                                                                                                                              | `live_provider(connection, token)` | Idempotent; re-sent on change                                                                      |
| `Describe{connection_id}` → descriptor                                                                                                                                                                     | `descriptor()`                     | Capabilities and `query_fields`; replaces per-kind host matches                                    |
| `ParseNativeId{connection_id, input}`                                                                                                                                                                      | `extsync::native_id`               | Accepts URLs and short forms                                                                       |
| `Query`, `QueryPage{cursor, limit}`, `QueryAfter{after, resume, limit}`, `Summary`                                                                                                                         | same                               | `QueryAfter` keeps the shared checkout order                                                       |
| `Get`, `Create{ctx, draft}`, `Update{native_id, now, patch}`, `Close`, `Assign`                                                                                                                            | same                               | `ctx.generated_id` is the idempotency key; `patch.expected_token` drives `conflict`                |
| `AddNote`, `EditNote`, `DeleteNote` (with metadata)                                                                                                                                                        | same                               | Note-trailer encoding stays a shared helper in the SDK                                             |
| `ClaimNext`, `Release`, `Renew`, `AdvisoryClaims`, `AppendAdvisoryClaimEvent`                                                                                                                              | same                               | Feature-gated                                                                                      |
| `GetAttachment{native_id, attachment_id, original}` → stream of `Chunk{offset, bytes}` (1 MiB)                                                                                                             | `attachment_bytes`                 | Streaming download                                                                                 |
| `BeginAttachmentUpload` → `upload_id`; `UploadChunk`; `CommitAttachmentUpload`                                                                                                                             | `add_attachment`                   | Chunked upload; 100 MiB cap preserved                                                              |
| `SetAttachmentMarkup`, `DeleteAttachment`, `Restore`, `PurgeTrashed`, `GroupDirectory`, `SetGroupOfMembers`, `AiFeedbackRecords`, `RateAiContent`, `ReportNotWorking`, `FindTransfer`, `AssignCurrentUser` | same                               | Feature-gated; default `unsupported`                                                               |
| `Watch{connection_id, cursor}` → stream of `ChangeEvent{native_id, kind, cursor}`                                                                                                                          | new (`watch` capability)           | Host resumes with the last cursor; replaces fixed host polling                                     |
| `HandleWebhook{connection_id, headers, body}` → changed ids                                                                                                                                                | `github_webhooks.rs` logic         | Host owns the HTTP endpoint `/webhooks/plugins/{connection_id}`; the plugin verifies the signature |
| `Connect` / `BeginAuth` / `CompleteAuth`                                                                                                                                                                   | connect and device flows           | Interactive and device sign-in steps return prompts the host renders, headless or in the client    |

- **Provider-kind branching moves into these methods.** Today the host has matches on
  the provider name outside `extsync`; HS2-5P32MR removes them by moving each behind a
  method above. The affected places are:
  - the GitHub endpoint registry;
  - webhook setup;
  - the connect commands;
  - `accounts.rs` host keys;
  - `native_id.rs`;
  - display names.
- **The client contract does not change.** The web client already reads
  `ProviderDescriptor` capabilities. HS2-Y8GQEW removes its remaining provider-name
  comparisons.

## 38.9 Capability: `ai_drive`

This mirrors `Drive`/`TurnHandle` (`crates/hotsheet-aitools/src/drive.rs`):

| Method                                                                  | Notes                                                          |
| ----------------------------------------------------------------------- | -------------------------------------------------------------- |
| `Info`, `ModelCatalog`                                                  |                                                                |
| `Run{target, content, cwd, model, effort, env}` → stream of `TurnEvent` | `Output`, `PermissionAsked`, `Usage`, `NativeActivity`, `Done` |
| `AnswerPermission{turn_id, request_id, decision}`                       | The host keeps the shared permission bridge and vault          |
| `Interrupt{turn_id}`                                                    |                                                                |

- **The host keeps:** PTYs and terminals, the permission bridge, metrics storage,
  sessions and jobs.
- **The plugin owns** the tool-specific client: the Claude stream-json channel, the
  Codex app-server protocol, and transcript or rollout watchers.
- **ACP and the generic spawn drive** are standard protocol shapes and may stay as host
  transports that manifest-only plugins select. HS2-8Q4K4S decides each case and
  records the reasoning in [05](05-ai-tool-plugins.md).

## 38.10 Capability: `notifier`

- **Method.** `Deliver{channel_id, settings, message}` → `{id?, retryable}`, with
  `PluginError` on failure.
- **Capabilities.** The capabilities ([32](32-notifier-plugins.md)) are returned in the
  handshake rather than in the manifest.
- **Migration.** HS2-MSZSQ8 moves external notifiers to this protocol and decides
  whether the built-ins move.

## 38.11 Conformance and SDK

- **Conformance suite.**
  - Each capability has a suite in a `hotsheet-plugin-conformance` crate, modelled on
    `hotsheet-resource-claims`'s `conformance::run`.
  - It runs against any installed plugin over **both** transports, so a plugin
    declaring protobuf must also pass on JSON when asked.
  - `hotsheet-cli plugin verify <id>` runs it, along with the structural checks.
  - The first-party GitHub, GitLab and Jira plugins must pass it in CI against fakes.
- **SDK.**
  - `hotsheet-plugin-sdk`, a Rust crate, provides the handshake, both transports,
    dispatch to a typed service trait, the stream helpers and the note-trailer helper.
  - Authors in other languages generate message types from the `.proto` files; the
    JSON transport needs no generated code at all.
  - HS2-0M14YS publishes the author guide and an example plugin.

## 38.12 What stays in core

The **Markdown/Git ticket store** stays in core. It is Hot Sheet's native store, not an
adapter to someone else's system. Offline and headless operation, merge, claims,
indexes and the migrator depend on it directly, and a plugin boundary would add a
process hop to every local read without adding extensibility.

Shared machinery also stays in the host:

- credentials;
- the outbox;
- events;
- the plugin supervisor;
- terminals and the permission bridge.

## 38.13 Workspace layout

| Crate                                                 | Role                                                                                                                             |
| ----------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `hotsheet-plugin-protocol`                            | `.proto` files, checked-in generated types, framing and handshake; no async runtime                                              |
| `hotsheet-plugin-host`                                | code-bearing manifest, trust gate, supervisor and capability client adapters (kept out of the 18k-line loader crate, HS2-XY0VTN) |
| `hotsheet-plugins`                                    | AI-tool plugin loader (existing); installs code-bearing plugins with their binaries                                              |
| `hotsheet-plugin-sdk`                                 | plugin-author helpers (used by first-party plugins too)                                                                          |
| `hotsheet-plugin-conformance`                         | per-capability conformance suites                                                                                                |
| `plugins/<id>/` crates (e.g. `plugins/github-source`) | first-party plugin executables, built and shipped beside the CLI and server                                                      |

`hotsheet-extsync` shrinks to the shared helpers the first-party source plugins use,
or is folded into them. The policy test from HS2-WXBD02 then pins that host crates
carry no tool- or provider-specific code.

## 38.14 Implementation tickets

- HS2-XY0VTN: protocol crate, supervisor, both transports, trust gate, and the loader fix for binaries and subdirectories.
- HS2-5P32MR: `ticket_source` capability, with the GitHub, GitLab and Jira plugins moved out.
- HS2-0M14YS: SDK, author guide, example plugin, and conformance through `plugin verify`.
- HS2-8Q4K4S: `ai_drive` capability, with tool-specific drives moved out.
- HS2-MSZSQ8: `notifier` capability and migration of external notifiers.
- HS2-WXBD02: policy test that host crates carry no tool- or provider-specific code.
- Follow-ups filed by HS2-XE0ZVB:
  - code signing for third-party plugins (HS2-A14SKY);
  - OS sandboxing of plugin processes (HS2-SA5KGZ);
  - moving claim backends onto this protocol (HS2-DRMG92);
  - project-scoped plugin directories, `.hotsheet2/plugins/` (HS2-4V38T2).
