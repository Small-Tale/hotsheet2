# Hot Sheet platform design decisions

Kerf UI owns shared interaction conventions and design philosophy. Read the
installed `@kerfjs/ui/docs/interaction-conventions.md` and its linked
`design-philosophy.md` before building a web or Tauri surface. Use Apple's
[Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines/)
for native macOS and iOS behavior. This document keeps Hot Sheet-specific
choices, plans, and ticket evidence. [Hot Sheet UI conventions](ui-design-philosophy.md)
are the short entry point; [ux-components.md](ux-components.md) defines product
component contracts.

## 2. Product composition contracts

These choices came from the Hot Sheet HIG review. Requirements in
[06-clients.md](06-clients.md), component contracts in
[ux-components.md](ux-components.md), and current code take precedence over the
dated implementation snapshot in §7.

- **Shell and chrome.** The window is sidebar · list/board workspace · inspector,
  with a bottom terminal drawer and a 1024×600 floor. The inspector collapses
  before the sidebar; sidebar regions stay at least 250 px. The drawer's toggle
  and actions stay in the header and View menu. The workspace toolbar contains
  the project title, view choice, sort/group, bulk actions, search, inspector
  toggle, New Ticket, and More menu. Its controls must fit without wrapping at
  the floor (HS2-A2A9GT).
- **Ticket state and focus.** Needs review, blocked, Up Next, and a live claim
  pair color with text. A ticket row's accessible name includes title, status,
  priority, and applicable state. Activity announcements are polite and never
  narrate each PTY line. Each sidebar, workspace, inspector, and terminal region
  has one scroll owner; terminal output follows the end only while pinned to it.
- **Color and motion.** Kerf public tokens own generic chrome. Hot Sheet aliases
  name only domain states such as ticket status. The selected row may differ for
  focused and inactive windows; no state relies on color alone. Claim animation
  becomes static under Reduce Motion. The dark and contrast work is tracked by
  HS2-DKZG9S; do not encode a light-only assumption as a design rule.
- **Writing.** Ticket verbs are Start, Complete, Claim, Release, Move to Up Next,
  Mark Verified, and Report Not Working…. Empty, loading, offline, and failed
  states describe different conditions. Autosave status stays beside the editor
  rather than producing a toast per save.
- **Privacy and identity.** Only server instance data carries bearer credentials;
  provider tokens live behind machine credentials. A checkout id is a readable
  path identity, not a secret or ticket-store id. See [AGENTS.md](../AGENTS.md).
- **Ticket movement.** A board-column or sidebar-queue drop changes ticket state;
  cross-store transfer is an explicit copy or move through the idempotent
  provider-neutral operation. Move to…, Copy to…, and Set Status remain available
  without dragging. A ticket blocked by a claim or provider capability cannot
  appear draggable.
- **Search and restoration.** Workspace search covers slug, title, tags, details,
  and notes through the checkout index; it does not silently narrow to the active
  sidebar view. Exact slugs can reveal otherwise excluded lifecycle states.
  Project view, sort, pane sizes, and unfinished drafts restore at their own
  scopes. Background reconciliation keeps the visible result set and active
  editing context stable.
- **Notifications and permissions.** Permission prompts are queued in a non-modal
  popup across projects. A blocked AI tool may warrant a time-sensitive native
  notification; routine ticket changes do not. The Notifications view is the
  foreground receiver and clears its count. Automatic permission timers remain
  explicit per-project choices.
- **Platform integration.** Tauri keeps native window decorations and the
  configured minimum size. Native macOS uses system scenes and controls, with a
  terminal content split. All clients use the shared Lucide metaphor set; app
  attribution is Small Tale Inc. (HS2-0P83KD, [AGENTS.md](../AGENTS.md)).

## 4. Product menu and command plan

### 4.4 The menu bar (macOS and Tauri)

Hot Sheet's planned order is **App · File · Edit · View · Project · Ticket · Window ·
Help**. The command registry drives native and Tauri menus, the in-browser More
menu, and shortcuts (HS2-80VPPW, HS2-KTHGVE). A temporarily unavailable command
remains visible but disabled with its reason; a context-menu command also has a
menu or keyboard route.

Standard contents that apply to Hot Sheet:

| Menu      | Items (in order)                                                                                                                                                                                                                                                                                            |
| --------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Hot Sheet | About Hot Sheet · Settings… ⌘, · Services ▸ · Hide Hot Sheet ⌘H · Hide Others ⌥⌘H · Show All · Quit Hot Sheet ⌘Q                                                                                                                                                                                            |
| File      | New Ticket… ⌘N · New Window ⇧⌘N · Open Store… ⌘O · Open Recent ▸ · Close ⌘W / Close Tab · Close Window ⇧⌘W · Export As… · Print… ⌘P                                                                                                                                                                         |
| Edit      | Undo/Redo (result-named) ⌘Z ⇧⌘Z · Cut ⌘X · Copy ⌘C · Paste ⌘V · Paste and Match Style ⌥⇧⌘V · Delete · Select All ⌘A · Deselect All ⇧⌘A · Find ▸ (Find ⌘F focuses search) · Spelling and Grammar ▸ · Substitutions ▸ · Transformations ▸ · Speech ▸ · (Start Dictation, Emoji & Symbols added by the system) |
| View      | as List / as Board (checkmarked) · Sort By ▸ · Group By ▸ · Show/Hide Tab Bar · Show/Hide Toolbar · Customize Toolbar… · Show/Hide Sidebar ⌃⌘S · Show/Hide Inspector ⌥⌘I · Show/Hide Terminal · Enter Full Screen ⌃⌘F                                                                                       |
| Project   | Switch to open projects · Reload · Ticket Sources… · Commands… · Permissions…                                                                                                                                                                                                                               |
| Ticket    | Open · Open in New Window · Start · Complete · Mark Verified · Report Not Working… · Set Status ▸ · Set Priority ▸ · Set Category ▸ · Move to Up Next / Backlog · Claim / Release · Add Tag… · Duplicate · Copy Slug · Copy Link · Move to Store… · Archive · Delete…                                       |
| Window    | Minimize ⌘M · Zoom · Show Previous/Next Tab · Move Tab to New Window · Merge All Windows · Bring All to Front · open windows                                                                                                                                                                                |
| Help      | Hot Sheet Help · Release Notes · Keyboard Shortcuts                                                                                                                                                                                                                                                         |

App-specific menus mirror the hierarchy (projects contain tickets), so Project
precedes Ticket. Format is omitted (Markdown, not rich text). Menu bar extra: only if
users enable it in Settings, showing active AI work as a **menu**, never a popover;
the same items must exist elsewhere.

**Hot Sheet application.** Native: `CommandMenu`/`CommandGroup`. Tauri: the same
structure through the Tauri menu API driven by one command registry, which also
feeds the in-browser More menu and the Keyboard Shortcuts reference so no command is
context-menu-only (HS2-80VPPW, with HS2-KTHGVE for the shortcut table).

## 5. Product input rules

### 5.1 Keyboards

Hot Sheet preserves standard system shortcuts. On macOS, Control combinations
belong to the terminal and system; the web client maps Command shortcuts to
Control on Linux and Windows. The command registry is authoritative for these
bindings (HS2-80VPPW, HS2-KTHGVE).

**Hot Sheet shortcut register** (macOS keys; the web client maps ⌘→Ctrl on Linux and
Windows and must not intercept Control combinations on macOS, which belong to the
terminal and the system):

| Shortcut                     | Command                                                                    | Notes                                |
| ---------------------------- | -------------------------------------------------------------------------- | ------------------------------------ |
| ⌘N                           | New Ticket…                                                                |                                      |
| ⇧⌘N                          | New Window                                                                 |                                      |
| ⌘O / ⌘W / ⇧⌘W / ⌘Q / ⌘M / ⌘H | Open Store… / Close (Tab) / Close Window / Quit / Minimize / Hide          | system meanings, never rebound       |
| ⌘,                           | Settings…                                                                  |                                      |
| ⌘Z / ⇧⌘Z                     | Undo / Redo (result-named)                                                 | text-level undo inside editors       |
| ⌘X / ⌘C / ⌘V                 | Cut / Copy / Paste ticket(s) when the work area owns focus; text otherwise | existing focus-ownership rule        |
| ⌘A / ⇧⌘A                     | Select All / Deselect All tickets in the focused list                      |                                      |
| ⌘F / ⌥⌘F                     | Focus workspace search                                                     |                                      |
| ⌘J                           | Scroll to selected ticket                                                  |                                      |
| ⌃⌘S / ⌥⌘I                    | Show/Hide Sidebar / Inspector                                              |                                      |
| ⌃⌘F                          | Enter/Exit Full Screen                                                     | system                               |
| ⌘P                           | Print…                                                                     |                                      |
| ⌘?                           | Help                                                                       |                                      |
| ⇧⌘[ / ⇧⌘]                    | Previous / Next project tab                                                | window-tab convention                |
| ⌘1 / ⌘2                      | View as List / as Board                                                    | app-specific, Command-only           |
| Return / ⌘↓                  | Open selected ticket in the reader                                         | `Return` opens; `⌘↓` mirrors Finder  |
| Space                        | Quick Look-style preview of an attachment                                  | when the attachment list has focus   |
| Delete / ⌘Delete             | Archive selected / Delete… selected                                        | Delete-key parity with Edit › Delete |
| Esc / ⌘.                     | Cancel, close popover or dialog, clear search                              |                                      |

