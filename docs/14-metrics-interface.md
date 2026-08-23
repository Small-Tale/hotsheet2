# 14. Usage/Cost Metrics Interface

> **Status: Design (HS2-69).** A **unified usage/cost metrics interface** every AI
> tool conforms to, based on what HS1 actually *showed* in the UI — **not** the HS1
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
  "tool": "claude",              // plugin id
  "model": "claude-opus-4-8",
  "tokens_in": 18234,
  "tokens_out": 2101,
  "cache_read": 15000,           // optional, when the tool reports it
  "cache_write": 0,
  "cost_usd": 0.0423,            // tool-reported, else computed from a price table
  "project": "01J9Z…",          // project id
  "ticket": "01J9ZK…",          // ULID when attributable, else null
  "session": "…"                // opaque, for de-dup
}
```

- **The plugin's only job** is to turn its native telemetry (Claude's OTLP stream,
  Codex's usage output, an ACP tool's counters) into these events. The host owns
  everything else. A tool with no telemetry omits the capability.
- **Cost:** prefer the tool's own reported cost; where absent, compute from a
  per-model **price table** (a small, updatable data file) so cost is always present.
- **Per-ticket attribution** reuses HS1's proven trick: the *active ticket* at emit
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
  conflict and a team view sums across contributors. *Recommend per-contributor
  sharding* — it's conflict-free and attributes cost per person, which teams want.
- **Privacy:** sharing is opt-in per store; a solo/local project shares nothing.
  Decide what a rollup exposes (cost + tokens by model/period/person — no prompt
  content, ever).

## 14.5 What consumes it

- The **cost widget** + **analytics/stats dashboards** (HS2-47) read via §14.3.
- Cross-project + cross-person rollups feed a team cost view (from the shared rollups,
  §14.4).

## 14.6 Open questions
- **Price table maintenance** — where the per-model prices live and how they update
  (ship a default, allow override; the `claude-api` skill is the source of truth for
  Anthropic pricing).
- **Rollup cadence + tail size** — how often to roll up vs. how much raw JSONL a live
  read scans; tune for dashboard latency.
- **Per-contributor identity** for shared rollups = the git email (same as assignment,
  [10](10-assignment-and-collaboration.md)).
- **Non-Claude attribution fidelity** — Codex/ACP tools may report usage more coarsely;
  attribute what they give, don't fabricate.

## 14.7 Build plan (follow-ups)
- HS2-69 (this) = the spec.
- **Shipped:** the raw-JSONL writer + aggregation + DB-free read (HS2-69); the price
  table (`record_priced`, HS2-8BCRHS); the **rollup files** + settled-plus-tail read
  path + retention + **per-contributor git-sharing** (`roll_up_through` /
  `summary_settled` / `prune_raw_before` / `team_summary`, exposed via
  `hotsheet-cli metrics --roll-up/--prune-before/--team`, HS2-8BCRHS).
- **Remaining:** the `metrics` plugin capability + per-tool telemetry mappers (Claude
  OTLP / Codex usage → `UsageEvent`, needs live telemetry formats + drive integration);
  wiring the dashboards / cost widget (HS2-47, client).

## 14.8 Cross-references
- The `metrics` plugin capability: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.3
- Dashboards that consume it: docs/11 area 25 (HS2-47)
- Git sharing rides the sync engine: [02-ticket-storage.md](02-ticket-storage.md) §2.12
