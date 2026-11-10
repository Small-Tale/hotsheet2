# Development and testing strategies

HS2-BF79YX defines one versioned project policy in `.hotsheet2/settings.json`, under `development_strategy`. It is shared by every client of that checkout. The setting is independent of browser storage and machine-local settings. An unset policy preserves the existing integration-policy default and risk-tier local gate behavior, so upgrading does not silently change a project. Its effective report says `source: unconfigured-project` and `local_gate: risk-selected`; integration is the existing `integration_strategies.default` (`serial` unless separately configured). Explicitly setting Ticket Assured instead says `source: project`, `integration: serial`, and `local_gate: full-local`. A newly selected policy applies to future work; an active integrator generation retains the strategy it registered with.

| Preset                            | Use case                                   | Integration                       | Local verification                                                                          | Main boundary                                                        |
| --------------------------------- | ------------------------------------------ | --------------------------------- | ------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| Ticket Assured (`ticket-assured`) | Highest per-ticket confidence              | `serial` when explicitly selected | Full local gate when explicitly selected; existing risk-tier selection for an unset project | Push a verified ticket, then use its covering CI run                 |
| Steady Main (`steady-main`)       | Targeted local work with a green main goal | `ci-staged`                       | Targeted affected checks, preserving mandatory T2 selection                                 | Promote only an exact green staged SHA; goal defaults to 360 minutes |
| Risk Guided (`risk-guided`)       | Select checks by ticket risk while staging | `ci-staged`                       | Existing T0/T1/T2 risk and explicit testing tags                                            | Promote only an exact green staged SHA                               |

The green-main goal is an operational target, not a timer that promotes red or untested code. Every preset requires an exact green SHA before main promotion and attribution of red CI before ticket completion. A preset cannot disable a required risk-tier, browser, platform, or native check. HS2-GT57BQ defines the staged batch and promotion mechanics; HS2-SVD5WA governs required local verification and recovery. Release Window is separate work in HS2-RCKZSE.

## Selection and precedence

The shared policy contains `version: 1`, `default`, and optional `green_main_goal_minutes` (1–10,080; default 360). `default` is one of the three names above. An explicit ticket instruction takes precedence over an explicit session instruction, which takes precedence over the project default. The local gate recognizes `development-strategy:<name>` on a ticket and `HOTSHEET_DEVELOPMENT_STRATEGY=<name>` for the session. `coordination development-strategy-resolve --session <name> --ticket <name>` previews the same precedence in the headless CLI. The selected integration strategy must already be allowed by the project's existing `integration_strategies` policy: `serial` is always allowed; Steady Main and Risk Guided require a reviewed `coordination strategy-allow --strategy ci-staged --reason <why>` first. A conflict fails explicitly and does not rewrite either policy.

Read the policy with `hotsheet-cli coordination --project <checkout> development-strategies`, or `GET /checkouts/{reference}/development-strategy`. Set it with `hotsheet-cli coordination --project <checkout> development-strategy-set --preset <name> [--green-main-goal-minutes <minutes>]`, or authenticated `PUT /checkouts/{reference}/development-strategy` with the versioned policy object. Both paths use the same Rust validation and shared settings write. A new integrator can select a session override with `register --development-strategy <name>` (or `next` with the same option). A separate `--strategy` must agree with that preset; a currently registered generation keeps its recorded integration strategy. The REST route is owner-authenticated by the server's usual checkout boundary. The GUI editor is HS2-NBDXPW and must call this route rather than introduce another policy store.

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

The headless update rejects unsupported versions, unknown names, invalid green-main goals, and a staged preset when `ci-staged` is not permitted. The underlying integration-policy allowlist remains the authority for whether a new integrator may register on `ci-staged`. Changing a default never mutates a live coordinator session or retrospectively completes a ticket. The versioned `DevelopmentPolicy` model and typed preset registry are the extension points for HS2-ARJNJV's reviewed custom policies; this version has no custom fields. Cadence automation (HS2-0BJ1A3), skill setup (HS2-F9062F), and GUI selection (HS2-NBDXPW) remain separate follow-ups; these presets do not claim those capabilities. No customer content is stored in this policy.
