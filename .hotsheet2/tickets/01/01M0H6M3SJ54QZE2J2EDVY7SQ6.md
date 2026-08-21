---
id: 01M0H6M3SJ54QZE2J2EDVY7SQ6
slug: HS2-69W1C1
title: 'Codex ProxyTransport: complete the shared-daemon handshake (codex app-server proxy)'
category: feature
priority: default
status: started
created_at: 2026-08-21T01:27:25.589Z
updated_at: 2026-08-21T01:38:02.278Z
legacy_number: HS2-115
schema: 1
---

Follow-up from HS2-112. The live client uses StdioTransport (codex app-server direct: one persistent process per connection, which satisfies "no process per play"). ProxyTransport (codex app-server proxy -> shared daemon control socket) is built and compiles, but the running daemon is already initialized and does NOT answer a fresh JSON-RPC "initialize" from a newly-connected proxy client (probed manually: initialize over the proxy returns no response; direct app-server answers it immediately with the same framing).

To enable cross-connection instance sharing (multiple host connections reusing ONE codex daemon), figure out the proxy/daemon join handshake: likely the proxy client must NOT re-run initialize (skip straight to thread/start or thread/resume against the daemon's existing session), or there's a proxy-specific hello. Then switch CodexAppServer::connect to skip/adjust the handshake when the transport is a proxy, and add a gated live test that drives the shared daemon (ensure_codex_daemon already starts it). The daemon also requires the managed standalone install under $CODEX_HOME/packages and a short control-socket path (< SUN_LEN) — the isolated-home test harness must account for both.

Relates to: HS2-112, HS2-110, docs/13 §13.5.
