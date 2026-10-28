# Integrator throughput baseline (2026-10-27)

HS2-34PWSK measures existing workflow evidence. It does not establish an advantage over one
terminal with background agents. That comparison belongs to HS2-K1QNGC. The original report's
1.34 tickets/hour and bottleneck rankings were invalidated by Review return
01M5YA13B4BVS4F62CTH7Z182Q: same-slug commits and later correction claims were joined to the
wrong operations, and integrated commits were counted without completion and covering green CI.
Those figures must not drive gate or architecture decisions.

## Reproduce a fixed observation

Use the integrator's verified compatible CLI; opening an old pool with a newer candidate can
migrate it. Collection is one bounded read, with no host launch, credentials or polling:

```sh
zsh -ic 'node scripts/throughput-baseline.mjs -C <shared-store> --project <shipping-checkout> \
  --cli <verified-cli> --since 2026-10-26T11:22:00Z --until 2026-10-27T09:20:00Z \
  --snapshot-out <evidence.json> --json' > result.json
zsh -ic 'node scripts/throughput-baseline.mjs --input <evidence.json> \
  --since 2026-10-26T11:22:00Z --until 2026-10-27T09:20:00Z --json' > replay.json
```

Offline replay uses only the archived input: no CLI, Git or network. Archive its checksum and
the measuring script commit with the output. Later live reads are new observations, not a replay
of an earlier pool. Snapshot files contain identifiers, counts, hashes, times, typed handoffs and
Git ancestry booleans; ticket titles, note prose, paths, credentials and customer content are omitted.

## Definitions and limits

- **Cohort:** operations with one exact addressed assignment note in the inclusive observation
  window. Unknown-start and outside-window operations are excluded and counted separately.
  Claims, handoffs, returns, release, CI and completion must also fall inside that window and
  before the next assignment for the ticket. This is an assigned-in-window cohort, not every
  completion produced by earlier work during the window.
- **Assignment and handoff times are proxies:** the assignment note's creation and the unique
  code handoff naming the structured source commit, or the exact typed findings note id.
  Claim-to-handoff is elapsed wall time, not active labor. Legacy assignments without typed
  evidence remain unknown; the script does not guess from a nearby heading.
- **Source integration:** exact source SHA in main history, or a unique independently computed
  stable Git patch-id match. Subjects are never integration evidence. Ambiguous mappings,
  missing local source objects or history outside the 2,000-commit bound remain unknown.
  Patch equivalence is Git's whitespace-insensitive patch identity, not proof of review intent.
- **First observed push:** creation of the first recorded required main/push CI run containing
  that exact main commit. This is a proxy for remote observation, not the local integration
  instant or integrator mutation time. The query is bounded to the last 200 main runs;
  absent metadata is unknown rather than evidence of absence.
- **Covering green:** workflow CI, branch main, event push, valid run id/head SHA, completed and
  successful; actual local ancestry must contain the integrated source mapping. Select the
  earliest observed successful outcome by `updatedAt`, not earliest creation. `updatedAt` is a
  completion proxy (later metadata edits can shift it), not a precise job-finish timestamp.
- **Verified shipped/completed:** code integration and first observed push, covering green,
  structured Done, current ticket Completed, and a bounded completion timestamp at or after
  green. Deduplicate by qualified ticket. Findings completions are separate; returned originals
  cannot inherit a corrected operation's completion. Pending code is separate from throughput.
- **Roles:** a different worker's claim does not establish integrator activity. Historical role/
  generation-bound integration and gate transitions are not available from these contracts, so
  those spans remain unknown. Current availability is not historical role authority.
- **Release gaps:** release to the next own claim within the same worker generation is an
  observed gap, not idle capacity or proof that work was available. Missing releases are unknown.
- **Cohorts:** coordination/product, risk, scope, overlap, platform and provider classifications
  remain unknown without recorded cohort evidence. A title heuristic is not a measured cohort.
  Human interventions, active review time and cost are unavailable. No provider effect is claimed.

HS2-4273QB adds actual coordination transition timestamps. They do not substitute for exact
joins here. Phase-report's current completion timestamp and current assignment/ticket states
cannot reconstruct every historical reopen or state transition; this conservative metric is a
lower bound on matched evidence, not a claim that unmatched work never shipped.

## Transition evidence for new assignments (HS2-4273QB)

`coordination status` now retains an ordered per-assignment transition history with RFC 3339
instants and the known role/id. The collector carries this identifier-only history into its
frozen snapshot. New observations prefer Offered and Handoff events over note-time proxies,
report Accepted separately from the ticket's actual claim history, and use the Review return
event when present. They also report Verifying, Integrating, CI, Done and Blocked times
when present. Legacy assignments without history still use the bounded proxies above. A CI
transition records entry to that phase; it is not proof of a remote push or a green run. The
2026-10-27 observation below remains frozen and is not recalculated from later state.

