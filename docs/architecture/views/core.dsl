// Core ticketing views.
// Owned by HS2-AH9HAK. New views use keys prefixed HS2-Core-.

component coreLib "HS2-Core-TicketEngine" "Ticket model, Git file store, operations, sync and merge, and the derived index." {
    include coreModel coreStore coreOps coreWire coreSync coreMerge coreIndex coreUtil tickets searchIndex gitRemote server cli
    autoLayout lr
}
component coreLib "HS2-Core-Providers" "Provider contract, Git and external adapters, transfer, outbox, connection configuration and credentials." {
    include coreProviderContract coreGitProvider coreProviderConfig coreOutbox coreGithub coreGitlab coreJira coreExtShared coreSecrets coreOps github gitlab jira osServices server
    autoLayout lr
}
component coreLib "HS2-Core-MachineAndProject" "Checkout registry, settings, groups, activity and metrics, maintenance, and machine-local utility crates." {
    include coreCheckouts coreSettings coreGroups coreRemoteServers coreActivityMetrics coreRepoMaintenance coreMedia coreTls coreProcessJournal coreStore coreProviderConfig tickets checkoutData machineData osServices gitRemote
    autoLayout lr
}

dynamic coreLib "HS2-Core-GitSyncConflict" "One sync cycle whose incoming change conflicts in a ticket body. Nothing is lost: local commits stay unpublished and the working tree stays clean." {
    server -> coreSync "Runs sync_once for a remote-backed store"
    coreSync -> tickets "Takes the history-sync then Git transaction lock; checks held Git locks; commits pending writes"
    coreSync -> coreMerge "Registers the hotsheet-ticket driver and the -text line-ending pin"
    coreSync -> gitRemote "git fetch; failures are Offline, a lost tracking-ref race is retried"
    coreSync -> tickets "Replays local commits onto the upstream with merge-tree; Git runs the driver per ticket"
    cli -> coreMerge "merge-driver merges fields; both bodies changed, so git merge-file leaves markers and exits 1"
    coreSync -> tickets "Conflict: the isolated replay is discarded (or the live rebase aborted); conflict heads recorded in sync health"
    coreSync -> coreOps "Ask for help: one deduplicated Up Next repair ticket in the stalled store"
    autoLayout lr
}
dynamic coreLib "HS2-Core-IndexRebuild" "Index open and reconcile after drift: schema bump, corrupt database, moved Git owner or edits made outside Hot Sheet." {
    server -> coreIndex "open_reconciled at store open, or reconcile after a watcher event"
    coreIndex -> searchIndex "Takes the -open.lock sidecar; a corrupt file is deleted and recreated; a schema_version other than 26 drops and recreates all tables"
    coreIndex -> coreStore "Reads the Git baseline: owner key, HEAD and dirty ticket ids"
    coreStore -> tickets "git rev-parse, diff and status probes"
    coreIndex -> searchIndex "Compares stored head and git_owner; a mismatch or missing baseline selects the full walk"
    coreIndex -> tickets "Reads and hashes the candidate ticket files"
    coreIndex -> coreModel "Parses only files whose hash changed; an unparseable file keeps its prior row"
    coreIndex -> searchIndex "Upserts rows and FTS, deletes rows whose file is gone, records head and dirty ids, commits the IMMEDIATE transaction"
    autoLayout lr
}
dynamic coreLib "HS2-Core-OutboxRetry" "Opt-in Jira write-behind: durable admission, dispatch, rate-limit deferral and attention states." {
    server -> coreOutbox "admit_batch in one IMMEDIATE transaction: same id and payload returns the row, a changed retry or a full queue fails"
    server -> coreOutbox "claim_ready: first queued intent per ticket becomes sending, attempts plus one"
    server -> coreProviderContract "Resolves the Jira connection"
    coreProviderContract -> coreJira "get, then update with the current concurrency token"
    coreJira -> jira "GET issue; PUT fields; 429 maps to RateLimited with Retry-After"
    server -> coreOutbox "RateLimited: defer the row and the whole connection until Retry-After (default 30 s)"
    server -> coreOutbox "Auth, disabled, not found, unsupported, or every eighth failure: needs_attention; other errors back off exponentially"
    server -> coreOutbox "Acknowledged intent: confirm; settled payloads compact after 30 days"
    autoLayout lr
}
dynamic coreLib "HS2-Core-CrossProviderTransfer" "A user-initiated move from GitHub Issues to a Git store with an operation id and source provenance." {
    server -> coreProviderContract "move_between(source ref, destination connection, operation id)"
    coreProviderContract -> coreGithub "Reads the source issue"
    coreGithub -> github "GET issue, comments and asset-backed attachments"
    coreProviderContract -> coreGitProvider "Checks capabilities; crop, dependencies or unsupported fields fail before anything is created"
    coreProviderContract -> coreGitProvider "create with TransferProvenance and a ULID derived from the operation id; an existing transfer is returned"
    coreGitProvider -> coreOps "create_by, then records transfer_operation_id and transferred_from"
    coreOps -> coreStore "Committing writes inside the ticket transaction"
    coreStore -> tickets "Atomic file write and path-scoped commit"
    coreProviderContract -> coreGitProvider "Copies notes (requests before replies), attachments and assignees with derived ids"
    coreProviderContract -> coreGithub "Closes the source as obsolete; failure returns SourceCloseFailed naming the destination"
    autoLayout lr
}
