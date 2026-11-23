# Development and testing strategies

HS2-BF79YX defines one versioned project policy in `.hotsheet2/settings.json`, under `development_strategy`. It is shared by every client of that checkout. The setting is independent of browser storage and machine-local settings. An unset policy preserves the existing integration-policy default and risk-tier local gate behavior, so upgrading does not silently change a project. Its effective report says `source: unconfigured-project` and `local_gate: risk-selected`; integration is the existing `integration_strategies.default` (`serial` unless separately configured). Explicitly setting Ticket Assured instead says `source: project`, `integration: serial`, and `local_gate: full-local`. A newly selected policy applies to future work; an active integrator generation retains the strategy it registered with.

| Preset                            | Use case                                   | Integration                       | Local verification                                                                          | Main boundary                                                        |
| --------------------------------- | ------------------------------------------ | --------------------------------- | ------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| Ticket Assured (`ticket-assured`) | Highest per-ticket confidence              | `serial` when explicitly selected | Full local gate when explicitly selected; existing risk-tier selection for an unset project | Push a verified ticket, then use its covering CI run                 |
| Steady Main (`steady-main`)       | Targeted local work with a green main goal | `ci-staged`                       | Targeted affected checks, preserving mandatory T2 selection                                 | Promote only an exact green staged SHA; goal defaults to 360 minutes |
| Risk Guided (`risk-guided`)       | Select checks by ticket risk while staging | `ci-staged`                       | Existing T0/T1/T2 risk and explicit testing tags                                            | Promote only an exact green staged SHA                               |
| CI First (`ci-first`)             | Keep local load low; CI batches pushes     | `serial`                          | Best effort: affected checks within a strict budget (10 minutes by default), every tier     | Push to main without a full-gate receipt; CI is the gate             |

The green-main goal is an operational target, not a timer that promotes red or untested code. Every preset except CI First requires an exact green SHA before main promotion, and every preset requires attribution of red CI before ticket completion. A preset cannot disable a required risk-tier, browser, platform, or native check. HS2-GT57BQ defines the staged batch and promotion mechanics; HS2-SVD5WA governs required local verification and recovery. Release Window is separate work in HS2-RCKZSE.

HS2-0BJ1A3 projects the Steady Main goal from the durable staged-batch ledger. `coordination batch-status` reports `cadence.goal_minutes`, the last green main promotion and next target as Unix seconds, the oldest staged batch and age, CI failures and correction wall time since the last promotion, tickets promoted in the rolling goal interval ending now, a green CI candidate, and an action or missed-goal reason. A green candidate still requires the promotion wrapper's current-main and fast-forward checks; status alone does not prove it can move main. The Workers view shows the same source-scoped goal, last promotion, next target, and staged action. A policy-only goal change advances its event cursor; the app compares target time locally when rendering, without periodic network requests. A strategy write through the server (`PUT .../development-strategy` or `.../integration-strategies`) also publishes a `development_strategy_changed` change event for the checkout, so a waiting Workers long poll reads again at once instead of waiting for the platform file watcher to report the settings file, which a busy machine delivers late (HS2-A8HP69). Before the first recorded promotion, last promotion and target are unknown, so there is no missed-goal reason yet. A missed target prompts attribution, re-staging, or promotion of an already green exact SHA; it never weakens the fast-forward, CI, or ticket-completion fence. CI lookup outage reports `ci_unavailable` and leaves the ledger and main unchanged; the durable status groups a pending or unavailable lookup as `staging_ci_pending_or_unavailable` because it cannot infer the transient outage from ledger history alone. Ongoing claims and staged work continue while CI runs. Release-train scheduling remains HS2-RCKZSE.

## CI First (HS2-ZGS7JP)

