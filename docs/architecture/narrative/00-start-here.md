## Start here

This workspace describes Hot Sheet 2 in depth, so don't read it all at once. Go
one layer at a time and only go deeper where you need to:

0. **Overview.** Ten boxes that show how the main pieces connect.
1. **System.** People, outside services, the running processes and where they
   are deployed.
2. **Areas.** Inside one part: the web client, web host, server, CLI, core
   libraries or orchestration.
3. **Flows.** Step-by-step sequences that answer "what happens when…?"

The diagram list follows this order, and every title starts with its layer
number.

### Moving around

- **Double-click a box** to open its detailed views. On the overview this jumps
  straight to the right area. On other diagrams it lists the box's component
  views, the flows it takes part in, and a link to its source code.
- **Option/Alt-double-click** a model box to open its source on GitHub at the
  documented revision.
- **Step through the overview** with the animation controls at the bottom right
  of the diagram.
- **Switch light or dark** with the sun and moon icons above the diagram list.
- **Documentation** (this tab) has one chapter per area, after this page.

### Layer 0: the overview

![](embed:HS2-00-Overview)

- **Hot Sheet app** is the desktop window or a browser tab. It reaches the
  server through a small Node bridge.
- **Hot Sheet server** stays running. It keeps tickets live in every open
  window, owns terminal sessions, runs schedules and coordinates work.
- **CLI and MCP** do the same ticket work with no window. Scripts and AI tools
  use them, with or without a running server.
- **Ticketing core** is one set of Rust libraries behind every entry point. It
  reads and writes the **Git ticket store** (Markdown files in Git) or an
  **issue tracker** that keeps its own records.
- **Orchestration** decides which work goes to which machine, and whether CI let
  it through.

### Layer 1: the system

