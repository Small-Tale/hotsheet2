---
name: security-review
description: Re-analyze the project's attack surface at release time or on demand: re-derive every externally reachable surface from the code, check each for authentication, authorization, validation, injection, traversal, SSRF, secret leakage, CSRF, transport, and supply-chain issues, research current advisories for the exact dependency versions, write a dated severity-ranked report, and file remediation tickets.
allowed-tools: Read, Grep, Glob, Bash, WebSearch, WebFetch, Agent, Write
---

<!-- ai-playbook:skill name=security-review v=1 -->

Comprehensively re-analyze the project's **attack surface** and produce a dated,
severity-ranked security report. This is the standing "revisit the security analysis on every
release" mechanism. Treat any project that executes commands, holds tokens or keychain
secrets, serves files, or can be reached off-box as a high-value target.

**Two non-negotiables that make this skill worth running:**

1. **Enumerate the surface from the code, not from a list.** The real surface is whatever the
   current code exposes. Re-derive it every run: new routes, socket endpoints, tool surfaces,
   plugins, and native commands appear between releases.
2. **Do proactive external research every run.** Do not rely on training knowledge; fetch the
   latest advisories for the actual dependency versions and recent attack-pattern write-ups.

## Step 1: re-derive the attack surface from the code

Start from the codebase map's entry points, then sweep. Adjust the patterns to the frameworks
in use:

- **HTTP routes**: route registrations (`.route(`, `.get(`, `.post(`, router decorators,
  `app.use(`) across the server source; list every path and method.
- **Socket and upgrade endpoints**: WebSocket handlers, `on('upgrade')`, long-poll channels.
- **Endpoints outside the auth middleware** by design (telemetry ingest, health, webhooks):
  list them explicitly.
- **AI tool surfaces**: every MCP tool, agent command, or channel an AI agent can call.
- **Plugin and integration endpoints**: settings that hold long-lived tokens; server-side
  fetches of user- or plugin-supplied URLs.
- **File serving and uploads**: attachment routes, static hosting, any path joined from
  request input.
- **Secrets**: keychain access, secret-key files, configuration files holding credentials.
- **Exposure model**: bind address, trusted origins, auth middleware tiers, TLS or tunnel
  assumptions.
- **Native and shell surfaces**: desktop-shell commands the webview can invoke, CLI
  subcommands that run shell commands, terminal brokers.

Record the inventory as the report's first section and **diff it against the previous report**
(look for prior reports in the project's security directory) to flag what is new or removed.

## Step 2: check each surface

For every surface, ask:

- **Authentication before execution**: is the request authenticated before any handler runs,
  and at which tier (loopback-only, shared secret, trusted origin, mutual TLS)? Is any path
  open on an exposed server?
- **Authorization**: once authenticated, is the action's scope checked (cross-project access,
  management endpoints)?
- **Input validation and abuse pre-filtering**: body-size caps before buffering, rate limits
  on an exposed server, content-type enforcement, bounded schemas (grep for unbounded string
  and array validators an attacker could use to balloon memory or storage), per-request caps
  on ingest.
- **Injection**: parameterized queries only (grep for template literals or string
  concatenation inside query calls); a typed program with arguments for every command
  execution.
- **Path traversal**: normalization and containment on every path built from input; confirm
  the existing tests still hold.
- **SSRF**: every server-side fetch of a supplied URL (image proxies, webhooks, plugin
  fetches).
- **Secret leakage**: secrets in logs, error bodies, URLs (query parameters appear in access
  logs), socket connect URLs, exports, startup logs.
- **CSRF and cross-site socket hijacking**: same-origin or trusted-origin checks on mutations
  and upgrades; a malicious page must not forge an authenticated request or socket.
- **Transport**: what is plaintext versus TLS or tunnel-protected.
- **Dependencies and supply chain**: the ecosystem's audit command (`npm audit`,
  `cargo audit`, `pip-audit`, `govulncheck`) and the project's dependency-security document.

Spawn parallel sub-audits per surface group when the surface is large, one agent per route
group, socket endpoint, or tool surface, each returning structured findings.

## Step 3: proactive external research (required every run)

Read the exact dependency versions from the lockfiles first. Then research:

- advisories and CVEs for the actual versions of the runtime, web framework, socket library,
  database or storage engine, validation library, AI SDKs, desktop-shell crates, and each
  bundled plugin's dependencies; the audit commands give a machine baseline, and web research
  catches what the databases have not ingested plus framework-specific advisories;
- framework-specific attack patterns published since the last review (middleware bypasses,
  socket denial of service, TLS pitfalls);
- the current OWASP Top 10 and API Security Top 10 and any recent high-signal write-ups
  relevant to the surfaces above.

Cite each source (URL and date). **Adversarially verify** anything that would become a finding;
do not report an advisory that does not apply to how the project uses the dependency.

## Step 4: write the dated report and file tickets

Write `docs/security/security-review-YYYY-MM-DD.md` (create the directory if absent, or use
the location the docs index names):

- **TL;DR** verdict first: clean, N findings, M critical.
- **Surface inventory** and the diff against the previous report.
- **Findings**, each with title, surface, severity (critical, high, medium, low), description,
  the concrete attack (the request an attacker sends and what they get), evidence as file and
  line, recommended remediation, and any cited advisory.
- **Dependency posture**: audit summaries plus researched advisories.
- **Sign-off**: what was checked, what was deferred, open questions.

File a Hot Sheet ticket per actionable finding, tagged `security`, cross-referencing the
report. Keep the dependency-security document in sync.

## Step 5: wire into the release flow

Confirm the release script or checklist references this review (the `release-scripts` rule
describes the one resumable release command). If it does not, record that as a finding so the
standing mechanism actually fires. Do not edit the release pipeline without surfacing it; the
maintainer owns it.

## Notes

- This skill **reports and files tickets**; it does not fix. Remediation is separate tickets
  so each fix gets its own review.
- Be adversarial and concrete. A finding is "here is the request and here is what the attacker
  gets", not "consider hardening X".
- Never paste customer content or real secrets into the report or tickets (see the
  `customer-data-hygiene` rule).