CI First is for a project that would rather let CI batch the full suite than tax the developer's machine. It is selectable by any project with `development-strategy-set --preset ci-first [--best-effort-budget-minutes <1-1440>]`, the REST policy field `best_effort_budget_minutes`, or Settings › Development strategy. Its effective selection reports `integration: serial`, `local_gate: best-effort`, `local_budget_minutes` (10 unless configured), `push_without_receipt: true` and `require_green_main_sha: false`; `attribute_red_ci_before_completion` stays true.

- **Local verification is best effort.** `scripts/check.sh` plans the affected checks within the budget for every tier, T2 included: no analyzed `--t2-stage` selection and no reviewed `--test-plan` is required, although both are still accepted (an analyzed stage becomes an additive request). A ticket's `test-budget:<duration>` tag can only shorten the budget. The gate starts stages in plan order and stops starting new ones once the budget has elapsed; an unstarted stage is deferred to CI, listed in the output, and recorded in the receipt's `bestEffort` block (`ran`, `plannedDeferred`, `runtimeDeferred`, `budgetMs`, `actualMs`) and `deferred` list. A stage that ran and failed still fails the gate. A rerun on the same inputs reuses the passed stages and spends a fresh budget on the deferred ones.
- **The push goes straight to main.** `scripts/push.sh` resolves the effective strategy with the same ticket, session and project precedence as the gate. Only when it is `ci-first` does an ordinary main push proceed without an exact-tree receipt; the reservation records this so the pre-push hook does not demand one either. The clean-checkout, fast-forward, main-writer reservation, coordination-pool and remote-confirmation checks are unchanged, and `--standalone` is still required beside an existing coordination pool without a live integrator. Every other preset, and a strategy the push cannot resolve, keeps the receipt requirement. This makes the `--standalone --skip-tests` override a policy-backed normal path instead of a per-push exception.
- **CI is the gate.** After the push the ticket enters `final_testing`, and the covering main CI run (`node scripts/ci-gate.mjs status --covers <sha>`, through the HS2-3R9V0R watch rather than short-interval polling) decides completion. A red run is attributed to the tickets pushed since the last green run (docs/33), repaired forward, and reported to the maintainer in a ticket note and the session. Building that attribution manifest from the pushed commit range is HS2-H21PJS; falling back to a stricter local gate after repeated red runs is HS2-KW2R9T.

## Selection and precedence