| Diagram                                                                     | What it answers                                                |
| --------------------------------------------------------------------------- | -------------------------------------------------------------- |
| [Context]({workspace}/diagrams#HS2-Context)                                 | Who uses Hot Sheet and which outside services it depends on    |
| [Runtime containers]({workspace}/diagrams#HS2-WorkspaceRuntime)             | Every process and data store, and which talks to which         |
| [Ticket sources and providers]({workspace}/diagrams#HS2-ProviderBoundaries) | Where ticket records live and who owns them                    |
| [Migration and standby]({workspace}/diagrams#HS2-MigrationAndStandby)       | The optional Hot Sheet 1 importer and the macOS standby screen |
| [Desktop deployment]({workspace}/diagrams#HS2-DesktopDeployment)            | What runs where on one desktop machine                         |
| [Headless deployment]({workspace}/diagrams#HS2-HeadlessDeployment)          | The CLI and MCP with no server                                 |
| [Remote deployment]({workspace}/diagrams#HS2-RemoteDeployment)              | A paired browser, and a peer server that accepts placed work   |

### Layer 2: areas

Open the first diagram for an area, then follow the others as needed. Each area
has a documentation chapter with the evidence behind its diagrams.

| Area                   | Start with                                                  | Then                                                                                                                                                                     |
| ---------------------- | ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 2 Web client           | [Core]({workspace}/diagrams#HS2-WebClient-Core)             | [Live data]({workspace}/diagrams#HS2-WebClient-LiveData), [Editing]({workspace}/diagrams#HS2-WebClient-Editing), [Features]({workspace}/diagrams#HS2-WebClient-Features) |
| 3 Web host and desktop | [Bridge]({workspace}/diagrams#HS2-WebHost-BridgeComponents) | [Desktop shell]({workspace}/diagrams#HS2-WebHost-DesktopComponents)                                                                                                      |
| 4 Server               | [Admission]({workspace}/diagrams#HS2-Server-Admission)      | [Sources]({workspace}/diagrams#HS2-Server-Sources), [Execution]({workspace}/diagrams#HS2-Server-Execution), [Broker]({workspace}/diagrams#HS2-Server-Broker)             |
| 5 CLI and MCP          | [Command tree]({workspace}/diagrams#HS2-Cli-CommandTree)    | [MCP]({workspace}/diagrams#HS2-Cli-McpComponents), [Migrator]({workspace}/diagrams#HS2-Cli-MigratorComponents)                                                           |
| 6 Ticketing core       | [Ticket engine]({workspace}/diagrams#HS2-Core-TicketEngine) | [Providers]({workspace}/diagrams#HS2-Core-Providers), [Machine and project]({workspace}/diagrams#HS2-Core-MachineAndProject)                                             |
| 7 Orchestration        | [Placement]({workspace}/diagrams#HS2-Orch-Placement)        | [Runs and gates]({workspace}/diagrams#HS2-Orch-RunsAndGates), [Claims]({workspace}/diagrams#HS2-Orch-Claims), [Jobs]({workspace}/diagrams#HS2-Orch-Jobs)                 |

### Layer 3: flows

Start from a question:

| What happens when…                               | Flow                                                                       |
| ------------------------------------------------ | -------------------------------------------------------------------------- |
| I edit a ticket in the app?                      | [Ticket edit]({workspace}/diagrams#HS2-Flow-TicketEdit)                    |
| A `git pull` or the CLI changes a ticket file?   | [Out-of-band edit]({workspace}/diagrams#HS2-Flow-OutOfBandEdit)            |
| I edit a GitHub, GitLab or Jira ticket?          | [Native provider write]({workspace}/diagrams#HS2-Flow-NativeProviderWrite) |
| I start an AI session from the app?              | [AI session]({workspace}/diagrams#HS2-Flow-AiSession)                      |
| An AI tool asks for permission?                  | [AI permission]({workspace}/diagrams#HS2-Flow-AiPermission)                |
| Two workers try to claim the same ticket?        | [Claim contention]({workspace}/diagrams#HS2-Flow-ClaimContention)          |
| A ticket is placed on a machine and gated by CI? | [Placement to gate]({workspace}/diagrams#HS2-Flow-PlacementToGate)         |
| I open the desktop app?                          | [Desktop cold start]({workspace}/diagrams#HS2-Flow-DesktopColdStart)       |
| A phone or another computer connects?            | [Remote device]({workspace}/diagrams#HS2-Flow-RemoteDevice)                |
| I set Hot Sheet up on a new checkout?            | [Fresh install]({workspace}/diagrams#HS2-Flow-FreshInstallGui)             |
| I import a Hot Sheet 1 project?                  | [Migration]({workspace}/diagrams#HS2-Flow-Migration)                       |

The **End-to-end flows** chapter covers every flow, including failure and
recovery paths.

### What it looks like

Real screens from the app, captured from synthetic demo data. Each one is also
attached to its box in the diagrams: double-click the box to find it.

#### Workspace with an open project

![](embed:HS2-Screen-WorkspaceTicketList)

The Acme Notes project open in the workspace: project tab, views sidebar, and the queue of tickets read from the project's checkout through the bridge and server.

Related: [HS2-WebClient-OpenProject]({workspace}/diagrams#HS2-WebClient-OpenProject), [HS2-WorkspaceRuntime]({workspace}/diagrams#HS2-WorkspaceRuntime)

#### Ticket editor with notes

![](embed:HS2-Screen-TicketEditorNotes)

One ticket selected: the inspector edits its fields and details and lists its notes, including an attributed AI planning note.

Related: [HS2-Flow-TicketEdit]({workspace}/diagrams#HS2-Flow-TicketEdit), [HS2-WebClient-CoalescedEdit]({workspace}/diagrams#HS2-WebClient-CoalescedEdit)

#### AI session terminal

![](embed:HS2-Screen-AiSessionTerminal)

The project terminal drawer attached to a running AI session's PTY over the terminal WebSocket, with replay and live output.

Related: [HS2-Flow-AiSession]({workspace}/diagrams#HS2-Flow-AiSession), [HS2-WebClient-TerminalAttach]({workspace}/diagrams#HS2-WebClient-TerminalAttach)

#### AI question prompt

![](embed:HS2-Screen-AiQuestionPrompt)

A question the AI session asked arrives in the app as a prompt the user can answer here or in the terminal.

Related: [HS2-Flow-AiPermission]({workspace}/diagrams#HS2-Flow-AiPermission)

#### Orchestration plan and run

![](embed:HS2-Screen-OrchestrationPlanRun)

The Orchestration view: a goal's planned steps by column, the running step dispatched to a worker, and the inspector's Where It Runs, Plan, and Run sections.

Related: [HS2-Flow-PlacementToGate]({workspace}/diagrams#HS2-Flow-PlacementToGate), [HS2-Orch-Placement]({workspace}/diagrams#HS2-Orch-Placement), [HS2-Orch-RunsAndGates]({workspace}/diagrams#HS2-Orch-RunsAndGates)

#### Remote device pairing request

![](embed:HS2-Screen-RemoteDevicePairing)

A browser asks to pair with this host: a notice announces it, Settings carries the count, and Devices lists the waiting request to approve or reject.

Related: [HS2-Flow-RemoteDevice]({workspace}/diagrams#HS2-Flow-RemoteDevice), [HS2-Server-DevicePairing]({workspace}/diagrams#HS2-Server-DevicePairing)

#### Project ticket sources

![](embed:HS2-Screen-SettingsTicketSources)

Project settings list the ticket sources this checkout uses; GitHub, GitLab, and Jira sign-ins are shared app-wide under Accounts.

Related: [HS2-ProviderBoundaries]({workspace}/diagrams#HS2-ProviderBoundaries), [HS2-Core-Providers]({workspace}/diagrams#HS2-Core-Providers)

#### Adding a ticket source

![](embed:HS2-Screen-SettingsAddSource)

Adding a source offers a Hot Sheet Git ticket repository or a native GitHub Issues, GitLab Issues, or Jira Cloud connection, each keeping its own authoritative records.

Related: [HS2-ProviderBoundaries]({workspace}/diagrams#HS2-ProviderBoundaries), [HS2-Core-Providers]({workspace}/diagrams#HS2-Core-Providers)

#### Standby display

![](embed:HS2-Screen-StandbyDisplay)

Standby shows every open project's live activity at a glance: token rate, tickets updated, and a stalled-work alert.

Related: [HS2-MigrationAndStandby]({workspace}/diagrams#HS2-MigrationAndStandby), [HS2-Server-Saver]({workspace}/diagrams#HS2-Server-Saver)
