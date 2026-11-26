// orchestration relationships. Owned by HS2-RJB111. Both endpoints belong to this area,
// or one endpoint is an external system or person. Edges between areas go in
// relationships-cross.dsl. Included after every element, so order does not matter.

// Intra-area edges.
orchPlanningUsesDecl = orchPlanning -> orchDeclarations "Resolves sites and canonical ticket keys; stores through bounded storage" "Rust"
orchPlanningReadsGates = orchPlanning -> orchGates "Reads CI, deployment and review rollups and validation evidence for prerequisites" "Rust"
orchPlanningReadsRuns = orchPlanning -> orchRuns "Reads active runs on step tickets" "Rust"
orchPolicyChecksDecl = orchPolicy -> orchDeclarations "Validates policy targets against declared targets" "Rust"
orchSchedulerReadsPlan = orchScheduler -> orchPlanning "Reads ready and provisional work" "Rust"
orchSchedulerAppliesPolicy = orchScheduler -> orchPolicy "Applies session, tool and speculation limits" "Rust"
orchSchedulerReadsCapacity = orchScheduler -> orchAdmission "Reads capacity snapshots, tool capacity and resource standings" "Rust"
orchSchedulerDecidesSessions = orchScheduler -> orchSessions "Decides Start, Keep or WindDown for sessions" "Rust"
orchSessionsAdmit = orchSessions -> orchAdmission "Asks the host's admission decision before a launch" "Rust"
orchReconcileAligns = orchReconcile -> orchRuns "Aligns a run with its target ticket facts" "Rust"
orchReconcileAssesses = orchReconcile -> orchGates "Assesses CI, deployment and validation sources" "Rust"
orchProgressFolds = orchProgress -> orchRuns "Folds runs, run events and sessions into timelines and summaries" "Rust"
orchSpeculationReadsGates = orchSpeculation -> orchGates "Maps a CI rollup to a red, green or cancelled gate result" "Rust"
orchPermissionsJournal = orchPermissions -> orchRuns "Shares the bounded journal engine with run events" "Rust"
orchClaimsUseCas = orchTicketClaims -> orchGitRefClaims "Acquires, renews and releases ticket claim markers" "Rust"
orchResourceDispatch = orchResourceClaims -> orchResourceBackends "Dispatches to the configured backend" "Rust trait"
orchResourceRegistry = orchResourceClaims -> orchTicketClaims "Resolves the claim-backend registry and bindings" "Rust"
orchResourceGitRef = orchResourceBackends -> orchGitRefClaims "Uses the Git ref compare-and-swap for resource markers" "Rust"
orchJobsPollAdapters = orchJobStore -> orchCiAdapters "Polls held watch Jobs through their source" "Rust"

// External systems.
orchCasUsesGit = orchGitRefClaims -> gitRemote "Reads markers with ls-remote; writes with force-with-lease pushes" "Git"
orchResourcesUseShared = orchResourceBackends -> sharedClaims "Runs Upstash Lua scripts and SSE subscriptions, or approved plugin subprocesses" "HTTPS / subprocess"
orchAdaptersUseGithub = orchCiAdapters -> github "Reads Actions runs, jobs and artifacts; cancels runs" "HTTPS REST"
orchAdaptersUseGitlab = orchCiAdapters -> gitlab "Reads pipelines and jobs; cancels pipelines" "HTTPS REST"
orchAdaptersUseCi = orchCiAdapters -> ciServices "Reads Buildkite, CircleCI, Jenkins, Azure Pipelines and Bitbucket runs" "HTTPS REST"
