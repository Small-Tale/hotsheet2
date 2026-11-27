# Plugin transport benchmark (2026-11-27)

HS2-XE0ZVB measured three ways for the host to talk to an out-of-process plugin
before choosing the plugin protocol's transports ([38](38-plugin-protocol.md)).
The spike is `spikes/plugin-transport/`; run it with
`cargo run --release` from that directory.

## Setup

- One binary is both host and plugin, so every transport moves the same payloads.
- Payloads are ticket-shaped: a ticket has an id, a title, an 800-byte body, three labels, a status and a timestamp (about 0.9 KB as JSON).
- The host is synchronous, like `TicketProvider`. The gRPC client therefore pays a `block_on` per call, as a real synchronous host would.
- Release build on an Apple M1 Pro. Other workers were running, with a load average of about 375. Absolute numbers are inflated, but the ranking was identical in all five runs.

## Results (medians of 5 runs)

| Transport                            | Start-up to first answer | Small call p50 | Small call p99 | List of 10,000 tickets (~9 MB) | 16 MiB blob      | Stream of 100,000 events |
| ------------------------------------ | ------------------------ | -------------- | -------------- | ------------------------------ | ---------------- | ------------------------ |
| JSON-RPC 2.0, NDJSON over stdio      | ~3.4 ms                  | 6.8 µs         | ~130 µs        | ~42 ms                         | ~105 ms (base64) | ~1.2 M/s                 |
| Length-prefixed protobuf over stdio  | ~5 ms                    | 5.1 µs         | ~56 µs         | ~16 ms                         | ~16–32 ms        | ~5.8 M/s                 |
| gRPC (tonic 0.12) over a Unix socket | ~6 ms                    | 42 µs          | ~2.2 ms        | ~60 ms                         | ~83 ms           | ~3 M/s                   |

## Conclusions

- **Remote cost dominates single calls.** A GitHub, GitLab or Jira call costs 100–500 ms, so even JSON-RPC adds under 0.01%. The transport matters only for bulk lists, attachment bytes and event streams.
- **Protobuf over stdio is the fastest on every bulk measure:**
  - about 3× JSON for lists;
  - 5–7× JSON for blobs, because there is no base64;
  - about 5× JSON for event streams.
- **gRPC over a Unix socket is the slowest per call in a synchronous host.**
  - HTTP/2 plus the per-call `block_on` gives about 6× JSON's p50 and a long p99 tail.
  - It beats JSON on blobs and streams but loses to protobuf over stdio on every measure.
  - It would add tonic, hyper and h2 to both binaries, plus a socket lifecycle (named pipes on Windows).
- **The code-generation benefit doesn't need gRPC.** Generated code in any language comes from using protobuf as the contract language, not from gRPC as the transport.
- **Decision** (maintainer, 2026-11-27): JSON-RPC over stdio is always supported, and length-prefixed protobuf over stdio is the negotiated high-performance transport. gRPC is not a host transport for now.
- **Side finding:** the Homebrew `protoc` on the benchmark machine was broken (a missing abseil library). The build therefore vendors `protoc`, and generated code is checked in.
