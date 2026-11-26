// Conceptual overview for the guided tour (HS2-DHCY18). These are Structurizr
// custom elements: they appear only in the HS2-00-Overview custom view and
// summarize the evidence-backed C4 model in plain terms. Each element's url
// names the detailed view a double-click opens. Claims here must stay true of
// the detailed model; change that model first, then this summary.

ovPeople = element "You" "People" "Contributors and host owners working with tickets, terminals and AI sessions." "Overview,Overview People" {
    url "{workspace}/diagrams#HS2-Context"
}
ovAi = element "AI coding tools" "Claude, Codex and others" "Read and update tickets, ask for permission, run in Hot Sheet terminals." "Overview,Overview External" {
    url "{workspace}/diagrams#HS2-Flow-AiSession"
}
ovApp = element "Hot Sheet app" "Desktop window or browser" "Where you browse and edit tickets, run terminals and review plans." "Overview,Overview Client" {
    url "{workspace}/diagrams#HS2-WebClient-Core"
}
ovServer = element "Hot Sheet server" "Local, always on" "Keeps tickets live, owns terminal sessions and schedules, and runs coordination." "Overview,Overview Host" {
    url "{workspace}/diagrams#HS2-Server-Admission"
}
ovHeadless = element "CLI and MCP" "Headless entry points" "The same ticket operations with no window, for scripts and AI tools." "Overview,Overview Host" {
    url "{workspace}/diagrams#HS2-Cli-CommandTree"
}
ovCore = element "Ticketing core" "Shared Rust libraries" "One ticket model, store and provider layer that every entry point uses." "Overview,Overview Core" {
    url "{workspace}/diagrams#HS2-Core-TicketEngine"
}
ovOrch = element "Orchestration" "Plans, claims and gates" "Decides who works on what, where, and whether CI let it through." "Overview,Overview Core" {
    url "{workspace}/diagrams#HS2-Orch-Placement"
}
ovGitStore = element "Git ticket store" "Markdown files in Git" "The default ticket source: plain files, synced through a Git remote." "Overview,Overview Store" {
    url "{workspace}/diagrams#HS2-ProviderBoundaries"
}
ovTrackers = element "Issue trackers" "GitHub, GitLab, Jira" "Ticket sources that keep their own records; Hot Sheet reads and writes them natively." "Overview,Overview External" {
    url "{workspace}/diagrams#HS2-ProviderBoundaries"
}
ovPeers = element "Other machines and CI" "Peer Hot Sheet hosts, CI services" "Remote hosts that accept placed work, and CI runs that gate it." "Overview,Overview External" {
    url "{workspace}/diagrams#HS2-RemoteDeployment"
}

ovPeople -> ovApp "Works in"
ovPeople -> ovHeadless "Scripts with"
ovAi -> ovHeadless "Reads and updates tickets through"
ovApp -> ovServer "Gets live tickets and terminals from"
ovServer -> ovAi "Launches in terminals"
ovServer -> ovCore "Uses"
ovServer -> ovOrch "Runs"
ovHeadless -> ovCore "Uses directly"
ovHeadless -> ovServer "Can delegate to"
ovCore -> ovGitStore "Reads and writes"
ovCore -> ovTrackers "Reads and writes"
ovOrch -> ovPeers "Places work on and watches"
