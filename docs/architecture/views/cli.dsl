// CLI and MCP views.
// Owned by HS2-TW2DKM. New views use keys prefixed HS2-Cli-.

component cli "HS2-Cli-CommandTree" "hotsheet-cli command groups: the Clap tree routes every command to exactly one group handler; the post-command hook runs after each." {
    include developer cliEntry cliPostCommand cliHostClient cliStoreCmds cliTicketCmds cliClaimCmds cliProviderCmds cliSetupCmds cliPermissionHook cliCoordination cliOrchestration cliRemotes cliJobs cliResources cliNotify
    autoLayout lr
}
component cli "HS2-Cli-TicketingDependencies" "Ticket, store, claim, provider, setup and hook commands and what they open or call: the store directly, the local index, the host over HTTP, or external providers." {
    include cliStoreCmds cliTicketCmds cliClaimCmds cliProviderCmds cliSetupCmds cliPermissionHook cliPostCommand cliHostClient migImport coreLib tickets searchIndex checkoutData machineData server github gitlab jira gitRemote aiTool
    autoLayout lr
}
component cli "HS2-Cli-OperationsDependencies" "Coordination, orchestration, remote, job, resource and notify commands; the library components the server and MCP also link." {
    include cliCoordination cliOrchestration cliRemotes cliJobs cliResources cliNotify orchLib tickets checkoutData machineData server mcpCoreBackend github gitlab ciServices sharedClaims notifierServices gitRemote osServices
    autoLayout lr
}
component mcp "HS2-Cli-McpComponents" "MCP shim: stdio loop and backend selection, tool catalog, serverless and HTTP backends, presence; plus the CLI library components it links." {
    include *
    autoLayout lr
}
component migrator "HS2-Cli-MigratorComponents" "hotsheet-migrate, the Node exporter and the shared HS1 importer." {
    include *
    autoLayout lr
}

dynamic mcp "HS2-Cli-McpServerlessWrite" "An MCP hotsheet_update in serverless mode (--path): in-process write, transition notifications, then a host sync kick or a local-only notice." {
    aiTool -> mcpEntry "tools/call hotsheet_update on one stdin line"
    mcpEntry -> mcpTools "handle_message_for_checkout adds the actor from arguments or HOTSHEET_ACTOR_ROLE/ID"
    mcpTools -> mcpCoreBackend "PATCH /tickets/{id} on the Backend trait"
    mcpCoreBackend -> coreLib "ops update; an AI completion without confidence fails as error 422 confidence_required"
    mcpCoreBackend -> tickets "Commit the ticket file under the store lock"
    mcpCoreBackend -> server "POST /sync/kick with X-Hotsheet-Secret; with no accepting host the reply gains a commits-may-be-local-only notice"
}
dynamic mcp "HS2-Cli-McpServerWrite" "The same write in project mode (--project): project open, HTTP write, and the reattach-and-retry failure path when the server route stops answering." {
    aiTool -> mcpEntry "tools/call hotsheet_update"
    mcpEntry -> machineData "Resolve the project in checkouts.json and read the launched or machine host instance record"
    mcpEntry -> server "POST /projects/open returns the checkout id (redirects refused)"
    mcpEntry -> mcpTools "Dispatch with the checkout id injected"
    mcpTools -> mcpHttpBackend "PATCH /checkouts/{id}/tickets/{ticket}"
    mcpHttpBackend -> server "HTTP with X-Hotsheet-Secret; a 3xx is an error, 401/403 probe /health"
    mcpEntry -> machineData "Failure path: on a transport error with no status, re-read instance records"
    mcpEntry -> server "Failure path: re-open the project on the reattached host and retry once"
}
dynamic cli "HS2-Cli-Bootstrap" "hotsheet-cli bootstrap on a fresh checkout: store, Git owner, link, checkout registry, setup, remote; idempotent on re-run." {
    developer -> cliEntry "hotsheet-cli bootstrap --project <dir> [--store] [--remote]"
    cliEntry -> cliStoreCmds "Store group; -C is not resolved for bootstrap"
    cliStoreCmds -> coreLib "init_store or ensure_store_repository settles the store Git owner"
    cliStoreCmds -> tickets "Register the semantic merge driver and make the initial commit"
    cliStoreCmds -> checkoutData "Write the gitignored .hotsheet2/store link"
    cliStoreCmds -> machineData "Register the checkout and its Git source in checkouts.json"
    cliStoreCmds -> cliSetupCmds "run_setup for every detected, enabled tool; none detected is reported, not fatal"
    cliSetupCmds -> checkoutData "Write instructions, skills, hooks and MCP config"
    cliStoreCmds -> gitRemote "Add origin when --remote is given; otherwise print the exact remote and push commands"
}
dynamic cli "HS2-Cli-PermissionHook" "A launched AI session's permission-hook round trip, with its route-failure fallback." {
    aiTool -> cliPermissionHook "Runs hotsheet-cli permission-hook --agent <tool> with hook JSON on stdin"
    cliPermissionHook -> cliHostClient "hook_server_route: live instance for HOTSHEET_PROJECT, else HOTSHEET_SERVER/SECRET"
    cliHostClient -> machineData "Read instances/<hash>.json and check the pid is alive"
    cliPermissionHook -> server "POST /permissions/ask blocks until a person or rule decides"
    cliPermissionHook -> aiTool "Print allow or deny JSON on stdout"
    cliPermissionHook -> aiTool "Failure path: route error prints nothing so the tool shows its own prompt; headless Claude auto prints deny"
}
dynamic migrator "HS2-Cli-Migrate" "hotsheet-migrate: export from a copy, resumable import with checkpoints, verification, receipt and optional backup push." {
    developer -> migBinary "hotsheet-migrate <.hotsheet dir> -C <store> [--progress-json] [--backup-remote URL]"
    migBinary -> migImport "Invalidate earlier completion proof"
    migBinary -> migExporter "node export.mjs --out <tmp>/hotsheet-export.json --progress-json"
    migExporter -> legacy "Copy the datadir to a temp directory and open the copy"
    migBinary -> migImport "Import with progress"
    migImport -> tickets "Per ticket: write the pending checkpoint, the ticket and verified attachments, then remove the checkpoint"
    migImport -> coreLib "Write tickets through the core writer; a repeat import maps the same source to the same ULID"
    migImport -> tickets "Failure path: a failed copy keeps the checkpoint; a re-run resumes it or refuses a changed export"
    migImport -> tickets "Verify the export, write hotsheet-hs1-import.json and commit"
    migImport -> gitRemote "With --backup-remote: save origin, push and record the verified backup"
}