## Recomputed observation

Window: **2026-10-26T11:22Z to 2026-10-27T09:20Z (21.9667 hours)**.
The one-shot collection finished at 2026-10-27T09:32:13.064Z. Its current state is
evidence collected then, not a reconstruction of every state transition at the window end.

| Count                                                      |       Value |
| ---------------------------------------------------------- | ----------: |
| Observed operations / included addressed cohort / excluded | 74 / 70 / 4 |
| Exact matched handoffs / typed findings                    |      55 / 6 |
| Matched review returns                                     |          11 |
| Source-mapped code with observed push                      |          19 |
| Verified shipped/completed distinct code tickets           |          19 |
| Integrated pending verification                            |           0 |
| Completed typed findings (separate)                        |           5 |
| **Matched verified tickets/hour**                          |   **0.865** |

This is a conservative evidence-matched cohort rate, not total organizational throughput.
Legacy untyped findings, missing assignment notes, ambiguous source mapping and unmatched
completion remain unknown. Matched returns / matched handoffs are 11/55 (20%); this is not
a complete return rate for all operations. All cohort dimensions remain unknown.

| Elapsed proxy span (minutes) |   n | unknown |  median |     p90 |
| ---------------------------- | --: | ------: | ------: | ------: |
| assignedToClaim              |  67 |       3 |     0.6 |     1.4 |
| claimToHandoff               |  55 |      15 |    18.1 |    58.1 |
| handoffToIntegratorClaim     |   0 |      55 | unknown | unknown |
| integratingToMainGate        |   0 |      55 | unknown | unknown |
| mainGate                     |   0 |      55 | unknown | unknown |
| handoffToFirstObservedPush   |  19 |      36 |    34.8 |   109.9 |
| firstObservedPushToCiGreen   |  19 |       0 |    63.7 |   225.9 |
| ciGreenToCompleted           |  19 |       0 |    18.6 |   127.9 |
| assignedToVerifiedCompletion |  19 |       0 |   228.6 |   362.4 |
| observedReleaseToNextClaim   |  46 |       9 |    10.8 |    88.0 |

Unknown counts use the eligible denominator for each row, not all 74 observed operations.
There are 38 recorded CI/main/push runs created in the window: 17 failed and 3 cancelled.
The 17 successful completed runs whose outcome was observed by the end have a median
creation-to-update duration of 21.8 minutes. These independent run counts do not establish
ticket causality. The 63.7-minute median first-observed-push-to-green span is larger than
a green run duration, but these partial proxies do not isolate why or prove the largest
end-to-end bottleneck; review/main-gate costs are unobserved.

The sanitized frozen input is attached to HS2-34PWSK as `34pwsk-evidence.json` (761,169 bytes),
SHA256 `3d9b68277b527023b0780d106fe4a88898fbc50f251d84802bf705b229df6493`.
Offline replay produced the same summary/timelines as the collection. The ticket handoff
records the measuring source commit and attachment id; download that input to reproduce it.

## Representative manually verified workload cohorts

This four-operation sample classifies available evidence; it is **not** a whole-window
classification or a throughput comparison. Automated timeline cohort fields remain unknown.
Exact operation/source provenance below does not upgrade unmatched causal timeline evidence.
Provider mix remains unknown: names of workers do not establish model/provider execution.

| Exact operation                | Source SHA                               | Risk and workload                      | Platform evidence                                                | Files / added / removed |
| ------------------------------ | ---------------------------------------- | -------------------------------------- | ---------------------------------------------------------------- | ----------------------: |
| hs2-mfyyj9-codex-20261027-3    | bd3d5981826f2a72393394cb15c7c17473642eee | T2 claim authority / coordination      | Unix process groups and native Windows Job Object/event lifetime |          16 / 1208 / 26 |
| hs2-twzw7x-claude-20261027-3   | f70e6dc945a4115d7bf2dd5a307fe68a0ed1c650 | T2 assignment authority / coordination | Real-process CLI; native Windows dispatcher suite required       |         17 / 2780 / 123 |
| hs2-4zfb8k-claude-b-20261027-1 | 8463ef87eda136b788069c7465e97db3bf9c5f45 | T2 derived index / product             | Real-server browser and responsive QA                            |             5 / 238 / 3 |
| hs2-7kxgmc-codex-20261027-1    | 0df1f18aa9a7008ceb3e78f7f001fb941b8164a5 | T1 test/tooling configuration          | Ubuntu CI failure; local full-suite reproduction                 |             6 / 41 / 10 |

Supporting durable note IDs (reviewed manually, no generic prose inference):

