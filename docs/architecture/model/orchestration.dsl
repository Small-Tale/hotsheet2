// Orchestration, coordination, resource claims and jobs domain libraries.
// Owned by HS2-RJB111. Included inside the hs2 software system block by system.dsl.
// Keep element identifiers stable (README "Model conventions").

    orchLib = container "Orchestration and coordination libraries" "Linked Rust domain code for orchestration declarations, placement and runs, ticket and advisory claims, speculation rules, resource claims and job observation. Linked into the server and CLI; not a process of its own. The runtime composition (OrchestrationRuntime) and the coordination pool with worker plans live in the hotsheet-cli crate as cliOrchestration and cliCoordination." "Rust crates" "Library" {
        url "${SOURCE}/crates/hotsheet-ticketing/src/orchestration"

        // Declarations, plans and policy (crate hotsheet-ticketing, module orchestration).
        orchDeclarations = component "Target declarations and bindings" "Checked-in target manifest with sites, reviewed machine-local bindings fenced against concurrent probes, bounded atomic JSON storage, the action vocabulary and the project type decision." "Rust / JSON files" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/orchestration/workflow.rs"
        }
        orchPlanning = component "Plans and readiness" "Checked-in shared plan steps, machine-local ticket reads and published levels, pure readiness evaluation, Update Plan drafts with a three-way merge, the Propose Plan decision and re-plan memory." "Rust / JSON files" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/orchestration/readiness.rs"
        }
        orchPolicy = component "Throughput policy" "Checked-in placement, machine, tool, speculation and wind-down limits with compare-and-swap writes and a pure cap check. Grants nothing by itself." "Rust / JSON file" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/orchestration/policy.rs"
        }
        orchPermissions = component "Permission ledger and audit journals" "Per-operation permission requests, Always Allow rules and judgment-call decisions, plus the append-only owner audit, run-event and hosted-session journals." "Rust / JSON and JSONL files" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/orchestration/permissions.rs"
        }

        // Placement and sessions.
        orchAdmission = component "Capacity and admission" "Capacity snapshots and their jitter-free version, the pure admission decision, the machine owner's stricter-only admission file, host capabilities, step resource needs and AI tool account capacity." "Rust / JSON files" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/orchestration/capacity.rs"
        }
        orchSessions = component "Session registries" "Owner-side session records and host-side reservations with withdrawal fences, idle observations, and the pure assignment, join, role-prompt, safe-point and stop decisions." "Rust / JSON files" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/orchestration/sessions.rs"
        }
        orchScheduler = component "Scheduler" "Pure, order-independent placement pass that scores ready work and decides Start, Keep or WindDown per session, with refusal backoff memory, a decision journal and a per-owner pass lock." "Rust" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/orchestration/scheduler.rs"
        }

        // Runs, gates and recovery.
        orchRuns = component "Dispatch runs" "Machine-local record of each dispatch attempt with its state graph, bounded history, observations and per-run operation locks." "Rust / JSON file" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/orchestration/runs.rs"
        }
        orchGates = component "Gates and validation steps" "CI, deployment and review gates tied to one commit and moved only by typed readings, plus validation step definitions, immutable results and the host's accepted executions." "Rust / JSON files" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/orchestration/gates.rs"
        }
        orchReconcile = component "Run reconciliation" "Pure alignment of a run with its target ticket and assessment of CI, deployment, validation and session sources into a disposition." "Rust" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/orchestration/reconcile.rs"
        }
        orchProgress = component "Progress projections" "Pure timeline, progress, utilization and external-wait summaries, target lines, and the host's store of lines pushed by owners." "Rust" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/orchestration/timeline.rs"
        }
        orchSpeculation = component "Speculation rules" "Shared speculation policy and the pure provisional readiness rule for coordinated work, plus the orchestration provisional ledger that maps a CI rollup to red or green." "Rust / JSON file" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/provisional.rs"
        }

        // Ticket, advisory and distributed claims.
        orchTicketClaims = component "Ticket claim gate and backend registry" "Routes exact ticket claims of a Git-ref-bound store through a compare-and-swap marker, records provisional claims and conflicts, encodes holder labels, and keeps machine and project claim-backend registries and the host id." "Rust / JSON files" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/claim_gate.rs"
        }
        orchGitRefClaims = component "Git ref compare-and-swap" "Claim markers as orphan commits under refs/hotsheet, written with force-with-lease pushes, and the older self-claim loop over origin." "Rust / Git" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/gitref_cas.rs"
        }
        orchAdvisoryClaims = component "Advisory claim rules and replay ledger" "Reconciles provider claim markers as evidence, never as a lease, and keeps a durable SQLite ledger of provisional advisory claims for replay." "Rust / SQLite" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/advisory_claim.rs"
        }

        // Resource claims (crate hotsheet-resource-claims).
        orchResourceClaims = component "Resource claim service and local backend" "Blocking lease contract with fencing tokens, request routing, configuration resolution, the machine-local file backend with UDP wake waiters, signal interruption and the conformance suite." "hotsheet-resource-claims" {
            url "${SOURCE}/crates/hotsheet-resource-claims/src/service.rs"
        }
        orchResourceBackends = component "Remote resource claim backends" "Git-ref backend with an offline provisional fallback, Upstash Redis REST backend with Lua scripts and SSE waits, and approved external plugin subprocesses." "hotsheet-resource-claims / HTTPS / Git / subprocess" {
            url "${SOURCE}/crates/hotsheet-resource-claims/src/gitref.rs"
        }

        // Jobs (crate hotsheet-jobs).
        orchJobStore = component "Job store and monitor" "Per-checkout Job records with leases, feeds and history, the single-instance watch monitor loop, file-change waiters and the CI gate view consumed by orchestration." "hotsheet-jobs / JSON files" {
            url "${SOURCE}/crates/hotsheet-jobs/src/store.rs"
        }
        orchCiAdapters = component "CI watch adapters" "GitHub Actions, GitLab CI, Buildkite, CircleCI, Jenkins, Azure Pipelines and Bitbucket Pipelines watch sources that turn provider runs into green, red, cancelled or superseded observations." "hotsheet-jobs / REST port" {
            url "${SOURCE}/crates/hotsheet-jobs/src/github.rs"
        }
    }
