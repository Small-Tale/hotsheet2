// People and external systems. Shared; HS2-M81CCP owns changes. Area tickets
// may append a new external system at the end with a comment naming the ticket.

developer = person "Project contributor" "Uses tickets, workspaces, terminals, and headless commands."
operator = person "Host owner or operator" "Configures ticket sources, authorizes remote access, and sets orchestration and execution policy."

aiTool = softwareSystem "AI coding tools" "Installed Claude, Codex, Antigravity, and OpenCode surfaces. Actual launch, drive, permission, and telemetry capabilities vary by tool and adapter." "External"
gitRemote = softwareSystem "Git repository remotes" "User-selected remotes for ticket-store and source-code repositories." "External"
github = softwareSystem "GitHub" "Authoritative GitHub Issues, optional webhook delivery, and supported Actions job observations." "External"
gitlab = softwareSystem "GitLab" "Authoritative GitLab Issues and supported CI job observations." "External"
jira = softwareSystem "Jira" "Authoritative Jira issues and provider-native writes." "External"
ciServices = softwareSystem "Buildkite and CircleCI" "Optional additional CI job sources observed by the Jobs monitor." "External"
notifierServices = softwareSystem "Notification destinations" "Configured local or external notifier adapters, including webhook, messaging, push, email, and SMS." "External"
sharedClaims = softwareSystem "Shared resource claim backends" "Optional Upstash Redis or an approved external backend for resource leases. Git-ref leases use the selected repository remote." "External"
osServices = softwareSystem "Host OS services" "Native filesystem notifications, process and terminal facilities, and OS credential storage." "External"
legacy = softwareSystem "Hot Sheet 1 project" "Legacy PGLite project data used as an import source." "External"