- MF: third-operation assignment `01M5XYS61284XJA7CGJE3ZE4C9`, T2 preliminary design
  `01M5XYTVARX4CHZ8MFJV03REZ4`, source handoff `01M5XZGJVQCC92M97HMY8DYCFF`;
  actual native Windows result `01M5Y3YHTEDRNHSYPJNTFQ2566`. This operation is in the 19 matched completions.
- TW: third-operation assignment `01M5Y1MSBXNK3ZSPFFH0874TKQ`, source handoff
  `01M5Y34WEBYQPWW4XQH3G9YBYQ`, Windows suite wiring `01M5Y4EN3KD3DN7TMQBQJV9SK5`
  and actual Windows/pass-but-overall-red result `01M5Y7V0MTVG10DR5DWYRSACTR`.
  Its conservative automated handoff join remains unknown; root added an integration correction,
  so source patch identity does not establish the final main commit mapping.
- 4Z: source/T2/index/browser handoff `01M5XWMXN3M1024PFA5TA2A0DT` and completed review
  `01M5Y2ZFYW42MV0B1NY2KWYJY6`. Its missing exact addressed assignment-note match excludes
  it from the automated observation cohort; this table does not silently credit it.
- 7K: T1 assignment `01M5Y8KHFPP681571J4XHSEXS5`, preliminary measurement
  `01M5Y8PHX9ZX17XY5VQSPHSQ06`, source handoff `01M5Y9PNB9MKAT6EGD8ZWHDTYR`.
  It is a matched handoff pending integration evidence in the frozen observation, not verified throughput.

Reproduce count-only scope with `git show --format= --numstat <source-sha>`: count rows and
sum numeric additions/deletions. Compare path sets to measure static source overlap. MF/TW
share 3 changed paths; every other pair in this sample shares 2. Those are exact path intersections
including common documentation/workflow paths, **not** proof of concurrent contention or review
cost. Unclassified operations, actual in-flight overlap and per-provider effects remain unknown.
The note IDs and source SHAs make this manual annotation reproducible without copying prose,
customer content or local paths into the report.

## Existing gates and optimization decisions

`scripts/check.sh` caches a successful clean exact `HEAD^{tree}` under the Git-local root-gate
cache; dirty or different trees cannot reuse it. `scripts/push.sh` runs that gate before opening
the remote, and the pre-push hook recognizes the passed tree (HS2-CD3D2T). This correction does
not change that policy or add another cache.

Historical main-mutation, review and local gate stage durations are not established by the
available role-bounded evidence. The earlier 8.3-minute gate estimate and “less than one fifth”
cache conclusion are withdrawn. There is no measured basis here for changing cache or gate
policy. Obtain role-bound timings through HS2-4273QB before ranking local gate stages.

Candidate success criteria for the prospective experiment, not conclusions from this window:
reduce matched handoff-to-first-observed-push and push-to-covering-green medians by at least
25%, without reducing verified tickets/hour or increasing returns/reverts. HS2-NNSWDS,
HS2-E20S6K, HS2-FCCRCK and HS2-1C1565 consume measured evidence; none is justified by the
withdrawn baseline alone. Do not optimize an unmeasured “idle” gap.

## Prospective comparison protocol (HS2-K1QNGC)

1. **Arms.**
   - Arm A: the current integrator with two or more independent workers.
   - Arm B: one terminal using background agents on the same `main` with the same gates.
   - Do not repeat completed production work to manufacture a baseline.
2. **Allocation.** Alternate the arms in fixed blocks of at least four hours, or eight
   integrated tickets, over the same Up Next queue. Assign each block's tickets before it starts,
   matched on tier (T0, T1, T2), approximate scope (files and lines in the final commit), overlap
   with in-flight work, and platform requirements.
3. **Primary outcome.** Verified integrated tickets per hour: commits reached `main` with a
   covering green CI run and the ticket completed.
4. **Secondary outcomes.** End-to-end cycle time, handoff → integrated, correction rate, and the
   reverts or re-opens a ticket caused within 48 h.
5. **Exclusions.** Exclude blocks with a `main` CI outage longer than 30 min that is independent of
   both arms. Count any human intervention per arm from durable notes.
6. **Measurement.** Use the full collection command above at each block end with explicit start/end. Archive the sanitized snapshot, its checksum, output, script commit and cohort ticket list. Replay through `--input`. Arm B needs equivalent exact source/handoff/completion evidence; missing evidence stays unknown. Record matched risk, scope, overlap, platform and provider metadata separately before allocation, rather than deriving cohorts from ticket titles.
7. **Decision rule.** Prefer the arm whose primary outcome is at least 25% higher over at least
   three blocks each, with no worse correction or revert rate. Otherwise keep the simpler arm.
