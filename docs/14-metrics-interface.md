# 14. Usage/Cost Metrics Interface

> **Status: Design (HS2-69).** A **unified usage/cost metrics interface** every AI
> tool conforms to, based on what HS1 actually _showed_ in the UI — **not** the HS1
> debugging telemetry (tracing / span trees / waterfalls, docs/68), which is dropped
> (maintainer, 2026-08-19). Storage is **files, no DB**. This is the spec + storage
> design + build plan.

## 14.1 What we keep (only usage/cost)

HS1's telemetry served two audiences; we keep **one**:

- **KEEP — usage/cost** that appeared in the UI: today's-cost widget, per-ticket cost
  attribution, and the cost / model-donut analytics (HS1 docs/67, 70, 71).
- **DROP — debugging** telemetry: enhanced tracing, span trees, latency waterfalls
  (docs/68). Not rebuilt.

So the whole subsystem is scoped to "how many tokens / how much money, by model, over
time, attributed to tickets."

## 14.2 The common metric shape

Every AI plugin's `metrics` capability ([05](05-ai-tool-plugins.md) §5.3) maps its
tool's telemetry into one shape — a **usage event**:

```jsonc
{
  "ts": "2026-08-19T14:03:11.482Z",
  "tool": "claude", // plugin id
  "model": "claude-opus-4-8",
  "tokens_in": 18234,
  "tokens_out": 2101,
  "cache_read": 15000, // optional, when the tool reports it
  "cache_write": 0,
  "cost_usd": 0.0423, // tool-reported, else computed from a price table
  "project": "01J9Z…", // project id
  "ticket": "01J9ZK…", // ULID when attributable, else null
  "session": "…", // opaque, for de-dup
}
```

- **The plugin's only job** is to turn its native telemetry (Claude's OTLP stream,
  Codex's usage output, an ACP tool's counters) into these events. The host owns
  everything else. A tool with no telemetry omits the capability.
- **Cost:** prefer the tool's own reported cost; where absent, compute from a
  per-model **price table** (a small, updatable data file) when the model and
  relevant token categories have supported rates. Unknown costs remain absent.
- **Per-ticket attribution** reuses HS1's proven trick: the _active ticket_ at emit
  time (the channel/worklist knows which ticket is being worked) tags the event; a
  session with no active ticket attributes to the project only.

## 14.3 Storage — files, no DB (maintainer's design)

Two layers of files, no SQL telemetry store (this is the deliberate escape from HS1's
telemetry PGLite cluster + WAL management, docs/127):

```
<data>/metrics/
  raw/
    2026-08-19.jsonl            # rotating raw usage events, append-only, one file/day
    2026-08-20.jsonl
  rollups/
    daily/2026-08.json          # per-day totals for the month (cost/tokens by model)
    ticket/2026-Q3.json         # per-ticket cost rollup
    monthly.json                # coarse long-range series
  rollups/meta.json             # {last_rolled_up_through: "2026-08-20T00:00Z"}
```