The built-in policy contains `version: 1`, `default`, optional `green_main_goal_minutes` (1–10,080; default 360), and optional `best_effort_budget_minutes` (1–1,440; default 10, used only by CI First). `default` is one of the four built-in names above. A version 2 policy may also contain a map of up to 20 named custom strategies and `default_custom`. An explicit ticket instruction takes precedence over an explicit session instruction, which takes precedence over the project default. The local gate recognizes `development-strategy:<name>` on a ticket (ticket tag autocomplete offers that tag for each built-in and each of the project's custom strategies, HS2-PN74AH) and `HOTSHEET_DEVELOPMENT_STRATEGY=<name>` for the session. `coordination development-strategy-resolve --session <name> --ticket <name>` previews the same precedence in the headless CLI. The selected integration strategy must already be allowed by the project's existing `integration_strategies` policy: `serial` is always allowed; staged and batch strategies require a reviewed `coordination strategy-allow --strategy <name> --reason <why>` first. A conflict fails explicitly and does not rewrite either policy.

Read the policy with `hotsheet-cli coordination --project <checkout> development-strategies`, or `GET /checkouts/{reference}/development-strategy`. The GET route also accepts optional `session` and `ticket` named-strategy query parameters to preview the effective selection and its `source` without changing the saved policy; ticket takes precedence. An unknown name is refused. Set it with `hotsheet-cli coordination --project <checkout> development-strategy-set --preset <name> [--green-main-goal-minutes <minutes>] [--best-effort-budget-minutes <minutes>]`, or authenticated `PUT /checkouts/{reference}/development-strategy` with the versioned policy object. Both paths use the same Rust validation and shared settings write. A new integrator can select a session override with `register --development-strategy <name>` (or `next` with the same option). A separate `--strategy` must agree with that preset; a currently registered generation keeps its recorded integration strategy. The REST route is owner-authenticated by the server's usual checkout boundary. The GUI editor (HS2-NBDXPW) calls this route, shows a Saved policy summary of the project's resolved policy, and shows the live source-scoped cadence from the Workers stream. It has no session/ticket override preview (HS2-MN0MXT).

Example shared value:

```json
{
  "development_strategy": {
    "version": 1,
    "default": "steady-main",
    "green_main_goal_minutes": 360
  }
}
```

The headless update rejects unsupported versions, unknown names, invalid green-main goals, and a staged preset when `ci-staged` is not permitted. The underlying integration-policy allowlist remains the authority for whether a new integrator may register on `ci-staged`. Changing a default never mutates a live coordinator session or retrospectively completes a ticket. Cadence automation (HS2-0BJ1A3), skill setup (HS2-F9062F), and GUI selection (HS2-NBDXPW) share this policy. No customer content is stored in it.

## Named custom strategies (HS2-ARJNJV)

Version 2 adds a validated `custom` map and an optional `default_custom` selector. A custom name is lower-case kebab-case and cannot collide with a built-in name. A custom strategy cannot select the `best-effort` local gate, which belongs to CI First alone. CLI `development-custom-set --name <name> --file <json> [--select-default]` validates and stores one definition; `development-custom-select --name <name>` selects an existing definition. `development-strategy-set --preset <built-in>` clears `default_custom` for a safe rollback, retaining the custom definitions. Session and ticket overrides use the same names. `development-strategies` and the authenticated REST report include the effective name, source, and resolved custom fields. Version 1 projects keep their existing integration default and risk-tier local checks until explicitly changed.

The only connected CI gate supported by this checkout is `CI`; an unrecognized check name fails validation. A custom policy requires the project's checked-in `.github/workflows/ci.yml` named `CI` and a configured `origin` remote. `ci-staged` requires an allowed strategy and its exact configured staging branch; that scratch branch need not exist before the integrator creates it. `batch` likewise requires an allowed strategy. Red CI still needs attribution before completion. Serial policies require `attribute-and-retry`; batch and ci-staged require `fallback-to-serial`, matching their existing circuit breaker. Unsupported combinations are refused. Cadence automation for that recovery is HS2-0BJ1A3. `test_plan_timing` declares whether the plan is required before work or before handoff; HS2-F9062F owns installing that instruction into agent workflows. The green-main goal is a target, never permission to promote a red SHA. Custom checks can raise a ticket's minimum tier, select browser/platform/performance coverage, and cap T0/T1 planning time; T2 and full-local checks cannot have a time budget. Required affected checks always run. An AI agent can add a registered affected check with `scripts/check.sh --propose-stage '<cwd>::<command>'`; the gate rejects unknown or unaffected commands and includes the proposal in the exact receipt. A proposal cannot remove mandatory checks.

Strict serial example:

```json
{
  "version": 2,
  "default": "ticket-assured",
  "default_custom": "strict-review",
  "custom": {
    "strict-review": {
      "integration": "serial",
      "local_gate": "full-local",
      "minimum_tier": "T2",
      "browser": true,
      "platform": true,
      "perf": false,
      "test_plan_timing": "before-work",
      "ci_gate": "CI",
      "red_ci_recovery": "attribute-and-retry"
    }
  }
}
```

Fast reviewed batch example (requires `batch` in `integration_strategies.allowed`):

```json
{
  "version": 2,
  "default": "ticket-assured",
  "default_custom": "fast-reviewed",
  "custom": {
    "fast-reviewed": {
      "integration": "batch",
      "local_gate": "risk-selected",
      "minimum_tier": "T1",
      "budget_minutes": 5,
      "browser": false,
      "platform": false,
      "perf": false,
      "test_plan_timing": "before-handoff",
      "ci_gate": "CI",
      "green_main_goal_minutes": 180,
      "red_ci_recovery": "fallback-to-serial"
    }
  }
}
```
