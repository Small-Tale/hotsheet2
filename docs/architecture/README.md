# Hot Sheet 2 architecture baseline

**Audited source** [`8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774` on origin/main](https://github.com/Small-Tale/hotsheet2/commit/8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774), 2026-11-26.
Every claim in this directory describes that revision. The review branch
`docs/hs2-3fa79j-structurizr-baseline` merges exactly that commit, so its
working tree is the audited code plus these documents. The first, shallower
baseline audited `52010ebaf629b1ed95850cf4bb64b1385a03e6e4` (HS2-3FA79J). The
deep baseline re-pins all areas to one revision (HS2-H0T69J).

Goal: **a developer who reads these documents and then opens the code is not
surprised** by any module, type, state, side effect, lock, failure path,
environment variable, route or file format.

## Where to start

The viewer opens on `HS2-00-Overview`, a ten-box conceptual map whose boxes
double-click into the detailed views. The diagram list follows a guided tour
(overview, system, each area, then flows), and the Documentation tab's
**Start here** chapter walks the same layers with embedded diagrams
(HS2-DHCY18). The table below indexes the detailed views by question.

| Question                                                                  | Structurizr view                                                                                                                                                                                                                                                  |
| ------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Who interacts with Hot Sheet, and which systems does it depend on?        | `HS2-Context`                                                                                                                                                                                                                                                     |
| What runs on a machine, and which processes share a ticket store?         | `HS2-WorkspaceRuntime`, `HS2-DesktopDeployment`, `HS2-HeadlessDeployment`                                                                                                                                                                                         |
| Which records are authoritative, and how does a write reach open clients? | `HS2-ProviderBoundaries`, `HS2-TicketComponents`, `HS2-WebGitTicketWrite`, `HS2-NativeIssueWrite`, `HS2-GitReconciliation`, `HS2-HeadlessTicketWrite`, `HS2-Flow-TicketEdit`, `HS2-Flow-OutOfBandEdit`, `HS2-Flow-NativeProviderWrite`, `HS2-Flow-ReplayOverflow` |
| What is inside each area?                                                 | `HS2-Core-TicketEngine`, `HS2-Orch-Placement`, `HS2-Cli-CommandTree`, `HS2-Server-Admission`, `HS2-WebHost-BridgeComponents`, `HS2-WebClient-Core` and the other area component views                                                                             |
| Where do browser state, live updates, and terminal input live?            | `HS2-WebComponents`, `HS2-ExecutionComponents`, `HS2-UnixTerminal`, `HS2-WebClient-LiveData`, `HS2-WebClient-Editing`, `HS2-Server-Execution`, `HS2-Flow-AiSession`, `HS2-Flow-AiPermission`                                                                      |
| How do claims, schedules, placement and CI gates fit together?            | `HS2-Flow-ClaimLifecycle`, `HS2-Flow-ClaimContention`, `HS2-Flow-ScheduledCommand`, `HS2-Flow-PlacementToGate`, `HS2-ScheduledCommand`, `HS2-HostSupport`, `HS2-OrchestrationComponents`, `HS2-OrchestrationPlacement`, `HS2-Orch-FailedCiGate`                   |
| What happens at desktop start and quit, and what survives?                | `HS2-Flow-DesktopColdStart`, `HS2-Flow-DesktopShutdown`, `HS2-Server-BrokerReattach`                                                                                                                                                                              |
| How do remote devices and remote projects work?                           | `HS2-Flow-RemoteDevice`, `HS2-Flow-RemoteProject`, `HS2-RemoteDeployment`                                                                                                                                                                                         |
| Does the headless path match the GUI for setup and migration?             | `HS2-Flow-FreshInstallGui`, `HS2-Flow-FreshInstallHeadless`, `HS2-Flow-Migration`, `HS2-LegacyImport`, `HS2-Cli-Bootstrap`, `HS2-Cli-Migrate`                                                                                                                     |
| How does the optional macOS screen saver fit?                             | `HS2-MigrationAndStandby`, `HS2-Server-Saver`                                                                                                                                                                                                                     |

Each area has a narrative chapter and a coverage file:

| Area                                            | Chapter                                           | Coverage                                   | Ticket     |
| ----------------------------------------------- | ------------------------------------------------- | ------------------------------------------ | ---------- |
| Boundaries overview                             | [01-boundaries](narrative/01-boundaries.md)       | [overview](coverage/overview.md)           | HS2-3FA79J |
| Core ticketing crates                           | [10-core](narrative/10-core.md)                   | [core](coverage/core.md)                   | HS2-AH9HAK |
| Orchestration, coordination, resources and jobs | [20-orchestration](narrative/20-orchestration.md) | [orchestration](coverage/orchestration.md) | HS2-RJB111 |
| CLI and MCP shim                                | [30-cli](narrative/30-cli.md)                     | [cli](coverage/cli.md)                     | HS2-TW2DKM |
| Rust server, terminals, AI drive, notifications | [40-server](narrative/40-server.md)               | [server](coverage/server.md)               | HS2-P27EJM |
| Node web host and Tauri shell                   | [50-web-host](narrative/50-web-host.md)           | [web-host](coverage/web-host.md)           | HS2-T6SD67 |
| Browser web client                              | [60-web-client](narrative/60-web-client.md)       | [web-client](coverage/web-client.md)       | HS2-AYKHVV |
| End-to-end flows, failure and recovery          | [70-flows](narrative/70-flows.md)                 | (cites the area files)                     | HS2-M81CCP |

[Unresolved claims](unresolved.md) lists what could not be verified. The
[architecture overview](../01-architecture.md) is the prose entry point. The
[requirements index](../README.md) tracks design intent and delivery status.
The [codebase map](../CODEBASE-MAP.md) locates modules.

## Open or validate the workspace

Run `npm run view-architecture-docs` from the repository root. It finds a JDK
21 or newer (`JAVA_HOME`, Homebrew's keg-only `openjdk`, or `java` on `PATH`),
builds `structurizr.war` from the pinned tag once into
`~/.cache/hotsheet2/structurizr/`, validates the workspace, and serves a
scratch copy at http://localhost:8080 (set `PORT` to change it). Flags:
`--validate-only`, `--no-validate`, `--rebuild` (HS2-38CA53). The manual
steps it automates are below.

Use the consolidated [Structurizr application release v2026.10.26](https://github.com/structurizr/structurizr/releases/tag/v2026.10.26).
The release publishes no prebuilt archive; build `structurizr.war` from that
tag with its Maven wrapper and a JDK 21 or newer. Then run from the
repository root:

```sh
java -jar /path/to/structurizr.war validate -workspace docs/architecture/workspace.dsl
java -jar /path/to/structurizr.war inspect -workspace docs/architecture/workspace.dsl
node scripts/architecture-coverage.mjs check
```

For local viewing, copy `docs/architecture` to a scratch directory and pass the
copy to `java -jar /path/to/structurizr.war local /path/to/copy`. Open the
localhost URL printed by the command. Scratch copies keep viewer-generated
layout files out of the model history.
[Structurizr command documentation](https://docs.structurizr.com/commands)

Regenerate the client screenshots in `narrative/screenshots/` from synthetic
data with the opt-in spec (HS2-DHCY18), after `npm run test:fixtures` when the
fixture binaries are stale:

```sh
cd clients/web
HS2_ARCHITECTURE_SCREENSHOTS=1 npx playwright test tests/architecture-screenshots.spec.ts --workers=1
```

Then update `views/screenshots.dsl` from `screenshots/manifest.json` when a
capture is added or renamed.

`node scripts/architecture-coverage.mjs inventory --area <key>` lists every
required entry for an area and whether its coverage file maps it. `check`
also resolves every `path:line` citation under `docs/architecture` against
the working tree. Area keys and source roots are in [areas.json](areas.json).

## Depth standard

These rules set the granularity for every area (HS2-H0T69J).

1. **Every runtime container has components.** Rust crates that are linked
   rather than run are modeled as the library containers `coreLib` and
   `orchLib`, which the server, CLI and MCP link. This is a deliberate
   deviation from strict C4, where libraries are not containers. It lets
   one shared component be described once instead of in every host.
2. **Every large module is accounted for.** Each non-test source file of
   500 lines or more, and each directory whose non-test source totals 500
   lines or more, in an in-scope crate or package is either a component or
   is named in that component's coverage row. Every entry under
   `crates/hotsheet-server/src/routes` and `crates/hotsheet-cli/src/dispatch`
   is named regardless of size. `scripts/architecture-coverage.mjs` computes
   the list; the [areas.json](areas.json) roots decide area ownership.
3. **Small modules are grouped.** Modules below the threshold are listed by
   name in the row of the component that owns them. Do not leave a module
   only implied by a directory name when its behavior would surprise a reader.
4. **Each component records**, in its coverage row or chapter section:
   - responsibility: what it does and what it deliberately does not do;
   - owned state and persistence: in memory, files, Git, SQLite, browser storage;
   - main types and entry points;
   - concurrency: Tokio tasks, threads, workers, locks and lock order;
   - errors and recovery: what fails, what is retried, what is lost or kept;
   - configuration and environment inputs: settings keys, environment
     variables, CLI flags;
   - invariants the code relies on.
5. **Dynamic views cover failure paths**, not only the happy path. Each
   area's required dynamic views are listed in its ticket.
6. **Planned behavior stays out.** Describe what the audited revision does.
   Put designed-but-unbuilt behavior in requirements, or note it as absent.

## Evidence format

- **Cite each claim** as `path:line` or `path:start-end`, repository relative,
  in backticks, at the audited revision. The checker resolves these. Prefer
  the definition or the deciding branch over a file header.
- **Name the test** that demonstrates the behavior: `Test:` followed by a
  `path` (and test name when the file holds many), or `Test: untested` when
  no test exercises it. Never imply coverage that was not found.
- **Unverifiable claims are not asserted.** Put them in
  [unresolved.md](unresolved.md) with an id, the claim, why it could not be
  verified, and the follow-up ticket, if any.
- **Discrepancies** between code and requirements or contracts are listed in
  the chapter's "Discrepancies" section, each with a follow-up ticket slug.
- Model element `url` values use `${SOURCE}`, pinned in `workspace.dsl`.

## File layout and ownership

```text
docs/architecture/
  workspace.dsl                  includes, styles, SOURCE pin         shared
  areas.json                     area roots for the coverage checker  shared
  model/context.dsl              people and external systems          shared
  model/system.dsl               hs2 system, machineData, includes    shared
  model/overview.dsl             conceptual tour elements (custom)    shared
  model/<area>.dsl               the area's containers and components area
  model/<area>-relationships.dsl intra-area and area-to-external edges area
  model/relationships-cross.dsl  edges between two areas              append-only
  model/deployment.dsl           deployment environments              shared
  views/tour.dsl                 HS2-00-Overview custom view          shared
  views/shared.dsl               context, container, deployment views shared
  views/<area>.dsl               the area's component and dynamic views area
  views/flows.dsl                cross-layer dynamic views            HS2-M81CCP
  views/screenshots.dsl          image views of real client screens   shared
  narrative/00-start-here.md     guided tour chapter                  shared
  narrative/NN-<area>.md         area chapter (Structurizr !docs)     area
  narrative/screenshots/         real-client captures and manifest    shared
  coverage/<area>.md             per-module coverage rows             area
  unresolved.md                  unresolved-claims register           append-only
  decisions/                     architecture decision records        shared
```

Areas are `core`, `orchestration`, `cli`, `server`, `web-host` and
`web-client`. [areas.json](areas.json) holds the source roots;
`clients/web/src` is shared by the two web areas until HS2-T6SD67 publishes
the Node/browser module classification. Rules:

- An area ticket edits only its own files. It may append to
  `relationships-cross.dsl` and `unresolved.md` under a comment or heading
  naming its ticket; it never reorders, rewrites or deletes another
  ticket's entries. Shared files change only in HS2-M81CCP, or with a
  note on the owning ticket.
- A new external system goes at the end of `model/context.dsl` under a
  comment naming the ticket.
- New view keys use the area prefix: `HS2-Core-`, `HS2-Orch-`, `HS2-Cli-`,
  `HS2-Server-`, `HS2-WebHost-`, `HS2-WebClient-`, `HS2-Flow-`.
- Identifiers are global in the workspace. Prefix new area identifiers with
  the area (`core`, `orch`, `cli`, `mcp`, `server`, `host`, `web`) to avoid
  collisions between parallel tickets.

## Model conventions

- Treat element identifiers, relationship identifiers, and view keys as stable.
  Keep them through renames and description edits. Use deliberate new
  identifiers when the architectural entity or relationship changes identity.
  The 20 baseline view keys are unchanged by the area split.
- Document platform-specific or optional behavior in its view or coverage row.
- Treat a change to ownership, persistence, deployment, trust, communication,
  capability, or an important sequence as a possible architecture change.
  Review the affected model slice. Ordinary code rearrangement, wording, and
  layout edits do not create architecture events.
- Split a view that is too dense to read in the viewer rather than shrinking
  its font.
- Give every view a `title` of the form `<layer> <Area> · <name>` (for
  example `6 Ticketing core · Flow: index rebuild`). The viewer lists views
  in definition order (`structurizr.sort created`), so `workspace.dsl`
  includes view files in tour order.
- Keep the `HS2-00-Overview` custom elements true of the detailed model and
  point each one's `url` at a `{workspace}/diagrams#<key>` view.
- Put every colour, shape and line style inside the `light` or `dark` styles
  block. The viewer merges same-tag styles in place, so a tag styled both
  inside and outside a colour-scheme block shows dark colours in light mode.
- Start narrative chapters at `##`: the documentation viewer drops `#`
  headings, so `##` is the chapter and `###` its sections.
- `scripts/architecture-workspace-policy.test.mjs` pins these rules
  (HS2-DHCY18).

The source-controlled workspace replaces the old overview diagram as the
current map. The existing diagram and its HTML source remain available as an
earlier illustration. The prototype, visual-history comparison, and incremental
refresh have their own scope in HS2-MAZXCF, HS2-7WQK26, and HS2-S0QE57.

## No-surprise checklist

An area ticket completes only when every item holds for its area. The
independent review (HS2-38CA53) applies the same list.

- [ ] `node scripts/architecture-coverage.mjs check --area <key>` reports
      every required entry mapped and no bad citation.
- [ ] Every container in the area has components; every component records
      the seven fields of the depth standard or says "none" for a field.
- [ ] Every route, command family, MCP tool, background task, worker and
      durable store in the area is named (as applicable).
- [ ] Every environment variable and settings key the area reads is listed,
      with its default.
- [ ] Every lock, task, thread or worker is listed with its owner and
      shutdown behavior.
- [ ] Every durable record or file format the area writes is listed with its
      location, writer, and crash behavior.
- [ ] Each required dynamic view exists, including its failure path, and is
      narrated hop by hop with citations.
- [ ] Each claim has a citation and a test or `untested`; unverifiable claims
      are in `unresolved.md`.
- [ ] Discrepancies with requirements and contracts have follow-up tickets.
- [ ] A cold read of five sampled modules in the area produced no surprise
      the docs do not explain.
- [ ] The workspace validates with the pinned Structurizr release and every
      area view renders without layout errors.

## Branch etiquette

- Work on `docs/hs2-3fa79j-structurizr-baseline`; do not merge it to main
  until HS2-38CA53 and the maintainer sign off.
- Make at least one commit per ticket, with the slug first in the subject.
- Rebase only your own unpushed commits onto the branch tip before pushing.
  Never force-push the branch and never rewrite pushed commits.
- Do not merge newer `main` into the branch during the area work; the audit
  pin must stay the merged revision. A re-pin is its own ticket.
