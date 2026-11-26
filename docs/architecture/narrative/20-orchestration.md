## Orchestration, coordination, resource claims and jobs

![](embed:HS2-Orch-Placement)

This is the starting diagram for this area. The **Start here** chapter lists the others.

Status: written by HS2-RJB111. Audited source
`8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774`. Follows the depth standard and
evidence format in the architecture README. Per-module rows, with all seven
depth fields, are in [coverage/orchestration.md](../coverage/orchestration.md).

### Overview

This area is library code in the `orchLib` container. It is linked into the
server and the CLI and runs no process of its own. It covers four domains:

1. **Orchestration** (`crates/hotsheet-ticketing/src/orchestration`): an
   owner machine declares targets, plans steps on target tickets, and places
   AI sessions on its own or a paired machine. Each step is decided by pure
   functions over files the owner keeps in its machine home.
2. **Coordination**: ticket claims and leases, Git-ref claim markers,
   provider advisory claims, speculation on unconfirmed prerequisites,
   worker plans, and development strategies.
3. **Resource claims** (`crates/hotsheet-resource-claims`): renewable,
   fenced leases on non-ticket resources over four backends.
4. **Jobs** (`crates/hotsheet-jobs`): durable Job records and a watch
   monitor over seven CI providers. Orchestration gates read from them.

The orchestration core is written as pure decisions plus small, bounded,
atomically replaced JSON stores. The modules hold no in-memory state across
calls, and they start no threads or tasks. The exceptions are the Jobs
monitor and watchers and the resource-claim waiters.

The runtime that sequences these decisions is not in `orchLib`. It lives in
the hotsheet-cli crate, which the server and MCP also link as a library:

- `OrchestrationRuntime` (`crates/hotsheet-cli/src/orchestration.rs:186`) is
  the CLI chapter's component `cliOrchestration`.
- The coordination pool, worker plans, the speculation ledger and the
  development-strategy resolver (`crates/hotsheet-cli/src/coordination/contract.rs:10`)
  are the component `cliCoordination`.

The server runs `cliOrchestration` in Tokio loops
(`crates/hotsheet-server/src/routes/orchestration.rs:353-560`). The CLI and
server chapters describe those hosts; this chapter cites them where a domain
rule depends on them. Dynamic views here are scoped to `orchLib`, so they
show the hosting `server` or `cli` container rather than those components.

Views:

| View                          | What it shows                                                                   |
| ----------------------------- | ------------------------------------------------------------------------------- |
| `HS2-Orch-Placement`          | Declarations, plans, policy, permissions, admission, sessions and the scheduler |
| `HS2-Orch-RunsAndGates`       | Runs, gates, reconciliation, speculation and progress                           |
| `HS2-Orch-Claims`             | Ticket, Git-ref, advisory and resource claims                                   |
| `HS2-Orch-Jobs`               | The Job store, monitor and CI adapters                                          |
| `HS2-Orch-LeaseExpiry`        | Lease expiry and recovery (dynamic)                                             |
| `HS2-Orch-RefusedPlacement`   | Remote placement that the target refuses (dynamic)                              |
| `HS2-Orch-FailedCiGate`       | A failed CI observation driving a gate (dynamic)                                |
| `HS2-Orch-WorkerPlanDispatch` | A worker plan dispatch (dynamic)                                                |
| `HS2-Orch-SpeculationRed`     | Speculation invalidated by a red base gate (dynamic)                            |

### Components

The model has 19 components. Their responsibilities are in the model
descriptions and the coverage rows.

| Group                     | Components                                                                  |
| ------------------------- | --------------------------------------------------------------------------- |
| Declarations and planning | `orchDeclarations`, `orchPlanning`, `orchPolicy`, `orchPermissions`         |
| Placement                 | `orchAdmission`, `orchSessions`, `orchScheduler`                            |
| Runs and gates            | `orchRuns`, `orchGates`, `orchReconcile`, `orchProgress`, `orchSpeculation` |
| Claims                    | `orchTicketClaims`, `orchGitRefClaims`, `orchAdvisoryClaims`                |
| Resource claims           | `orchResourceClaims`, `orchResourceBackends`                                |
| Jobs                      | `orchJobStore`, `orchCiAdapters`                                            |

Three things are deliberately outside these components:

- **Ticket lease rules.** These live in `crates/hotsheet-ticketing/src/ops.rs`, which
  belongs to the core area. They are summarized under
  [Lease semantics](#lease-semantics) because the claim gate depends on them.
- **The coordination pool.** Worker plans, assignments, the speculation
  ledger and the development-strategy resolver live in
  `crates/hotsheet-cli/src/coordination` (CLI area, component
  `cliCoordination`). The domain is described
  [below](#coordination-pool-worker-plans-and-development-strategies).
- **The process journal.** It is in `crates/hotsheet-process-journal/src/lib.rs` (core area).
  Jobs use it only through `JournalProbe` (`crates/hotsheet-jobs/src/probe.rs:37-66`). No
  production `Acquire` sets a `launch_id`
  (`crates/hotsheet-cli/src/orchestration/validation_runner.rs:113-116`), so outside tests the
  probe never reports a Job's owner gone. Test:
  `crates/hotsheet-jobs/tests/adversarial.rs` (`owned_liveness_follows_the_process_journal_launch_id`)
  covers the probe only.

#### Lock order

Lock order inside a binding or manifest write:

1. Source guard.
2. Manifest lock.
3. Binding lock.
4. Remote guard.

Sources: `crates/hotsheet-ticketing/src/orchestration/workflow.rs:56-60`,
`crates/hotsheet-ticketing/src/orchestration/workflow.rs:629-650`.

Plan definitions take the shared-plan lock and then the machine plan lock.
They write the machine reads before the shared plan, so lock-free readers
never see a step without its reads
(`crates/hotsheet-ticketing/src/orchestration/readiness.rs:1316-1317`, `crates/hotsheet-ticketing/src/orchestration/readiness.rs:1396-1412`).
Test: `crates/hotsheet-ticketing/src/orchestration/readiness_tests.rs`
(`hs2_g052rv_a_concurrent_reader_never_sees_a_step_before_its_reads`).

### Durable state and file formats

**Common write path.** Every orchestration file uses one storage helper
(`crates/hotsheet-ticketing/src/orchestration/storage.rs:146-184`):

- Write a same-directory `.orchestration-*.tmp` file.
- `fsync` it, then atomically replace the target.
- On Unix, `fsync` the parent directory.

A crash leaves the complete old file or the complete new file. It can also
leave a stray temp file, which nothing sweeps.

**Common read rules.**

- A record with a `schema_version` other than 1 is refused, never rewritten
  (`crates/hotsheet-ticketing/src/orchestration/storage.rs:198-211`).
- Machine files are mode 0600.
- Each store has a size cap.
- Checked-in files live under `.hotsheet2/` in the checkout.
- `HOME` below is `HOTSHEET_HOME`.

Test: `crates/hotsheet-ticketing/src/orchestration/tests.rs`
(`hs2_4j6v46_newer_corrupt_and_untrusted_shared_data_are_not_overwritten`).

#### Orchestration records

Every record here is written by the orchestration runtime. Locations are in
the coverage rows.

| Record                                                                                                                                               | Scope          | Lifecycle                                                                   | Crash behavior                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------------------- | -------------- | --------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `.hotsheet2/orchestration.json` (targets)                                                                                                            | checked in     | Compare-and-swap on `revision`; removed targets stay as tombstones          | Atomic                                                                                                      |
| `.hotsheet2/orchestration-plan.json`, `orchestration-policy.json`                                                                                    | checked in     | Compare-and-swap; identical content is a no-op                              | Atomic. A merge-marker file is `Corrupt`                                                                    |
| `HOME/orchestration-bindings.json`                                                                                                                   | machine        | Keys never deleted; 32-entry audit window per binding                       | A bind reservation survives as a fence                                                                      |
| `HOME/orchestration-plan.json`, `orchestration-plan-updates/`, `orchestration-replan/`                                                               | machine        | Reads are pruned with their steps. Drafts are discarded when an update ends | Drafts left by a crash are discarded at the next seed                                                       |
| `HOME/orchestration-permissions.json`                                                                                                                | machine        | Lapses are computed on read. Only used or lapsed requests are pruned        | Atomic                                                                                                      |
| `HOME/orchestration-runs.json`                                                                                                                       | machine        | Up to 1024 runs; only terminal runs are pruned                              | Atomic                                                                                                      |
| `HOME/orchestration-gates.json`                                                                                                                      | machine        | Superseded, then settled gates are pruned; open gates never                 | Atomic                                                                                                      |
| `HOME/orchestration-validation.json`, `orchestration-hosted-validations.json`                                                                        | machine        | Results immutable; 16 per step. Host keeps 256 executions                   | A host execution still running at the crash ends as `execution_lost`                                        |
| `HOME/orchestration-sessions.json`, `orchestration-hosted-sessions.json`, `orchestration-withdrawn-sessions.json`, `orchestration-session-idle.json` | machine        | Owner prunes finished sessions; host prunes ended ones; fences last 15 min  | Withdraw writes the fence before removing the reservation                                                   |
| `HOME/orchestration-admission.json`, `orchestration-tool-capacity.json`                                                                              | machine        | Absent means policy defaults                                                | Damaged means refuse                                                                                        |
| `HOME/orchestration-scheduler.json`, `orchestration-scheduler-decisions/`, `orchestration-scheduler-wake/token`                                      | machine        | Placement memory per owner; decision journal                                | The wake token is a plain, best-effort write (`crates/hotsheet-cli/src/orchestration/scheduler.rs:354-359`) |
| `HOME/orchestration-provisional.json`                                                                                                                | machine        | Created only by a change; settled entries retire                            | Atomic                                                                                                      |
| `HOME/orchestration-line-observations/<checkout>.json`                                                                                               | machine (host) | Entries live 30 s to 15 min                                                 | Unreadable reads as empty                                                                                   |
| Journals: `orchestration-audit/`, `orchestration-run-events/`, `hosted-session-audit/` (`*.jsonl`)                                                   | machine        | 1 MiB segment plus one predecessor; readers are told of the `gap`           | `fsync` per append; a torn last line is skipped                                                             |

#### Coordination and claim records

| Record                                                                                                                                  | Writer                                 | Lifecycle and crash behavior                                                                                                                                                                                 |
| --------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Ticket frontmatter `claimed_by`, `claim_lease_expires_at`, `claim_eta_at`, `worker_label`, `claim_host`, `claim_count`, `claim_history` | `crates/hotsheet-ticketing/src/ops.rs` | Written inside the ticket transaction lock (`crates/hotsheet-ticketing/src/store.rs:570-580`). History is append-only and merged by `merge_claim_history` (`crates/hotsheet-ticketing/src/merge.rs:162-167`) |
| `refs/hotsheet/claims/<ULID>`, `refs/hotsheet/resource-claims/<ns>/<res>` on the remote                                                 | `GitRefCas`, `distclaim`               | Orphan commit holding a JSON marker. Deleted on release; otherwise it lapses at its lease                                                                                                                    |
| `<git-dir>/hotsheet-provisional-claims.json`, `<git-dir>/hotsheet-claim-conflicts.json`                                                 | claim gate                             | Cleared on confirm or loss. Conflicts are acknowledged after a delivered push (`crates/hotsheet-server/src/claim_conflict_push.rs:43-60`)                                                                    |
| `HOME/resource-claims/configurations.json`, `.hotsheet2/resource-claims.json`                                                           | backend registry                       | Atomic; written only on change                                                                                                                                                                               |
| `HOME/host-id`                                                                                                                          | `claim_host`                           | Created once with `create_new`                                                                                                                                                                               |
| `HOME/advisory_claims/provisional.sqlite`                                                                                               | server and CLI                         | Survives restart and is replayed. Test: `crates/hotsheet-server/tests/jira_advisory_claim_http.rs` (`hs2_bpkm76_provisional_claim_replays_after_restart_and_reconciles`)                                     |
| `HOME/resource-claims/namespaces/<ns>/state.json`, `waiters/<ULID>`, `key`                                                              | local resource backend                 | A released resource keeps its fencing counter. Crashed waiter files are removed 60 s after their deadline                                                                                                    |
| `HOME/claim-backends/approvals.json`, `<id>/manifest.toml`                                                                              | external plugin approval               | Temp file and persist, with no lock (`crates/hotsheet-resource-claims/src/external/approval.rs:77-90`)                                                                                                       |
| Upstash keys `hs2rc:v1:*`                                                                                                               | Upstash Lua scripts                    | Claim hash expires one hour after lease end. The fencing counter never expires                                                                                                                               |
| `HOME/jobs/<checkout>/<ULID>/job.json`, `feed.jsonl`, `history.json`, `revision`                                                        | Job store                              | Retention is 15 min or 24 h, and at most 64 Jobs. Crash debris is swept by `prune_locked` (`crates/hotsheet-jobs/src/store.rs:905-932`)                                                                      |
| `<git-common-dir>/hotsheet-coordination/state.json`, `speculation.json`, `batches.json`                                                 | coordination pool (CLI area)           | Temp file, `fsync`, persist (`crates/hotsheet-cli/src/coordination/mod.rs:166-232`)                                                                                                                          |

#### State machines

**Dispatch run.** Source: `crates/hotsheet-ticketing/src/orchestration/runs.rs:20-82`. Test:
`crates/hotsheet-ticketing/src/orchestration/run_tests.rs`
(`hs2_ebhdsb_every_state_edge_is_allowed_or_refused_exactly`).

| From                         | To                                                   | Trigger                                                                                                                                                                    |
| ---------------------------- | ---------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| new                          | requested                                            | `begin` (`crates/hotsheet-ticketing/src/orchestration/runs.rs:496`)                                                                                                        |
| requested                    | queued, dispatched, completed, failed, cancelled     | Dispatch, or `align` with the target ticket (`crates/hotsheet-ticketing/src/orchestration/reconcile.rs:214-247`)                                                           |
| queued                       | dispatched, completed, failed, cancelled, intervened | A live claim means `assigned_by_dispatcher`. A claim that moved with no holder means `claim_released` (`crates/hotsheet-ticketing/src/orchestration/reconcile.rs:234-237`) |
| dispatched                   | completed, failed, cancelled, intervened             | The claim was taken or released by someone else (`crates/hotsheet-ticketing/src/orchestration/reconcile.rs:252-259`)                                                       |
| intervened                   | dispatched, completed, failed, cancelled             | The run's own worker holds the claim again (`crates/hotsheet-ticketing/src/orchestration/reconcile.rs:244-247`)                                                            |
| completed, failed, cancelled | none                                                 | Terminal. `update` answers `NotActive` (`crates/hotsheet-ticketing/src/orchestration/runs.rs:597-599`)                                                                     |

Two edges sit outside the table:

- A `Correct` control can move any non-terminal run to any state
  (`crates/hotsheet-ticketing/src/orchestration/runs.rs:76-81`).
- Pause, resume and handoff are history entries, not states.

**CI, deployment and review gate.** Source: `crates/hotsheet-ticketing/src/orchestration/gates.rs:59-120`. Test:
`crates/hotsheet-ticketing/src/orchestration/gate_tests.rs` (`hs2_5jbypv_every_gate_edge_is_allowed_or_refused_exactly`).

| From              | To                                                                                  | Trigger                                                                                                                                                                            |
| ----------------- | ----------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| new               | unknown (CI); unknown or awaiting_approval (deployment); awaiting_approval (review) | `link` (`crates/hotsheet-ticketing/src/orchestration/gates.rs:715-726`)                                                                                                            |
| unknown           | pending, passed, failed, superseded                                                 | Provider reading, or a newer link                                                                                                                                                  |
| awaiting_approval | pending, unknown, failed, superseded, passed (review approved)                      | A person's answer or a review mark (`crates/hotsheet-ticketing/src/orchestration/gates.rs:901-966`). A provider never enters or leaves this state                                  |
| pending           | passed, failed, unknown, superseded                                                 | Provider reading; `newer_gate_linked`                                                                                                                                              |
| failed            | pending, passed, superseded                                                         | Re-watch by `gate link` or `gate sync`. The follower reads only pending gates, or unknown gates with `watch_lost` (`crates/hotsheet-ticketing/src/orchestration/gates.rs:300-303`) |
| passed            | superseded                                                                          | A newer commit linked, or the branch moved                                                                                                                                         |
| superseded        | none                                                                                | Every reading is ignored (`crates/hotsheet-ticketing/src/orchestration/gates.rs:344-346`)                                                                                          |

**Owner session.** Source: `crates/hotsheet-ticketing/src/orchestration/sessions.rs:131-137`.

| From      | To            | Trigger                                                                                                                                                                      |
| --------- | ------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| none      | requested     | `begin`, before the host is asked (`crates/hotsheet-cli/src/orchestration/sessions.rs:2517-2518`)                                                                            |
| requested | running       | The host answers started or existing (`crates/hotsheet-cli/src/orchestration/sessions.rs:2609-2626`)                                                                         |
| requested | failed        | The host refused, or a permanent transport refusal (`crates/hotsheet-cli/src/orchestration/sessions.rs:2573-2599`). Any other error leaves the session requested for a retry |
| requested | stopped       | Stop or withdraw                                                                                                                                                             |
| running   | stopped, lost | Wind-down or stop, or the host no longer lists it                                                                                                                            |
| any       | owner changes | Takeover by a person sets `paused`. `hand_back` is allowed only when the session is idle (`crates/hotsheet-ticketing/src/orchestration/sessions.rs:397-407`)                 |

Test: `crates/hotsheet-ticketing/src/orchestration/session_tests.rs`
(`hs2_gdjxgz_every_state_edge_is_allowed_or_refused_exactly`).

**Assignment.** Orchestration and the coordination pool use different
assignment models.

- **Orchestration dispatcher assignment** (`decide_assignment`, pure,
  `crates/hotsheet-ticketing/src/orchestration/assignment.rs:68-94`):
  - A live dispatcher or integrator in the pool gives `awaiting_dispatcher`.
  - An unknown pool gives `pool_unknown`.
  - Otherwise the state is `no_live_dispatcher`. The owner claims the work
    itself once `max(queued + 2 min, backoff_until)` has passed.
  - Test: `crates/hotsheet-ticketing/src/orchestration/assignment_tests.rs`
    (`hs2_t16sk0_every_pool_reading_decides_exactly`).
- **Coordination-pool assignment phases**
  (`crates/hotsheet-cli/src/coordination/contract.rs:106-116`):

  | Actor      | Allowed transitions                                                                                              | Source                                                  |
  | ---------- | ---------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- |
  | Worker     | offered → accepted; accepted → handoff; offered or accepted → blocked                                            | `crates/hotsheet-cli/src/coordination/mod.rs:925-933`   |
  | Integrator | handoff → verifying or integrating; verifying → done; integrating → ci; ci → done or integrating; blocked → done | `crates/hotsheet-cli/src/coordination/mod.rs:1024-1036` |

  Integration can also be suspended (`suspended_integration`).

**Ticket claim.** The full transition table is under
[Lease semantics](#lease-semantics). Resource-claim and Job lease tables are
there too.

**Permission request.** Source: `crates/hotsheet-ticketing/src/orchestration/permissions.rs:127-138`.

- A new request is pending (filed by `ask`).
- Allow Once leads to allowed. When the action happens, allowed becomes used.
  An allowed request unused for 1 h lapses.
- Always Allow leads to allowed plus a rule. The rule answers future requests
  with the same key.
- Deny is final.
- A pending request unanswered for 24 h lapses.
- A different answer to a settled request is `Conflict`.

Test: `crates/hotsheet-ticketing/src/orchestration/permission_tests.rs`
(`hs2_xhq1eh_transition_matrix_from_every_state`).

### Lease semantics

#### Ticket claims (frontmatter, core `crates/hotsheet-ticketing/src/ops.rs`)

**How expiry works.**

- Expiry is lazy. No task marks a claim expired.
- A claim is available when any of these holds
  (`crates/hotsheet-ticketing/src/ops.rs:2679-2685`):
  - it is unclaimed;
  - it has no lease;
  - its lease end is at or before now.
- The default lease is 30 minutes in both the CLI and the server
  (`crates/hotsheet-server/src/routes/claims.rs:19`).

| From                    | Event                           | Result                                                                                                                                                                                          |
| ----------------------- | ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| unclaimed or expired    | claim or claim-next by w        | Claimed by w. `claim_count` + 1, a `Claim` event, and a not-started ticket becomes started (`crates/hotsheet-ticketing/src/ops.rs:3163-3177`, `crates/hotsheet-ticketing/src/ops.rs:2806-2828`) |
| live, held by w         | claim by w                      | Idempotent. A `Renew` event is added only if the lease or ETA changes, and the count stays the same (`crates/hotsheet-ticketing/src/ops.rs:3138-3162`)                                          |
| live, held by w         | claim by w2                     | `WrongWorker` (`crates/hotsheet-ticketing/src/ops.rs:3124-3137`)                                                                                                                                |
| live, held by w         | renew by w                      | `Renew` event. An optional attempt fence must match the latest `Claim` id                                                                                                                       |
| expired, last held by w | renew by w                      | Refused: "lease expired; inspect claim history and claim the exact ticket again" (`crates/hotsheet-ticketing/src/ops.rs:3314-3319`)                                                             |
| expired, last held by w | claim by w2 ("steal")           | Same as a new acquisition. No `Release` is written for w, so the history reads Claim w, Renew w, Claim w2                                                                                       |
| claimed                 | release by the holder, or force | `Release` event; fields cleared (`crates/hotsheet-ticketing/src/ops.rs:3185-3210`)                                                                                                              |
| claimed                 | status leaves the active set    | `end_claim` releases it (`crates/hotsheet-ticketing/src/ops.rs:886-889`)                                                                                                                        |

**Claim-next.** It skips tickets that are:

- blocked by `blocked_reason`;
- unavailable;
- in `final_testing`.

Source: `crates/hotsheet-ticketing/src/ops.rs:2791-2796`. Claim-next ignores
dependency edges.

**ETAs.** An ETA must be in the future and no more than 14 days ahead
(`crates/hotsheet-ticketing/src/ops.rs:2701-2766`).

**Session-end safety net.** `release_worker` releases every claim one worker
holds (`crates/hotsheet-ticketing/src/ops.rs:3219-3235`).

**Merges.** When a merge sees both sides change the claim fields, the side
with the newer `updated_at` wins, field by field
(`crates/hotsheet-ticketing/src/merge.rs:125-161`).

Tests:

- `crates/hotsheet-ticketing/src/ops.rs`
  (`exact_claim_is_eligible_holder_idempotent_and_expiry_takeover_safe`)
- `crates/hotsheet-cli/tests/claim_run.rs`
  (`hs2_mfyyj9_expired_renewal_refuses_and_requires_a_new_exact_claim`)

**Stores bound to a `git_ref` backend.** The claim gate runs a
compare-and-swap on the remote before the frontmatter write
(`crates/hotsheet-ticketing/src/claim_gate.rs:283-317`):

- A lapsed marker is replaced by force-with-lease.
- The fencing token becomes `max(prev + 1, now_ms)`
  (`crates/hotsheet-ticketing/src/gitref_cas.rs:283-290`).
- A renew whose marker is missing re-acquires it.

Two ticket-claim paths do not steal, and their release is unconditional:

- **The older `distclaim` path.** It uses a plain push that loses to any
  existing marker, even an expired one. Its sweep then deletes expired markers
  with no lease check (`crates/hotsheet-ticketing/src/distclaim.rs:159-172`,
  `crates/hotsheet-ticketing/src/distclaim.rs:229-243`).
- **Advisory claims.** They are never leases. Several live claims on one
  issue mark it as conflicted
  (`crates/hotsheet-ticketing/src/advisory_claim.rs:77-165`).

#### Resource claims

| Backend                                                                             | Expiry clock        | Renewal                                                                                                                    | Takeover after expiry                                                            | Release of a lapsed claim                            |
| ----------------------------------------------------------------------------------- | ------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | ---------------------------------------------------- |
| Local (`crates/hotsheet-resource-claims/src/local/mod.rs:254-363`)                  | Machine wall clock  | The token holder sets `now + lease`. After expiry the answer is `expired`. Waiters are woken only if the lease got shorter | Implicit overwrite. Fencing becomes the last value + 1, kept even across release | Clears the slot and answers `expired`                |
| Git ref (`crates/hotsheet-resource-claims/src/gitref.rs:215-259`)                   | The claimer's clock | Expiry is checked on the client, then the marker is rewritten by compare-and-swap                                          | Force-with-lease with fencing `max(prev + 1, now_ms)`                            | Deletes the ref and answers `Ok` (see Discrepancies) |
| Upstash (`crates/hotsheet-resource-claims/src/upstash/scripts.rs:46-103`)           | Redis `TIME`        | Lua `RENEW`. Publishes `shortened` when the lease got shorter                                                              | `INCR` on the fencing counter                                                    | `DEL` plus a `released` publish; answers `expired`   |
| External plugin (`crates/hotsheet-resource-claims/src/external/backend.rs:412-516`) | Plugin-defined      | Plugin-defined. Hot Sheet validates the reply                                                                              | Plugin-defined                                                                   | Plugin-defined                                       |

Tests:

- `crates/hotsheet-resource-claims/tests/transitions.rs`
  (`hs2_h3wppy_expiry_frees_the_resource_and_fences_the_old_holder`,
  `hs2_h3wppy_renew_and_release_after_expiry_report_expired`)
- `crates/hotsheet-resource-claims/tests/upstash.rs`
  (`hs2_k9j1rk_a_lapsed_claim_says_expired_until_its_grace_ends_and_fencing_survives`)

**Waiting for a held resource.** The local backend registers a waiter before
its first acquire attempt (`crates/hotsheet-resource-claims/src/local/mod.rs:310-312`).

- **Sleep.** It sleeps on a loopback UDP socket until the earlier of two
  times: its own deadline, measured on the monotonic clock, or the holder's
  lease end (`crates/hotsheet-resource-claims/src/local/mod.rs:185-236`).
- **Wake.** A wake makes it re-inspect the resource.
- **Signals.** A stop signal under `InterruptGuard` ends the wait as
  `interrupted` and removes the registration.

Test: `crates/hotsheet-cli/tests/resource_claims.rs`
(`hs2_qgy1nn_a_wait_ended_by_a_stop_signal_removes_its_registration_and_dies_by_the_signal`).

#### Job leases

**Lease.** A Job lease is 60 s, renewed at a third of its length
(`crates/hotsheet-jobs/src/store.rs:54-65`, `crates/hotsheet-jobs/src/monitor.rs:268-270`).

**Takeover.** `acquire` takes over an active Job whose lease has expired.
It applies the lost and reattach edges in one step and increments the
generation (`crates/hotsheet-jobs/src/store.rs:373-431`). Reports from an
older generation are refused (`crates/hotsheet-jobs/src/store.rs:1004-1017`).

**Lost.** An expired lease becomes `lost` in either case:

- the probe says the owner is gone;
- the probe cannot tell, and 5 more minutes have passed.

Source: `crates/hotsheet-jobs/src/store.rs:676-718`.

Test: `crates/hotsheet-jobs/tests/adversarial.rs`
(`a_stalled_watch_lease_can_be_taken_over_but_a_live_one_cannot`).

| From               | To                                 | Family                      |
| ------------------ | ---------------------------------- | --------------------------- |
| new                | queued                             | all                         |
| queued             | running, cancelled; waiting        | all; waiting for watch only |
| running ↔ waiting  |                                    | all                         |
| running or waiting | succeeded, failed, cancelled, lost | all                         |
| lost               | running or waiting (reattach)      | watch only                  |

Source: `crates/hotsheet-jobs/src/state.rs:10-24`.

### Placement end to end

The path from owner to target is described below; the code is in the CLI
orchestration runtime.

1. **Scheduler.** An applied scheduler pass yields `Start` for a step
   (`crates/hotsheet-cli/src/orchestration/scheduler_apply.rs:139-144`). A
   pass is applied only when all of these hold
   (`crates/hotsheet-cli/src/orchestration/scheduler.rs:1286-1300`):
   - placement is on;
   - the pass is not a dry run;
   - the project is orchestrating;
   - an identified actor is given.

   Otherwise it is a dry run.

2. **Owner authority.** The pass first files an Allow Once through
   `scheduler_gate` (`crates/hotsheet-cli/src/orchestration/scheduler_apply.rs:170-199`).
   `session_start_gated` then calls `authorize`
   (`crates/hotsheet-cli/src/orchestration/access.rs:312-394`), which:
   - requires an identified actor;
   - resolves the target freshly; an unusable target is `TargetUnavailable`;
   - applies the policy's limits (`verdict`);
   - asks the permission ledger with the key (owner, target,
     `session_start`, tool).

   A person acting needs no request. Every result is audited.

3. **Owner record.** The owner records the session as `requested`, then calls
   `POST /checkouts/{c}/target-sessions` on the destination
   (`crates/hotsheet-cli/src/orchestration/sessions.rs:2517-2568`). A local
   destination is called in process; a remote one goes over the reviewed
   peer registration (`crates/hotsheet-cli/src/orchestration/tickets.rs:535-563`).
4. **Target authority.** The target runs no orchestration permission check.
   `target-sessions` is ordinary authenticated checkout data
   (`crates/hotsheet-cli/src/orchestration/request.rs:192-194`). The TLS layer
   requires a read-write device role for a POST
   (`crates/hotsheet-server/src/tls.rs:204-229`).
5. **Target start.** The target runs these steps:
   1. `reserve_with`; a withdrawal fence answers `session_withdrawn`.
   2. An already-started reservation answers `existing`.
   3. Role check.
   4. **Admission**, `decide_admission` over its own snapshot and admission
      file (`crates/hotsheet-cli/src/orchestration/sessions.rs:1348-1417`,
      `crates/hotsheet-cli/src/orchestration/capacity.rs:327-342`).
   5. A cap check on a fresh session listing.
   6. Tool preparation, launch, `mark_started` and the role prompt
      (`crates/hotsheet-cli/src/orchestration/sessions.rs:1431-1524`).
6. **Owner outcome.** On `started`, the owner marks the session running and
   uses the Allow Once. The scheduler then clears the target's refusals
   (`crates/hotsheet-ticketing/src/orchestration/scheduler.rs:361-366`).

Tests:

- `crates/hotsheet-cli/src/orchestration/tests/sessions.rs`
  (`hs2_gdjxgz_the_targets_machine_may_refuse_and_a_refusal_never_launches`)
- `crates/hotsheet-server/tests/remote_servers/sessions.rs`
  (`hs2_crbr8x_the_host_journals_a_remote_orchestrators_start_stop_and_refusal`)

### Speculation

Two separate rule sets use the word "provisional".

**Coordinated work (the coordination pool).**

- **Policy.** The shared `speculation` setting is off by default
  (`crates/hotsheet-ticketing/src/provisional.rs:21-86`).
- **When a prerequisite counts as provisional.** `ticket_readiness`
  (`crates/hotsheet-ticketing/src/provisional.rs:518-563`) requires all of
  these:
  - the prerequisite is in `final_testing`;
  - its newest `CI gate commit: <sha>` note names a commit that is reachable
    (`crates/hotsheet-cli/src/coordination/speculation_runtime.rs:130-132`);
  - that commit's CI gate is pending or unknown;
  - the stacked depth is within `max_depth`.
- **Admission.** `speculation_reservation` admits speculative work only when
  no gate is already red and the base contains every gate commit
  (`crates/hotsheet-cli/src/coordination/speculation_runtime.rs:459-500`).

**Orchestration plans.** Steps can also be provisional. Their standings come
from `crates/hotsheet-ticketing/src/orchestration/provisional.rs`. The
scheduler keeps `100 - risk_weight` percent of a provisional step's score.

**Invalidation when a base CI gate turns red** (view
`HS2-Orch-SpeculationRed`):

1. The CI watch Job for the gate commit reports red.
   `LiveGateProbe::gate` maps it to `GateResult::Red` with the run id and
   attempt (`crates/hotsheet-cli/src/coordination/speculation_runtime.rs:134-156`).
2. `speculation-sync` calls `SpeculationLedger::apply`, which:
   - sets the gate red;
   - derives `at_risk` for each dependent;
   - records a risk notice with a recovery
     (`crates/hotsheet-cli/src/coordination/speculation.rs:429-525`). The
     recovery is `ReverifyByCoveringRun` when the dependent is already in CI
     or done, and `RebaseOntoFix` otherwise
     (`crates/hotsheet-cli/src/coordination/speculation.rs:886-891`).

   A verdict is sticky: only another verdict replaces it.

3. Idempotent notes are written on the dependent and on the prerequisite
   (`crates/hotsheet-cli/src/coordination/speculation_runtime.rs:536-592`).
4. While `repair_first` lists the red prerequisite:
   - new provisional reservations are refused;
   - integration of at-risk work is refused
     (`crates/hotsheet-cli/src/coordination/speculation.rs:627-650`).
5. Recovery happens in one of two ways:
   - A later sync finds a green, reachable fix commit and rebases the landed
     work onto it
     (`crates/hotsheet-cli/src/coordination/speculation_runtime.rs:715-781`).
   - Someone runs `speculation-rebase` or `speculation-reopen`.

**The orchestration path.** A failed gate behaves the same way:

- Readiness returns `blocked` with `gate_failed`
  (`crates/hotsheet-ticketing/src/orchestration/readiness.rs:488-489`).
- The orchestration ledger moves stacked work to `at_risk`
  (`crates/hotsheet-ticketing/src/orchestration/provisional.rs:134-139`).

Tests:

- `crates/hotsheet-cli/src/coordination/speculation_tests.rs`
  (`hs2_2sjpch_red_then_fix_with_parked_and_integrated_dependents_across_a_restart`)
- `crates/hotsheet-cli/src/orchestration/tests/provisional.rs`
  (`hs2_jsf985_a_red_gate_puts_the_work_at_risk_and_its_fix_rebases_it`)

### Coordination pool, worker plans and development strategies

The pool lives in `<git-common-dir>/hotsheet-coordination/`
(`crates/hotsheet-cli/src/coordination/mod.rs:129-147`):

- `state.json`, with `state.lock` beside it, holds sessions, assignments and
  the next plan. Pool versions 1 to 9 are accepted.
- Each session lease is between 10 and 3600 seconds
  (`crates/hotsheet-cli/src/coordination/mod.rs:405-406`).

**Worker plan.** A worker plan holds at most 128 rows and 128 retained events
(`crates/hotsheet-cli/src/coordination/contract.rs:266-277`).

- Server routes:
  - `PUT /checkouts/{reference}/worker-plan`
  - `GET .../worker-plan/events`
  - `POST .../worker-plan/dispatch`
- The routes call the CLI coordinator in `spawn_blocking`
  (`crates/hotsheet-server/src/routes/checkout_workers.rs:476-538`) and emit no
  change event.
- Dispatch never claims the ticket; the worker claims it itself
  (`crates/hotsheet-cli/src/coordination/next_plan.rs:157`).
- The server reads live workers from `state.json` without writing
  (`crates/hotsheet-server/src/live_worker_plan.rs:1-99`).

**Development strategy.** The resolver reads the shared setting
`development_strategy` from `.hotsheet2/settings.json`. Versions 1 and 2 are
accepted (`crates/hotsheet-cli/src/coordination/development_strategy.rs:15-16`).

- **Precedence:** ticket, then session, then project (`default_custom`, else
  `default`), then unconfigured
  (`crates/hotsheet-cli/src/coordination/development_strategy.rs:330-345`).
  There is no user level.
- **Outputs**
  (`crates/hotsheet-cli/src/coordination/development_strategy.rs:395-418`):
  - `integration`;
  - `local_gate`;
  - `require_green_main_sha`, which is false only for `ci-first`;
  - `attribute_red_ci_before_completion`, which is always true;
  - `green_main_goal_minutes`, default 360;
  - `local_budget_minutes`, default 10, `ci-first` only;
  - `push_without_receipt`, true only for `ci-first`.
- **Who reads the session and ticket overrides.**
  `HOTSHEET_DEVELOPMENT_STRATEGY` and the ticket tag
  `development-strategy:<name>` are read by `scripts/gate-ticket-profile.mjs`,
  not by Rust.

Test: `crates/hotsheet-cli/src/coordination/development_strategy_tests.rs`
(`hs2_bf79yx_ticket_then_session_then_project_precedence_keeps_gates`).

### Concurrency, tasks and locks

| Lock, task or thread                                         | Owner                                                                 | Scope and shutdown                                                                                                                                                                               |
| ------------------------------------------------------------ | --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `<file>.json.lock` per orchestration file                    | `Storage::lock`                                                       | Taken per mutation. The OS releases it when the process exits. Lock files are never removed                                                                                                      |
| `orchestration-run-operations/NN.json.lock`, 64 stripes      | `RunRegistry::operation_lock`                                         | One run's read, act and record sequence. Reentrant per thread (`crates/hotsheet-ticketing/src/orchestration/runs.rs:375-396`)                                                                    |
| `orchestration-scheduler-pass-<owner>.json.lock`             | scheduler pass                                                        | One pass per owner (`crates/hotsheet-ticketing/src/orchestration/scheduler.rs:2622-2633`)                                                                                                        |
| `<store>/.hotsheet-note-mutation.lock`                       | ticket transaction (core)                                             | Every claim mutation                                                                                                                                                                             |
| `.resource-claims-config.lock`; `namespaces/<ns>/state.lock` | backend registry; local resource backend                              | Released before waiters are woken                                                                                                                                                                |
| `jobs/.monitor.lock`, then `jobs/<checkout>/.lock`           | Job monitor                                                           | The slot is held for the monitor's life; checkout locks are taken one at a time. The monitor exits after 30 s idle or on a `.monitor.stop` naming it (`crates/hotsheet-jobs/src/slot.rs:66-147`) |
| `hotsheet-path-watcher` thread per `BackgroundWatcher`       | Jobs watchers                                                         | Restart backoff from 100 ms to 5 s (`crates/hotsheet-jobs/src/watch.rs:110-115`)                                                                                                                 |
| Resource waiter UDP socket and `InterruptGuard` self-pipe    | local resource backend                                                | The socket file is removed on drop. The guard restores signal handlers and re-raises the caught signal                                                                                           |
| Tokio loops: gate follower, scheduler, run recovery          | server (`crates/hotsheet-server/src/routes/orchestration.rs:370-560`) | One loop per server, home and owner. Backoff from 1 s to 60 s. A scheduler step waits at most 25 s (`crates/hotsheet-cli/src/orchestration/scheduler.rs:41`)                                     |
| Distributed work loop                                        | server (`crates/hotsheet-server/src/dist_work_loop.rs:28`)            | Every 15 s: sweep, then work once                                                                                                                                                                |
| Advisory ledger `Mutex`                                      | server (`crates/hotsheet-server/src/lib.rs:987-991`)                  | SQLite busy timeout 5 s                                                                                                                                                                          |

### Failure and recovery

**Server start.** `spawn_run_recovery`
(`crates/hotsheet-server/src/routes/orchestration.rs:522-560`):

- starts followers for every owner that has an unfinished run, a waiting gate
  or plan tickets;
- starts the scheduler for automatic owners;
- reaps orphaned validations;
- re-surfaces waiting permission requests.

**First target-follower step.** It reconciles each run through `run_reconcile`
(`crates/hotsheet-cli/src/orchestration/recovery.rs:127-201`):

1. Align the run with the target ticket.
2. Repeat a pending cancel if the target answers.
3. Wind down the sessions of terminal runs.
4. Otherwise assess the sources and store the reconciliation only when the
   facts changed.

**What is retried.**

| Item                                | Retry behavior                                                                                            |
| ----------------------------------- | --------------------------------------------------------------------------------------------------------- |
| Cancel                              | Repeated by the recovery step above                                                                       |
| Dispatch of a run still `requested` | At most 3 times per stored outcome (`crates/hotsheet-cli/src/orchestration/scheduler_dispositions.rs:22`) |
| Lost gate watch                     | Reattached                                                                                                |
| Refused placement                   | Backs off from 60 s, doubling to 30 min                                                                   |
| Job poll                            | Backs off up to 300 s                                                                                     |
| Upstash 429                         | At most 3 attempts                                                                                        |
| Git-ref race                        | At most 4 compare-and-swap attempts                                                                       |

**What is lost.**

- In-memory follower cursors and authorization caches
  (`crates/hotsheet-cli/src/orchestration/gates.rs:382-391`).
- A torn journal tail.
- Journal entries beyond retention. They are reported as a `gap`.
- Feed sequence numbers skipped by a crash.

**What is held for a person.**

- Intervened runs always need attention.
- Unverifiable sources hold the run.
- Claim conflicts are pushed once to the losing terminal.

**Fail-closed cases.** Damaged or newer data is never overwritten. It fails
closed everywhere, with three exceptions:

- line observations read as empty;
- the CLI treats unreadable re-plan memory as empty;
- `ProcessJournal::list` fails as a whole if any record is damaged
  (`crates/hotsheet-process-journal/src/lib.rs:869-886`).

### Configuration and environment

The crates in this area read no environment variables themselves. The
variables below are read by their hosts.

| Name                                                                                                                                                                                                                    | Read by                                                                   | Default                                     | Purpose                                                                                                                                                                                                                                                |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `HOTSHEET_HOME`                                                                                                                                                                                                         | server and CLI                                                            | platform machine home                       | Root for every `HOME/` path above                                                                                                                                                                                                                      |
| `HOTSHEET_WORKER_ID`                                                                                                                                                                                                    | AI launch environment                                                     | none                                        | Launched session worker id; matches claim-conflict pushes to a terminal (`crates/hotsheet-server/src/claim_conflict_push.rs:52-55`). Claim commands do not read it; `--worker` defaults to `worker` (`crates/hotsheet-server/src/routes/claims.rs:73`) |
| `HOTSHEET_ACTOR_ROLE`, `HOTSHEET_ACTOR_ID`                                                                                                                                                                              | CLI attribution                                                           | none                                        | Set to `ai` and the tool or worker id by launchers                                                                                                                                                                                                     |
| `HOTSHEET_DEVELOPMENT_STRATEGY`                                                                                                                                                                                         | `scripts/gate-ticket-profile.mjs`                                         | unset                                       | Session strategy override                                                                                                                                                                                                                              |
| `GH_TOKEN`, `GITHUB_TOKEN`, `GITLAB_TOKEN`, `BUILDKITE_API_TOKEN`, `CIRCLECI_TOKEN`, `JENKINS_USER`, `JENKINS_API_TOKEN`, `AZURE_DEVOPS_EXT_PAT`, `BITBUCKET_ACCESS_TOKEN`, `BITBUCKET_USERNAME`, `BITBUCKET_API_TOKEN` | CLI job adapters (`crates/hotsheet-cli/src/dispatch/job_ci_rest.rs:1-32`) | none; GitHub falls back to `gh auth token`  | CI credentials                                                                                                                                                                                                                                         |
| `HOTSHEET_GITHUB_API_URL`, `HOTSHEET_GITLAB_API_URL`, `HOTSHEET_BUILDKITE_API_URL`, `HOTSHEET_CIRCLECI_API_URL`, `HOTSHEET_AZURE_DEVOPS_API_URL`, `HOTSHEET_BITBUCKET_API_URL`, `HOTSHEET_JENKINS_URL`                  | CLI job adapters                                                          | provider public API; Jenkins has no default | API roots                                                                                                                                                                                                                                              |
| `HOTSHEET_JOB_WATCH_FIXTURES`                                                                                                                                                                                           | CLI debug builds                                                          | unset                                       | Fixture watch sources (`crates/hotsheet-jobs/src/fixture.rs:17-40`)                                                                                                                                                                                    |
| `HOTSHEET_TEST_UPSTASH_URL`, `HOTSHEET_TEST_UPSTASH_TOKEN`                                                                                                                                                              | live Upstash test only                                                    | unset                                       | Enables `crates/hotsheet-resource-claims/tests/upstash_live.rs`                                                                                                                                                                                        |

Settings keys:

- **Shared** (`.hotsheet2/settings.json`): `project_type`, `speculation`,
  `development_strategy`, `integration_strategies`.
- **Checked-in policy files:** `orchestration.json`,
  `orchestration-plan.json`, `orchestration-policy.json`,
  `validation-profiles.json`, `resource-claims.json`.
- **Machine-owner file:** `orchestration-admission.json`.

The defaults are in the coverage rows for `orchPolicy`, `orchAdmission` and
`orchSpeculation`.

### Dynamic views

#### `HS2-Orch-LeaseExpiry`

1. Worker w1 claims the exact ticket. A `Claim` event and a lease end are
   written (`crates/hotsheet-ticketing/src/ops.rs:3163-3177`).
2. w1 stops renewing. Once the lease end is at or before now, the ticket
   reads available. Nothing runs to mark this
   (`crates/hotsheet-ticketing/src/ops.rs:2679-2685`).
3. **Failure path.** A late renew from w1 is refused with "lease expired"
   (`crates/hotsheet-ticketing/src/ops.rs:3314-3319`). Test:
   `crates/hotsheet-cli/tests/claim_run.rs`
   (`hs2_mfyyj9_expired_renewal_refuses_and_requires_a_new_exact_claim`).
4. w2 runs claim-next or an exact claim. `prepare_claim` takes the
   new-acquisition branch (`crates/hotsheet-ticketing/src/ops.rs:2791-2796`).
5. For a `git_ref`-bound store, `gate_claim` runs `GitRefCas::acquire`
   (`crates/hotsheet-ticketing/src/claim_gate.rs:283-317`).
6. A force-with-lease push replaces the lapsed marker with a larger fencing
   token. A rejected push is a lost race and is retried, up to 4 attempts
   (`crates/hotsheet-ticketing/src/gitref_cas.rs:136`,
   `crates/hotsheet-ticketing/src/gitref_cas.rs:244-301`). Test:
   `crates/hotsheet-ticketing/src/gitref_cas_tests.rs`
   (`hs2_wjf0d8_a_lapsed_claim_is_taken_with_a_larger_fencing_token`).
7. The frontmatter commit carries `Claim` w2 and `claim_count` + 1. w1's
   history stays, and no `Release` is written for w1. Test:
   `crates/hotsheet-ticketing/src/ops.rs`
   (`exact_claim_is_eligible_holder_idempotent_and_expiry_takeover_safe`).

#### `HS2-Orch-RefusedPlacement`

The `server` container plays both roles: the owner, and the target reached
over the paired peer API.

1. **Owner.** The applied pass yields `Start`
   (`crates/hotsheet-cli/src/orchestration/scheduler_apply.rs:139-144`).
2. **Owner.** `authorize` finds the pass's Allow Once and audits it
   (`crates/hotsheet-cli/src/orchestration/sessions.rs:2497-2514`).
3. **Owner.** The session is recorded as requested, then the POST goes to the
   peer (`crates/hotsheet-cli/src/orchestration/sessions.rs:2517-2568`).
4. **Peer.** `reserve_with` writes a reservation with `started=false`
   (`crates/hotsheet-ticketing/src/orchestration/sessions.rs:1378-1448`).
5. **Peer.** `decide_admission` refuses, for example with
   `session_cap_reached`
   (`crates/hotsheet-ticketing/src/orchestration/capacity.rs:886-938`). Other
   refusals at this point:
   - `session_role_active`;
   - a cap broken on the fresh listing;
   - `tool_setup_failed`;
   - `session_host_unavailable`;
   - `session_start_failed`.

   Sources: `crates/hotsheet-cli/src/orchestration/sessions.rs:1399-1474`.

6. **Peer.** `refuse_with` releases the reservation and answers HTTP 200 with
   `outcome: refused`, the reason and the admission detail
   (`crates/hotsheet-cli/src/orchestration/sessions.rs:1369-1379`,
   `crates/hotsheet-cli/src/orchestration/sessions.rs:718-735`).
7. **Peer.** The hosted audit journals `start` as `denied`
   (`crates/hotsheet-cli/src/orchestration/hosted_audit.rs:242-252`).
8. **Owner.** The session is marked failed with the host's reason. The
   outcome is audited as failed, and the Allow Once stays unused
   (`crates/hotsheet-cli/src/orchestration/sessions.rs:2588-2595`,
   `crates/hotsheet-cli/src/orchestration/access.rs:484-501`). A transient
   transport error instead leaves the session `requested`.
9. **Owner.** The scheduler records `Refused`
   (`crates/hotsheet-cli/src/orchestration/scheduler_apply.rs:262-292`).
10. **Owner.** Placement memory persists the refusal count, time and reason
    (`crates/hotsheet-ticketing/src/orchestration/scheduler.rs:2712-2737`).
    The target is skipped for 60 s, doubling up to 30 min, or until a
    followed capacity change. The next attempt uses a new operation that
    replaces the failed session
    (`crates/hotsheet-cli/src/orchestration/session_executor.rs:342-347`).

Tests:

- `crates/hotsheet-server/tests/remote_servers/sessions.rs`
  (`hs2_crbr8x_the_host_journals_a_remote_orchestrators_start_stop_and_refusal`)
- `crates/hotsheet-cli/src/orchestration/tests/scheduler.rs`
  (`hs2_ecwg50_a_refused_start_backs_off_and_leaves_the_target_alone`)

#### `HS2-Orch-FailedCiGate`

1. The monitor serves the held watch Job
   (`crates/hotsheet-jobs/src/monitor.rs:366-436`).
2. The GitHub adapter finds that the newest covering run concluded `failure`
   and decides red (`crates/hotsheet-jobs/src/github.rs:740-743`,
   `crates/hotsheet-jobs/src/github.rs:1018-1019`).
3. The monitor finishes the Job as failed with `gate=red`, which bumps the
   revision (`crates/hotsheet-jobs/src/monitor.rs:555-559`).
4. The server's gate follower wakes on the store change
   (`crates/hotsheet-server/src/routes/orchestration.rs:370-414`). `gate_of`
   reads `Red` (`crates/hotsheet-jobs/src/ci.rs:224-226`).
5. `GateRegistry::apply` takes a typed failed reading with covering evidence,
   so pending becomes failed
   (`crates/hotsheet-cli/src/orchestration/gates.rs:550-566`,
   `crates/hotsheet-ticketing/src/orchestration/gates.rs:343-413`).
6. A deduplicated gate run event is appended
   (`crates/hotsheet-cli/src/orchestration/gates.rs:487-516`).
7. Readiness gives dependents `blocked` with `gate_failed`
   (`crates/hotsheet-ticketing/src/orchestration/readiness.rs:488-489`).
8. Provisional work goes at risk
   (`crates/hotsheet-ticketing/src/orchestration/provisional.rs:134-139`).
9. The run event wakes a scheduler pass. The red-gate stack score
   (`crates/hotsheet-ticketing/src/orchestration/scheduler.rs:429-433`) ranks
   the repair first.

Tests:

- `crates/hotsheet-cli/src/orchestration/tests/gates.rs`
  (`hs2_5jbypv_the_github_actions_adapter_reads_the_watch_job_and_wakes_on_its_change`)
- `crates/hotsheet-ticketing/src/orchestration/readiness_tests.rs`
  (`hs2_42tara_ticket_prerequisite_table_covers_every_gate_state`)

#### `HS2-Orch-WorkerPlanDispatch`

1. `PUT worker-plan` calls `replace_next_plan` under `state.lock`
   (`crates/hotsheet-cli/src/coordination/next_plan.rs:17-105`). It checks
   authority, generation, the expected revision and that no assignment is
   pending, then records a `Replaced` event
   (`crates/hotsheet-cli/src/coordination/next_plan.rs:130-144`).
2. `POST worker-plan/dispatch` re-checks the row under the ticket transaction
   and `state.lock` (`crates/hotsheet-cli/src/coordination/next_plan.rs:249-328`).
   It requires all of these:
   - the worker holds no live claim;
   - the ticket is Up Next, open, unblocked and claim-available;
   - its priority and slug are unchanged;
   - its dependencies are closed, or a speculative reservation passes.

   **Failure path:** any failed check refuses the dispatch and leaves the row.

3. With speculation enabled, provisional readiness decides whether unfinished
   dependencies are acceptable
   (`crates/hotsheet-cli/src/coordination/next_plan.rs:194-205`).
4. In one lock session, dispatch:
   - creates the `offered` assignment;
   - records the `Assigned` event;
   - writes the speculation record.

   Sources: `crates/hotsheet-cli/src/coordination/mod.rs:676-796`,
   `crates/hotsheet-cli/src/coordination/next_plan.rs:331-345`.

5. The worker's `coordination next` wait wakes on `state.json`
   (`crates/hotsheet-cli/src/coordination/wait.rs:313-329`). The worker then
   accepts, moving the assignment from offered to accepted.
6. The worker claims the exact ticket itself; dispatch never claims it.
7. The claim is written to the ticket frontmatter.

Test: `crates/hotsheet-cli/src/coordination/next_plan_tests.rs`
(`hs2_xcmavt_restart_replacement_and_dispatch_preserve_exact_plan`).

#### `HS2-Orch-SpeculationRed`

This view is narrated under [Speculation](#speculation).

### Discrepancies

Each discrepancy names the follow-up ticket filed from HS2-RJB111.

1. **Git-ref resource-claim semantics differ from §36.**
   - `docs/36-resource-claims.md:60` says the first grant gets fencing token 1.
     The Git-ref backend's first token is `max(now_ms, 1)`
     (`crates/hotsheet-ticketing/src/gitref_cas.rs:288`).
   - `docs/36-resource-claims.md:67` says releasing a lapsed claim answers
     `expired`. Git-ref answers `Ok` (`crates/hotsheet-resource-claims/src/gitref.rs:244-252`).
   - `docs/36-resource-claims.md:75` says the latest token after release
     answers `expired`. Git-ref answers `invalid`
     (`crates/hotsheet-resource-claims/src/gitref.rs:324-335`). Follow-up: HS2-K9ZRKQ.
2. **Resource-claim doc is stale.**
   - §36 calls Upstash and external backends design-only and lists only
     `local` and `git_ref` kinds (`docs/36-resource-claims.md:17-18`,
     `docs/36-resource-claims.md:400`). Both are implemented
     (`crates/hotsheet-resource-claims/src/config.rs:135-136`).
   - §36 says tokens are stored as a keyed HMAC digest
     (`docs/36-resource-claims.md:112-114`). Remote backends store an unkeyed
     SHA-256 (`crates/hotsheet-resource-claims/src/token.rs:81-84`).
   - The error table omits `interrupted`.
   - §36 lists four reserved plugin ids; the code reserves seven.
   - §36 says a 429 is retried three times. The code makes three attempts in
     total (`crates/hotsheet-resource-claims/src/upstash/mod.rs:41`). Follow-up: HS2-KC9Y7R.
3. **The backend test probes the wrong ref prefix.** The Git-ref backend test
   lists `refs/hotsheet/claims/*`, the ticket prefix, not the resource prefix
   (`crates/hotsheet-resource-claims/src/config.rs:404`). §36 says it probes
   the claim refs (`docs/36-resource-claims.md:439`). Follow-up: HS2-K9ZRKQ.
4. **`held_namespaces` misses some namespaces.** It enumerates only the
   machine local backend, so project-scoped and remote namespaces are missed
   (`crates/hotsheet-resource-claims/src/service.rs:34-38`). This contradicts
   its own comment and §36.13. Follow-up: HS2-THCDVM.
5. **Job owner liveness is never wired.** §33 says an owned Job whose process
   is gone becomes lost (`docs/33-jobs.md:31-32`, `docs/33-jobs.md:165-166`).
   No production code sets `launch_id`
   (`crates/hotsheet-cli/src/orchestration/validation_runner.rs:116`), and
   the monitor reconciles with `NoProbe`
   (`crates/hotsheet-jobs/src/monitor.rs:311`). Follow-up: HS2-X6Q8ZJ.
6. **Smaller Jobs doc mismatches.**
   - The feed cap is per Job, not per checkout (`docs/33-jobs.md:93`,
     `crates/hotsheet-jobs/src/store.rs:520-526`).
   - Job directories are created without an explicit owner-only mode
     (`docs/33-jobs.md:153`, `crates/hotsheet-jobs/src/store.rs:306`).
   - The GitHub and CircleCI paging limits are undocumented. Follow-up: HS2-54WH8X.
7. **The `distclaim` sweep can delete a live marker.**
   - `sweep_expired` deletes markers with an unconditional push and no lease
     check (`crates/hotsheet-ticketing/src/distclaim.rs:159-172`). A marker
     re-taken between the list and the delete can therefore be removed.
   - `docs/08-distributed-and-remote.md:528-530` describes force-with-lease
     stealing that only `GitRefCas` implements. Follow-up: HS2-0CSJ0Q.
8. **Hosted validation pruning can forget a running execution.** At capacity
   it removes index 0 when no record has finished
   (`crates/hotsheet-ticketing/src/orchestration/hosted_validations.rs:269-276`),
   so `known()` can report a running execution as unknown. §26.12 is silent
   on this (`docs/26-orchestration-projects.md:2013-2014`). Follow-up: HS2-SRRHMV.
9. **§26 contradicts the policy code.**
   - The policy table gives `placement.automatic` a default of false
     (`docs/26-orchestration-projects.md:888`). The code treats unset as Auto
     (`crates/hotsheet-ticketing/src/orchestration/policy.rs:538-540`), as
     does §26 itself at line 968.
   - `dispatcher_wait_minutes`, `join_minutes` and `role_sessions` are
     missing from that table.
   - `policy_target_not_permitted` is documented but never produced
     (`crates/hotsheet-ticketing/src/orchestration/policy.rs:1109-1137`). Follow-up: HS2-F1AS2G.
10. **§26 contradicts the binding and plan code.**
    - A site bound to another ticket store is checked after the probe, not
      before it (`docs/26-orchestration-projects.md:218-221`,
      `crates/hotsheet-ticketing/src/orchestration/workflow.rs:399-406`).
    - Both sides removing a step merges cleanly; the doc table says it
      conflicts (`docs/26-orchestration-projects.md:5740`,
      `crates/hotsheet-ticketing/src/orchestration/plan_update.rs:198-205`).
    - A validation prerequisite's `ticket` field is required by the core type
      (`crates/hotsheet-ticketing/src/orchestration/readiness.rs:62`). Follow-up: HS2-F1AS2G.
11. **§26 omits several run and gate details.**
    - The gate table omits awaiting approval to passed for reviews.
    - The run-event kind list omits `provisional`.
    - An intervened run is not "active", so a second run on the same ticket
      can begin (`crates/hotsheet-ticketing/src/orchestration/runs.rs:47-50`;
      no test found).
    - The wave estimate uses completed runs only.
    - The refused-start code list omits `session_withdrawn`.
    - Line observations are filtered on read, not pruned
      (`docs/26-orchestration-projects.md:6437`).
    - The permission ledger never prunes rules or denied requests
      (`docs/26-orchestration-projects.md:689-690`). Follow-up: HS2-F1AS2G and HS2-0P90V8.
12. **Claim docs in §02, §04 and §05 overstate the code.**
    - "Poison quarantine" and "claim-next over the index" are not implemented
      (`docs/05-ai-tool-plugins.md:2297-2299`). Claim-next lists every ticket
      file (`crates/hotsheet-ticketing/src/ops.rs:2853-2871`).
    - "Newest lease wins" on merge is really newest `updated_at`
      (`docs/02-ticket-storage.md:782-783`). Follow-up: HS2-MQDY8B.
13. **Development strategies (§34).** The doc names only policy `version: 1`;
    the code also accepts version 2 and writes it for custom strategies
    (`crates/hotsheet-cli/src/coordination/development_strategy.rs:16`).
    Precedence matches the doc. Follow-up: HS2-MQDY8B.

Unresolved claims for this area are in the [register](../unresolved.md) (ORCH-U1, ORCH-U2, ORCH-U3, ORCH-U4, ORCH-U5, ORCH-U6).