The single source for this table is the command registry (HS2-80VPPW, HS2-KTHGVE).

### 5.2 Pointing devices

The Hot Sheet cursor rule uses `pointer` for clickable controls and selectable
rows, `text` for editable text, `not-allowed` for disabled controls and invalid
drops, grab for draggable surfaces, resize cursors for splitters, and the platform
default for non-interactive content. This web choice differs from AppKit's arrow
over buttons. Kerf supplies the native control baseline; `cursor-semantics.css`
adds delegated option rows and resize-drag cursors (HS2-CYSE1W). Under
Option/Alt, a valid drag copy uses `dropEffect=copy`; invalid drops use `none`.

### 5.3 Focus and selection

Live ticket updates may not steal focus. If an AI worker moves or deletes the
selected ticket, clear the selection without jumping to another row. The
terminal drawer shows a visible focus state. Hover-revealed row actions remain
available from the keyboard or context menu.

## 6. Product AI presentation

### 6.1 Generative AI and machine learning

AI notes, distilled activity notes, ticket repair proposals, narration, and
tool-generated fields carry a persistent AI label in their accessible name.
AI-driven mutations offer Undo, Retry, or Edit where supported and confirm before
irreversible steps. Repair is guided and never auto-applies below threshold;
progress names the agent's current step. Settings disclose which tool receives
ticket text, and feedback is stored as a regular note (HS2-WBW3Z9,
HS2-3GRNZW). Permission prompts remain the human decision point
([05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.7).

## 7. Dated client baseline

Historical review of the shipped web client on 2026-09-03. This table is
dated evidence, not the current implementation status; use the named tickets
and current code for today's state.

| Area                  | Already conforms                                                                                                                                               | Gap                                                                                                                              |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| Structure             | Sidebar · workspace · inspector split view with persistent splitters, 1024×600 floor, inspector-first collapse, header-owned restore controls, terminal drawer | Key/inactive window muting (Tauri), background extension under chrome, scroll-edge treatment                                     |
| Menus and commands    | Icon-bearing context menus identical in list and board; outside-click and Esc dismissal                                                                        | No menu bar / command registry; no in-browser command surface; shortcut table not centralized (HS2-80VPPW, HS2-KTHGVE)           |
| Editing               | 150 ms autosave, no Save/Cancel, field-aware live merge, reader escalation                                                                                     | Result-named undo entries; Edit-menu wiring                                                                                      |
| Selection and focus   | Native click/⌘/⇧ selection model; no focus theft on live updates; work-area focus ownership                                                                    | Accent-vs-gray highlight by list focus; roving tabindex audit (HS2-A2A9GT)                                                       |
| Feedback and modality | Non-modal permission popups with queueing; inline connection banner; silent background reconciliation                                                          | Dialog button grammar audit; specific progress text; ellipsis rule (HS2-A2A9GT)                                                  |
| Color and appearance  | Semantic Web Awesome tokens, `--hs-*` aliases limited to domain states, literal-token lint                                                                     | Light-only; no `prefers-color-scheme`, `prefers-contrast`, `prefers-reduced-motion`, `prefers-reduced-transparency` (HS2-DKZG9S) |
| Typography            | Token-based sizes                                                                                                                                              | Inter-first stack instead of system font; scale not mapped to macOS text styles (HS2-PS29TA)                                     |
| Icons                 | Lucide through a shared component with the icon-policy test                                                                                                    | Historical native decision ticket HS2-0P83KD; the current project rule is Lucide across clients                                  |
| Cursors               | Kerf document cursor baseline; `cursor-semantics.css` for option rows and splitter cursors                                                                     | `dropEffect` copy/none semantics                                                                                                 |
| Notifications         | Permission events, history view, "Decision made outside Hot Sheet"                                                                                             | Interruption-level policy and coalescing once native/Tauri notifications ship                                                    |
| AI content            | Attributed activity timeline; Attempt AI repair creates a ticket instead of editing files                                                                      | Persistent AI labels, feedback, undo/retry affordances (HS2-WBW3Z9)                                                              |
| Help                  | Tooltips on icon actions                                                                                                                                       | 60–75 character verb-first audit; contextual tips; Help menu content                                                             |

---

## 8. Client plans

### 8.1 The native SwiftUI macOS app (client step 3)

Build order and the HIG surfaces each step unlocks:

1. **Scenes and windows.** `WindowGroup` per project window with `NavigationSplitView`
   (sidebar · content · detail) plus `.inspector`, native window tabbing for project
   tabs, `.defaultSize` and the 1024×600 minimum, `Settings` scene with pane
   toolbar, state restoration. Auxiliary `Window` scenes for a detached terminal and
   the Notifications history.
2. **Menu bar and commands.** `CommandMenu`/`CommandGroup` implementing §4.4 exactly,
   sharing titles, order, enablement, and shortcuts with the command registry the web
   client already uses. Disable, never hide. Dock menu with New Ticket… and recent
   projects.
3. **Content views.** `Table` for the ticket list (sortable, resizable, alternating
   rows, multi-selection, drag), a `LazyVGrid`/`LazyHStack` board using the same
   ticket-summary contract, `OutlineGroup` sidebar with persisted expansion, a
   `TextEditor`-based details/notes editor with the same autosave and field-aware
   merge semantics, `.searchable` with scopes and tokens.
4. **Presentation.** `.sheet` for New Ticket, provider connection, bulk operations,
   Not Working evidence; `.alert`/`NSAlert` only for irreversible surprises; popovers
   (detachable) for filters and quick pickers; `confirmationDialog` for post-action
   choices.
5. **System integration.** `UNUserNotificationCenter` with interruption levels and
   Approve/Deny actions; App Intents for CLI-parity verbs plus a `Ticket` entity in
   Spotlight; a WidgetKit Up Next widget (small/medium/large, vibrant-safe);
   `ShareLink` for ticket links; Quick Look for attachments; optional
   `MenuBarExtra` (user-enabled, menu not popover) showing active AI work; Services
   and Open Recent for stores.
6. **Terminal.** A native terminal surface in a bottom split (content layer, no
   glass) honoring the server-arbitrated sizing model ([06](06-clients.md) §6.7),
   with a visible focus state and the standard find-in-scrollback.
7. **Polish gates.** Icon Composer app icon; Developer ID signing and notarization;
   VoiceOver, Full Keyboard Access, Increase Contrast, Reduce Transparency, Reduce
   Motion passes; light and dark; graphite accent; the visual QA rule from [AGENTS.md](../AGENTS.md).

The current project rule selects Lucide for the native app too ([AGENTS.md](../AGENTS.md)).

### 8.2 The Tauri host (client step 2)

- Native window decorations and minimum size; never a CSS title bar.
- A native menu built from the command registry (§4.4), with platform-appropriate
  modifiers and the same enablement rules; on Linux/Windows the menu renders
  in-window with Ctrl/Alt conventions.
- Notification plugin for background events; a tray item may show active AI work
  (compact status, updated only on change, cleared when the claim ends).
- Key/inactive window state mirrored into the web UI (`blur`/`focus` events → muted
  selection accent).
- OS-level file drops for attachments; "Reveal in Finder" replaces browser download
  where the platform allows.
- Bundled Web Awesome assets, no external requests, production Vite output only
  (see [06](06-clients.md) §6.3 startup budget).

### 8.3 The browser client (now)

The browser follows Kerf UI components and public tokens, with Hot Sheet-specific
compositions demonstrated in `/ux-demo`. The catalog must show every
supported variant and state (project rule), which is how the HIG's "consistent
presentation across surfaces" is proven for each component.

### 8.4 iOS, Android, and beyond

iOS (client step 4) reuses this document plus the iOS platform considerations in the
digests (HS2-46RA38): tab bar or adaptable sidebar, sheets with detents, edit menus,
44 pt targets, Dynamic Type, Live Activities for active claims, App Shortcuts, Home
Screen quick actions, a widget family, and the remote-first permission-answering role
from [08](08-distributed-and-remote.md). Android (step 5) keeps the behavioral rules
(menus, undo, feedback, modality, focus, AI disclosure) and adopts Material
conventions for presentation. Any future surface — a menu bar extra, a Shortcuts
action, a widget, a Spotlight result — is designed from the relevant HIG page first
and recorded here.

---

## 9. Hot Sheet rules that override or extend the HIG

These are deliberate. Do not "fix" them toward the HIG without a maintainer decision.

| Hot Sheet rule                                                                                                 | Relationship to the HIG                                                                                                                                                         |
| -------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Lucide icons through the shared renderer** across web, Tauri, and SwiftUI ([AGENTS.md](../AGENTS.md))        | The HIG assumes SF Symbols on Apple platforms; Hot Sheet keeps one metaphor set across clients. Standard-action meanings still align where possible.                            |
| **Every actionable context-menu item carries an icon**                                                         | The HIG says icons per group all-or-none and sparingly; Hot Sheet chooses "all", which satisfies uniformity.                                                                    |
| **Autosave at 150 ms with no Save/Cancel buttons** for details, notes, titles, tags, blocked reasons           | Consistent with "avoid explicit save"; Hot Sheet fixes the debounce and forbids routine Save/Cancel. Explicit submission remains for creating objects and completing workflows. |
| **Cursor semantics: `pointer` on clickable controls and selectable rows**                                      | The HIG/AppKit shows an arrow over buttons and rows; the web convention is kept deliberately (§5.2).                                                                            |
| **1024×600 AppShell floor**; sidebars ≥ 250 px; no auto-hiding at breakpoints                                  | A Hot Sheet-specific application of the HIG's minimum-size and "defer compact layouts" guidance.                                                                                |
| **No fixed-interval polling; WebSocket or long polling only**                                                  | Not a HIG topic; it underpins the "silent, automatic sync" and "never steal focus" behaviors.                                                                                   |
| **Feedback needed = Needs review** with a purple rail that outranks blocked and Up Next rails                  | Hot Sheet's own state model; obeys "never color alone" via the badge text.                                                                                                      |
| **Permission popups are non-modal and visible across projects; auto-Allow/Deny timers are opt-in per project** | The HIG discourages time-boxed UI; Hot Sheet's timers are explicit user settings, paused when hidden, with a visible pause control — an accepted, documented deviation.         |
| **Provider capability gating**: unsupported actions are hidden or explained, never emulated                    | Matches "show when a gesture/command is unavailable"; extends it to provider capabilities.                                                                                      |
| **Visual QA in a real browser before completion**                                                              | Stricter than the HIG's "preview on devices"; it is the enforcement mechanism for everything in this document.                                                                  |

---

## 10. Review gates

Use Kerf's `interaction-conventions.md` review protocol and Hot Sheet's
[AGENTS.md](../AGENTS.md) for real-browser visual QA, ticket evidence, and the
project's test gates. Review the production composition and its UX demo at wide
and narrow sizes; confirm enabled actions work in each shipped parent and record
states inspected on the ticket.

## 11. Follow-up tickets created with the original HIG review

| Ticket     | Scope                                                                                                                                                                                                              |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| HS2-DKZG9S | Web client follows system appearance: dark palette for every token and `--hs-*` alias, `prefers-color-scheme`, `prefers-contrast`, `prefers-reduced-motion`, `prefers-reduced-transparency`, contrast verification |
| HS2-PS29TA | Web typography: system font stack decision and a 13 px HIG-aligned type scale                                                                                                                                      |
| HS2-0P83KD | Historical native icon decision; the current project rule is Lucide across clients                                                                                                                                 |
| HS2-80VPPW | Command registry driving the Tauri native menu, the in-browser command surface, and the SwiftUI menu bar; coordinates with HS2-KTHGVE (platform-aware shortcuts)                                                   |
| HS2-A2A9GT | HIG conformance audit of every shipped web surface against §10                                                                                                                                                     |
| HS2-WBW3Z9 | AI labeling, feedback, undo/retry, and confirmation affordances per §6.1                                                                                                                                           |
| HS2-46RA38 | iOS/iPadOS platform considerations before the iOS client starts                                                                                                                                                    |

---
