// core relationships. Owned by HS2-AH9HAK. Both endpoints belong to this area,
// or one endpoint is an external system or person. Edges between areas go in
// relationships-cross.dsl. Included after every element, so order does not matter.

// ---- model, store and operations ---------------------------------------------------
coreOpsUsesStore = coreOps -> coreStore "Reads and writes tickets inside the note-mutation transaction; committing writes" "Rust call"
coreOpsUsesModel = coreOps -> coreModel "Builds tickets, derives slugs, normalizes groups and notes" "Rust call"
coreOpsWritesWorklist = coreOps -> checkoutData "Regenerates the gitignored .hotsheet2/worklist.md" "Filesystem"
coreStoreUsesModel = coreStore -> coreModel "Parses and serializes ticket files" "Rust call"
coreStoreWritesGit = coreStore -> tickets "Atomically replaces ticket files, writes attachments, commits touched paths, launches a headless push" "Filesystem / git subprocess"
coreStoreUsesUtil = coreStore -> coreUtil "Atomic replace and canonical path spelling" "Rust call"
coreStoreUsesMedia = coreStore -> coreMedia "Validates crop dimensions and renditions for attachment markup" "Rust call"
coreWireUsesModel = coreWire -> coreModel "Maps Ticket to ApiTicket and TicketRow" "Rust call"

// ---- sync and merge ----------------------------------------------------------------
coreSyncUsesGit = coreSync -> tickets "Takes coordination locks, commits pending writes, replays or rebases, moves the branch, records sync health" "git subprocess"
coreSyncUsesRemote = coreSync -> gitRemote "Fetches and pushes the store branch" "Git"
coreSyncUsesMerge = coreSync -> coreMerge "Registers the merge driver and line-ending pin before integration" "Rust call"
coreMergeUsesModel = coreMerge -> coreModel "Merges three parsed tickets field by field" "Rust call"
coreMergeRegisters = coreMerge -> tickets "Writes the driver attribute and command into the private Git directory and config" "git config / info/attributes"
coreSyncAsksOps = coreSync -> coreOps "Creates one deduplicated Up Next repair ticket for a stall" "Rust call"
coreSyncReadsSettings = coreSync -> coreSettings "Reads ticket_store_sync.ask_ai" "Rust call"

// ---- index -------------------------------------------------------------------------
coreIndexUsesDb = coreIndex -> searchIndex "Opens, rebuilds, reconciles and queries" "SQLite"
coreIndexReadsFiles = coreIndex -> tickets "Reads and hashes ticket files" "Filesystem"
coreIndexUsesStore = coreIndex -> coreStore "Reads the Git baseline, ticket paths and changed ticket ids" "Rust call"
coreIndexUsesModel = coreIndex -> coreModel "Parses changed ticket bytes" "Rust call"
coreIndexUsesWire = coreIndex -> coreWire "Builds TicketRow pages" "Rust call"

// ---- providers ---------------------------------------------------------------------
coreContractRoutesGit = coreProviderContract -> coreGitProvider "Routes by connection id; creates transfer destinations" "TicketProvider"
coreContractRoutesGithub = coreProviderContract -> coreGithub "Routes by connection id; reads and closes transfer sources" "TicketProvider"
coreContractRoutesGitlab = coreProviderContract -> coreGitlab "Routes by connection id" "TicketProvider"
coreContractRoutesJira = coreProviderContract -> coreJira "Routes by connection id; close is unsupported, so a move from Jira fails after the copy" "TicketProvider"
coreGitProviderUsesOps = coreGitProvider -> coreOps "Delegates ticket operations by ULID" "Rust call"
coreGitProviderUsesWire = coreGitProvider -> coreWire "Returns ApiTicket with the updated_at concurrency token" "Rust call"
coreGithubCallsApi = coreGithub -> github "Reads and writes issues, labels, comments and asset files" "HTTPS / ureq"
coreGitlabCallsApi = coreGitlab -> gitlab "Reads and writes issues, notes, labels and uploads" "HTTPS / ureq"
coreJiraCallsApi = coreJira -> jira "Reads and writes issues, comments, labels, attachments and properties" "HTTPS / ureq"
coreGithubUsesSecrets = coreGithub -> coreSecrets "Stores and refreshes the device-flow token bundle" "Rust call"
coreOutboxUsesWire = coreOutbox -> coreWire "Stores base ApiTicket snapshots and projects pending edits" "Rust call"
coreConnectUsesConfig = coreExtShared -> coreProviderConfig "Persists the validated connection record" "Rust call"
coreConnectUsesCheckouts = coreExtShared -> coreCheckouts "Links the new source to the checkout" "Rust call"
coreConnectUsesSecrets = coreExtShared -> coreSecrets "Resolves the credential reference" "Rust call"
coreConnectBuildsAdapters = coreExtShared -> coreProviderContract "Builds and registers live adapters" "Rust call"
coreConfigUsesUtil = coreProviderConfig -> coreUtil "Publishes providers.json by temp file and rename" "Rust call"

// ---- machine and project state -----------------------------------------------------
coreCheckoutsUseConfig = coreCheckouts -> coreProviderConfig "Edits connection records while holding checkouts.json.lock, then providers.json.lock" "Rust call"
coreCheckoutsOpenStores = coreCheckouts -> coreStore "Admits and opens linked Git stores" "Rust call"
coreCheckoutsWriteProject = coreCheckouts -> checkoutData "Maintains the managed Hot Sheet block in the project .gitignore" "Filesystem"
coreSettingsUseProject = coreSettings -> checkoutData "Reads and writes shared and local settings files" "Filesystem"
coreGroupsWriteRegistry = coreGroups -> checkoutData "Writes and commits .hotsheet2/groups/<id>.json" "Filesystem / git subprocess"
coreGroupsRewriteTickets = coreGroups -> coreStore "Rewrites member tickets as one committed change" "Rust call"
coreSecretsUseOs = coreSecrets -> osServices "Stores the machine master key in the native credential store" "Native OS APIs"
coreMetricsWriteStore = coreActivityMetrics -> tickets "Appends machine-local JSONL and writes per-contributor rollups" "Filesystem"
coreCompactionRewrites = coreRepoMaintenance -> tickets "Rewrites attachment history with git-filter-repo and records a recovery marker" "git subprocess"
coreCompactionPushes = coreRepoMaintenance -> gitRemote "Replaces the remote branch with force-with-lease" "Git"
coreJournalSpawns = coreProcessJournal -> osServices "Spawns, detaches and observes child processes" "Native OS APIs"
