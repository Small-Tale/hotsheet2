# Hot Sheet 2 architecture baseline

**Audited source** [`52010ebaf629b1ed95850cf4bb64b1385a03e6e4` on origin/main](https://github.com/Small-Tale/hotsheet2/commit/52010ebaf629b1ed95850cf4bb64b1385a03e6e4), 2026-11-25. This documentation records what is present at that revision. Keep the source revision tied to the evidence when refreshing the model.

## Where to start

| Question                                                                  | Structurizr view                                                                                                                                      |
| ------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| Who interacts with Hot Sheet, and which systems does it depend on?        | `HS2-Context`                                                                                                                                         |
| What runs on a machine, and which processes share a ticket store?         | `HS2-WorkspaceRuntime`, `HS2-DesktopDeployment`, `HS2-HeadlessDeployment`                                                                             |
| Which records are authoritative, and how does a write reach open clients? | `HS2-ProviderBoundaries`, `HS2-TicketComponents`, `HS2-WebGitTicketWrite`, `HS2-NativeIssueWrite`, `HS2-GitReconciliation`, `HS2-HeadlessTicketWrite` |
| Where do browser state, live updates, and terminal input live?            | `HS2-WebComponents`, `HS2-ExecutionComponents`, `HS2-UnixTerminal`                                                                                    |
| How are scheduled commands, job observations, and remote work owned?      | `HS2-ScheduledCommand`, `HS2-HostSupport`, `HS2-OrchestrationComponents`, `HS2-OrchestrationPlacement`, `HS2-RemoteDeployment`                        |
| How do legacy import and the optional macOS screen saver fit?             | `HS2-MigrationAndStandby`, `HS2-LegacyImport`                                                                                                         |

The [coverage matrix](coverage.md) records the source evidence, architectural
invariants, platform variations, and the current-state limits. The
[architecture overview](../01-architecture.md) is the prose entry point.
The [requirements index](../README.md) tracks design intent and delivery
status. The [codebase map](../CODEBASE-MAP.md) locates more granular modules.

## Open or validate the workspace

Use the consolidated [Structurizr application release v2026.10.26](https://github.com/structurizr/structurizr/releases/tag/v2026.10.26).
With its application archive and a compatible JDK, run from the repository root:

```sh
java -jar /path/to/structurizr.war validate -workspace docs/architecture/workspace.dsl
java -jar /path/to/structurizr.war inspect -workspace docs/architecture/workspace.dsl
```

For local viewing, copy `docs/architecture` to a scratch directory and pass the
copy to `java -jar /path/to/structurizr.war local /path/to/copy`. Open the
localhost URL printed by the command. Scratch copies keep viewer-generated
layout files out of the model history.
[Structurizr command documentation](https://docs.structurizr.com/commands)

## Model conventions

- Store the workspace in Git. `model.dsl` defines elements, `relationships.dsl`
  defines dependency and communication edges, `deployment.dsl` defines host
  instances, and `views.dsl` composes them with explicit view keys.
- Treat element identifiers, relationship identifiers, and view keys as stable.
  Keep them through renames and description edits. Use deliberate new identifiers
  when the architectural entity or relationship changes identity.
- Put responsibilities and boundaries in the model. Map linked libraries and
  shared modules to their runtime owners in the coverage matrix. Keep evidence
  links pinned to the audited source revision and use view keys when discussing
  changes. The source stamp, evidence URLs and exported viewer timestamps are
  provenance for a comparison.
- Document platform-specific or optional behavior in its view or in the coverage
  matrix. Put planned capabilities in requirements or a proposal. Use the
  audited source revision as the starting point for future visual history.
- Treat a change to ownership, persistence, deployment, trust, communication,
  capability, or an important sequence as a possible architecture change.
  Review the affected model slice. Ordinary code rearrangement, wording, and
  layout edits do not create architecture events.

The source-controlled workspace replaces the old overview diagram as the
current map. The existing diagram and its HTML source remain available as an
earlier illustration. The prototype, visual-history comparison, and incremental
refresh have their own scope in HS2-MAZXCF, HS2-7WQK26, and HS2-S0QE57.
