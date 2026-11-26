// Orchestration and coordination views.
// Owned by HS2-RJB111. New views use keys prefixed HS2-Orch-.

component orchLib "HS2-Orch-Placement" "Plans, policy, permissions, capacity, sessions and the scheduler that places work." {
    include orchDeclarations orchPlanning orchPolicy orchPermissions orchAdmission orchSessions orchScheduler server checkoutData
    autoLayout lr
}
component orchLib "HS2-Orch-RunsAndGates" "Runs, gates, validation, reconciliation, speculation and progress projections." {
    include orchPlanning orchRuns orchGates orchReconcile orchProgress orchSpeculation orchPermissions server machineData
    autoLayout lr
}
component orchLib "HS2-Orch-Claims" "Ticket claims, Git ref markers, advisory claims and resource claim backends." {
    include orchTicketClaims orchGitRefClaims orchAdvisoryClaims orchResourceClaims orchResourceBackends coreLib server cli tickets machineData gitRemote sharedClaims
    autoLayout lr
}
component orchLib "HS2-Orch-Jobs" "Job store, watch monitor and CI adapters." {
    include orchJobStore orchCiAdapters server cli coreLib machineData github gitlab ciServices
    autoLayout lr
}

dynamic orchLib "HS2-Orch-LeaseExpiry" "A ticket claim lease lapses, a late renewal is refused and another worker takes the ticket over through a Git-ref-gated store." {
    cli -> coreLib "Worker w1 claims the exact ticket; a Claim event and a lease end are written"
    cli -> coreLib "w1 stops renewing; once the lease end is at or before now the ticket reads available"
    cli -> coreLib "A late renew by w1 is refused: lease expired, claim the exact ticket again"
    cli -> coreLib "Worker w2 runs claim-next or an exact claim and takes the new-acquisition branch"
    coreLib -> orchTicketClaims "The store is bound to a git_ref backend, so the claim is gated first"
    orchTicketClaims -> orchGitRefClaims "Acquire: the lapsed marker of another claim may be replaced"
    orchGitRefClaims -> gitRemote "Force-with-lease push with a larger fencing token; a rejected push is a lost race and is retried up to 4 times"
    cli -> tickets "Commit Claim w2 and claim_count + 1; w1's Claim and Renew history stays, with no Release for w1"
    autoLayout lr
}

dynamic orchLib "HS2-Orch-RefusedPlacement" "An owner places a session on a remote target that refuses admission. The server container plays the owner and, over the paired peer API, the target." {
    server -> orchScheduler "Owner: the applied scheduler pass yields Start for a ready step"
    server -> orchPermissions "Owner: authorize session_start; the pass's Allow Once is found and audited"
    server -> orchSessions "Owner: record the session as requested, then POST /target-sessions to the peer"
    server -> orchSessions "Peer: reserve the (checkout, operation) unless a withdrawal fence holds it"
    server -> orchAdmission "Peer: decide admission from its own snapshot and admission file; refused, for example session_cap_reached"
    server -> orchSessions "Peer: release the reservation and answer 200 with outcome refused and the reason"
    server -> orchPermissions "Peer: journal the hosted start as denied with the reason"
    server -> orchSessions "Owner: mark the session failed with the host's reason; the Allow Once stays unused"
    server -> orchScheduler "Owner: record the refusal for the target"
    orchScheduler -> machineData "Persist refusals, time and reason; skip the target for 60 s doubling to 30 min, or until a followed capacity change"
    autoLayout lr
}

dynamic orchLib "HS2-Orch-FailedCiGate" "A red GitHub Actions run fails a CI gate, blocks dependent steps, puts provisional work at risk and ranks the repair first." {
    orchJobStore -> orchCiAdapters "The monitor polls the held watch Job for the gate commit"
    orchCiAdapters -> github "Read workflow runs; the newest covering run concluded failure"
    orchJobStore -> machineData "Finish the Job as failed with gate=red and bump the checkout revision"
    server -> orchJobStore "The gate follower wakes on the store change and reads gate state red"
    server -> orchGates "Apply a typed failed reading with covering evidence; pending becomes failed"
    server -> orchRuns "Append a deduplicated gate run event"
    server -> orchPlanning "Re-evaluate readiness; dependent steps become blocked with gate_failed"
    server -> orchSpeculation "Provisional work stacked on the gate goes at risk"
    server -> orchScheduler "The event wakes a pass; the red-gate stack score ranks the repair first"
    autoLayout lr
}

dynamic orchLib "HS2-Orch-WorkerPlanDispatch" "A dispatcher replaces the worker plan and dispatches one row to a worker, who accepts and claims the ticket itself." {
    server -> checkoutData "PUT worker-plan: replace the plan under state.lock after authority, generation and revision checks; a Replaced event is recorded"
    server -> checkoutData "POST worker-plan/dispatch: re-check the row under the ticket transaction and state.lock; any failed check refuses and leaves the row"
    server -> orchSpeculation "With speculation enabled, accept unfinished dependencies only when provisional readiness allows"
    server -> checkoutData "Create the Offered assignment, record the Assigned event and the speculation record in the same lock session"
    cli -> checkoutData "The worker's coordination next wait wakes on state.json and accepts: Offered becomes Accepted"
    cli -> coreLib "The worker claims the exact ticket; dispatch never claims it"
    cli -> tickets "The claim is written to the ticket frontmatter"
    autoLayout lr
}

dynamic orchLib "HS2-Orch-SpeculationRed" "A red base CI gate invalidates speculative coordinated work." {
    cli -> orchJobStore "speculation-sync reads the CI watch Job for each watched gate commit; one is red"
    cli -> orchSpeculation "Apply the red result: dependents become at risk with a risk notice and recovery"
    cli -> checkoutData "Persist speculation.json; new provisional reservations are refused while repair_first is non-empty"
    cli -> tickets "Write idempotent notes on the dependent and a repair-first note on the prerequisite"
    autoLayout lr
}
