// context relationships. Owned by HS2-M81CCP (context and person edges). Both endpoints belong to this area,
// or one endpoint is an external system or person. Edges between areas go in
// relationships-cross.dsl. Included after every element, so order does not matter.

contributorUsesHs2 = developer -> hs2 "Manages project tickets and runs work" "Browser / terminal"
operatorManagesHs2 = operator -> hs2 "Configures projects, access, and execution" "Browser / terminal"
hs2UsesTools = hs2 -> aiTool "Launches and drives supported sessions" "Process launch / tool-specific protocol"
toolsUseHs2 = aiTool -> hs2 "Uses ticket, claim, and coordination tools" "MCP / local hooks"
hs2UsesGit = hs2 -> gitRemote "Synchronizes ticket repositories and inspects code revisions" "Git"
hs2UsesGithub = hs2 -> github "Reads or updates Issues and observes supported Actions jobs" "HTTPS"
githubNotifiesHs2 = github -> hs2 "Delivers configured repository events" "Webhook"
hs2UsesGitlab = hs2 -> gitlab "Reads or updates Issues and observes supported CI jobs" "HTTPS"
hs2UsesJira = hs2 -> jira "Reads and updates issues" "HTTPS"
hs2UsesCi = hs2 -> ciServices "Observes supported CI jobs" "HTTPS"
hs2SendsNotifications = hs2 -> notifierServices "Delivers configured notices" "Configured notifier protocols"
hs2UsesSharedClaims = hs2 -> sharedClaims "Acquires or observes configured shared resource leases" "HTTPS / subprocess adapter"
hs2UsesOs = hs2 -> osServices "Uses local processes, file events, terminals, and native secret storage" "Native OS APIs"
hs2ReadsLegacy = hs2 -> legacy "Imports a legacy project from a copied database" "Filesystem / PGLite"
contributorUsesWeb = developer -> web "Uses the workspace" "Web UI"
contributorUsesCli = developer -> cli "Runs headless commands" "Terminal"
contributorUsesDesktop = developer -> desktop "Opens the installed application" "Native window"
operatorUsesWeb = operator -> web "Manages projects, devices, and policy" "Web UI"