- **Raw = rotating JSONL** (append-only, cheap, HS1's mechanism — keep it).
- **Rollups = files, written periodically in parallel** by a background job: read raw
  JSONL, aggregate, write/refresh the rollup files, advance `meta.last_rolled_up_through`.
- **Reads never touch a DB.** To answer a query (a dashboard, the cost widget):
  **read the rollup files** for settled ranges **+ live-read the raw JSONL newer than
  `last_rolled_up_through`** and aggregate that tail on the fly. Rollups are big/slow
  history precomputed; the recent tail is small enough to scan live.
- **Rebuildable:** rollups are derived — delete them and the roll-up job rebuilds from
  the raw JSONL. (Same "derived cache" principle as the ticket index.)
- **Retention:** bound the raw JSONL (age/size cap) once rolled up; rollups are small
  and kept long. (HS1 docs/85 retention, simplified.)

## 14.4 Team sharing via git

Metrics can be shared across a team **through git** — no server sync needed:

- The **rollup files** (small, aggregated) are the shareable unit; commit them into a
  **metrics area of a store** (or a dedicated metrics store) that syncs via the normal
  automatic git sync ([02](02-ticket-storage.md) §2.12). Raw JSONL stays local
  (large, per-device) unless a team opts to share it.
- **Merge:** rollup files are additive per (period, tool, model) — a semantic merge
  (sum counters, union periods) keeps two people's rollups from clobbering; or shard
  rollups **per contributor** (`rollups/daily/<user>/2026-08.json`) so they never
  conflict and a team view sums across contributors. _Recommend per-contributor
  sharding_ — it's conflict-free and attributes cost per person, which teams want.
- **Privacy:** sharing is opt-in per store; a solo/local project shares nothing.
  Decide what a rollup exposes (cost + tokens by model/period/person — no prompt
  content, ever).

## 14.5 What consumes it

- The **cost widget** + **analytics/stats dashboards** (HS2-47) read via §14.3.
- The **ticket timeline** reads one ticket's history through
  `metrics::ticket_usage` (`GET /checkouts/{checkout}/tickets/{ticket}/usage`, HS2-ZA7W6E,
  HS2-J14225): the attributed raw events in `ts` order, de-duplicated by timestamp, session,
  tool, and token counts, plus the settled rollup's `by_ticket` cost and
  `last_rolled_up_through` day, so the client can show per-phase tokens and models while raw
  events exist and still a cost total after the raw tail is pruned. Tickets on other
  providers return an empty history rather than a capability error. Raw events are
  device-local (§14.3), so per-phase figures reflect this device's recorded sessions.
- Cross-project + cross-person rollups feed a team cost view (from the shared rollups,
  §14.4).
- The production web `AIConversation` consumes the same per-turn `Usage` projection that
  feeds this interface. Each assistant turn shows input/output token totals and the dialog
  sums only usage events received for that connection; it does not maintain an independent
  estimator or poll analytics. A reported or price-table-enriched `cost_usd` is shown with
  an approximation cue and sensible sub-cent precision. When cost is absent (for example,
  a tool omits both a priceable model and reported cost), the UI says “Cost unavailable”
  rather than treating unknown as zero. This keeps the conversation truthful while the
  raw/rollup store remains authoritative for historical reporting.

## 14.6 Resolved implementation decisions

- **Price table maintenance** ships a default with a local override (HS2-8BCRHS).
- **Rollup cadence + tail size** is caller-controlled; settled reads combine the latest
  rollup with its raw tail so dashboards can tune cadence without losing events.
- **Per-contributor identity** is git email, shared with assignment.
- **Non-Claude attribution fidelity** preserves only fields each protocol reports;
  Codex/Claude/ACP mappings are version-pinned and drift-tested.

## 14.7 Build plan (follow-ups)

- HS2-69 (this) = the spec.
- **Shipped:** the raw-JSONL writer + aggregation + DB-free read (HS2-69); the price
  table (`record_priced`, HS2-8BCRHS); the **rollup files** + settled-plus-tail read
  path + retention + **per-contributor git-sharing** (`roll_up_through` /
  `summary_settled` / `prune_raw_before` / `team_summary`, exposed via
  `hotsheet-cli metrics --roll-up/--prune-before/--team`, HS2-8BCRHS).
- **Shipped (HS2-8PSAFE / HS2-0WCRZY):** the `metrics` plugin capability
  (`Manifest.metrics` = `MetricsSpec{source}`; codex opts in with `source="codex-usage"`);
  the codex mapper (`codex_turn_usage` → `drive::Usage`, lenient fallback); and the
  **end-to-end codex path** — `CodexTurn` captures usage from
  `thread/tokenUsage/updated`, the drive surfaces it as `TurnEvent::Usage`, and the server
  driving loop records it per turn via `record_priced`
  attributed to the claimed ticket + session. **Claude** (HS2-TJ8FGR) is now mapped the
  same way — `claude_result_usage` reads the in-band stream-json `result` event's `usage`
  (declared `[metrics] source="claude-usage"`), `ClaudeTurn::usage()` surfaces it, and it
  flows through `TurnEvent::Usage` + the driving-loop recorder unchanged.
- **Client conversation projection (HS2-BXSPMN):** client-owned turns now pass usage through
  the same store price table before both recording and live projection. Their stable
  connection id is the metrics session key, so transcript totals and historical raw/rollup
  reads share one attribution boundary; no browser-side pricing table or counter exists.
  When that connection's session worker holds exactly one live ticket claim at turn start,
  the event also names that ticket (HS2-J14225). Ambiguous or expired claims leave usage
  project-only rather than assigning it to the wrong ticket. If a plugin omits the model
  from its telemetry, an explicitly selected turn/connection model labels that event.
- **Interactive Codex terminals (HS2-J14225):** the existing read-only daemon thread watch
  takes the last `thread/tokenUsage/updated` count for each completed turn and records one
  usage event. It snapshots the terminal session's unique live ticket claim at turn start.
  The resumed thread supplies the model when available; an unknown model remains unknown.
  Codex sessions without the shared daemon, and telemetry from before this capture path,
  cannot be attributed by this path. Raw events already recorded without a ticket are not
  retroactively assigned because their work cannot be identified reliably.
  **Interactive Claude terminals (HS2-W2GNTT):** the documented `UserPromptSubmit`,
  `Stop`, and `StopFailure` hooks provide a session id and transcript path, but no token
  counters. The server snapshots the transcript byte offset and unique live claim of
  the terminal's Claude or shell worker at prompt time. At turn end it reads only the
  appended JSONL records, sums numeric assistant usage once per message id, and records
  one priced event. Duplicate stops and historical records cannot add usage; an
  ambiguous or later-acquired claim leaves the event project-only. The parser skips
  subagent, malformed, and unknown records and bounds each scan to 64 MiB. The first
  prompt can precede transcript creation, so its cursor starts at byte zero; `Stop`
  waits up to one second for a late append. Turn hooks omit the matcher key so Claude
  2.1.308 invokes them. Claude's transcript row shape is observed locally rather than
  a published hook contract, so a changed format can omit usage until the parser is
  updated. No prompt or response text is sent to the server or copied into metrics.
  For `claude-fable-5-1`, the published model rate is $10/$50 per million ordinary
  input/output tokens. The transcript path prices cache reads at $0.25, five-minute
  writes at $12.50, and one-hour writes at $20 per million when its usage row names
  the cache-write TTL. Unclassified cache writes are not guessed. A store's committed
  `metrics/prices.json` can override these rates. Source:
  [Claude Fable 5.1 model pricing](https://platform.claude.com/docs/en/models/fable-5-1/overview).
  Codex Work sessions outside Hot Sheet drives/terminals have a local claim-window
  capture path (`HS2-2K2D7E`, `HS2-NFQ1NK`). CLI and MCP renewal and release capture
  usage; `hotsheet-cli metrics --refresh-codex` explicitly reads new usage while a
  claim is active. It binds this process's exact `CODEX_SESSION_ID`,
  `HOTSHEET_WORKER_ID`, and project store, verifies the matching
  rollout metadata, and reads only numeric `token_usage_record` and model-bearing
  `turn_context` fields. A response is attributed only when its timestamp falls in
  exactly one unexpired claim interval for that worker; overlapping or unclaimed
  work remains unassigned. Response IDs deduplicate repeated captures, and an
  already watched terminal session is skipped. A machine-local, gitignored response-ID
  checkpoint keeps repeated captures idempotent after raw metrics are pruned. The
  raw rollout's prompts and tool content are neither copied into metrics nor logged.
  This local format is observed from installed Codex sessions rather than a stable
  public export contract, so unrecognized records fail closed. Usage becomes visible during a live claim after
  renewal or explicit refresh; opening the Timeline reads the latest recorded usage.
  Historical unattributed raw metrics are never reassigned.
- **Live-verified + hardened (HS2-CQ6B96):** Codex 0.148 emits per-turn camelCase counts
  under `thread/tokenUsage/updated.params.tokenUsage.last`; `inputTokens` already includes
  its `cachedInputTokens` subset (`totalTokens == inputTokens + outputTokens`), so cached
  tokens must **not** be added again. That event currently omits model and cost. Claude
  2.1.241 emits `input_tokens`, `cache_read_input_tokens`,
  `cache_creation_input_tokens`, and `output_tokens` in the stream-json result; those three
  input classes are summed, and top-level `total_cost_usd` (session-cumulative, so a resumed
  turn subtracts its recorded baseline, HS2-4JPDJA, docs/13 §13.6) is carried through
  `drive::Usage.cost_usd` (the live result omitted model). Both live tests require positive
  parsed input/output counts, so future silent schema drift fails the gated tier.
- **ACP/OpenCode (HS2-96BZEF):** `acp_usage` maps only the standard
  `PromptResponse.usage` input/output counters (snake_case protocol or camelCase SDK
  spelling) into `drive::Usage`; OpenCode declares `[metrics] source="acp"`. ACP does
  not guarantee model, cache, or cost attribution, so those fields remain absent rather
  than inferred. The live stdio client, fast ACP contract cassette, and gated OpenCode
  smoke test now provide the drift oracle (HS2-PEQ6Q8).
- **Remaining:** wiring the dashboards / cost widget (HS2-47, client).

## 14.8 Cross-references

- The `metrics` plugin capability: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.3
- Dashboards that consume it: docs/11 area 25 (HS2-47)
- Git sharing rides the sync engine: [02-ticket-storage.md](02-ticket-storage.md) §2.12
