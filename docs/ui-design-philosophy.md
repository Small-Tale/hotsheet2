# Hot Sheet UI conventions

Kerf UI owns the shared interaction conventions and design philosophy. Read the
installed `@kerfjs/ui/docs/interaction-conventions.md` and its linked
`design-philosophy.md` before composing a Hot Sheet surface. The installed
`@kerfjs/ui/ai/component-catalog.json` and Hot Sheet's
`clients/web/ai/component-catalog-extension.json` define component selection and
geometry ownership. This file records Hot Sheet decisions and ticket evidence; it
does not fork Kerf's guidance.

## Product decisions

- Use the ticket's qualified provider identity for actions and links. A list row
  and board card share one ticket-summary contract; provider capability determines
  whether an action is enabled. See [06-clients.md](06-clients.md) and
  [ux-components.md](ux-components.md).
- `Needs review` names feedback-needed state. Live AI work is signaled by a
  renewable claim, while `Started` and `started_phase` are durable ticket state.
  Keep those meanings distinct in badges and timelines.
- Ticket text editing deviates from Kerf's "Text, state, and continuity" section,
  which persists routine edits after a short debounce (HS2-RE1PS6). Hot Sheet
  never writes to the server while the user is typing, so one editing session makes
  one write and one Git commit instead of one per pause, and nothing this client
  typed can return as "their" version. Keystrokes update the controlled draft and,
  after 150 ms, a local recovery copy keyed by project, ticket, and field that keeps
  the edit-start value. One server write follows blur, page hide, or an explicit
  finish, merged against that value. Reopening restores an unsaved copy; discrete
  edits such as tag chips save immediately. See [06-clients.md](06-clients.md)
  §6.3 for the full contract.
- Minimized ticket readers (HS2-Q8WF64, signed off as design C) follow Kerf's Actions,
  feedback, and dialogs defaults: a reader is modal only while it is restored, and Close All
  confirms only when a draft would be lost. Minimized tickets sit in the workspace's existing
  floating cluster (`FloatingToolbar` with a `ToolbarControlGroup`, a corner `Badge`, and a
  `PopupMenu`), per project tab, with Lucide `minus` for Minimize and `layers-2` for minimized
  tickets everywhere. Pressing Minimize keeps an unsent note as a draft instead of finishing it,
  the one exception to the note composer's finish-on-blur. Motion is an app-owned stand-in card,
  because the reader dialog is Kerf's and lives in the top layer. ⌥⇧⌘M minimizes or restores,
  since ⌘M is the OS window's. Interim decision waiting on Kerf `KF-HZTE26`:
  `PopupMenu` items have no second line, so a long title wraps at phone width. See
  [06-clients.md](06-clients.md) §6.3.
- Permission decisions remain visible across projects. Their automatic timers are
  explicit per-project choices and pause when hidden. See
  [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.7.
- Use Lucide through the shared icon renderer on every client; each actionable
  context-menu item carries an icon. See [design-guidelines.md](design-guidelines.md)
  §9 for platform-specific Hot Sheet decisions.
- The Workers workspace view (HS2-T7EVFD, HS2-E9SR7R) uses one column per observed
  worker, with the integrator first and supported background agents adjacent to
  their named parent. Phone columns page like the ticket board. Separate offered,
  live, planned, handoff, and remote CI states; label disconnected retained work
  Unresolved and unknown source coverage as unknown. Keep the shared ticket
  summary and inspector activation. See [06-clients.md](06-clients.md) §6.3 for
  the accepted design and implementation owners.

- The Orchestration view (HS2-J89JC7, signed off on HS2-HSMH2Q) is the production ticket
  board read as a timeline of dependency waves, newest on the left and Done on the right,
  with each column's cards grouped by project. Its cards are other projects' tickets: they
  keep the shared ticket summary, gain its orchestration line (state, placement, goal),
  and select plan steps rather than tickets of this project, so the ticket collection's
  context menus and group menus stay out of the view. Interim decision waiting on Kerf
  `KF-QHJKA4`: Kerf ColumnTrack cannot open at its end or at a named column, so the view's
  wiring sets the scroll position once per board layout and on a width change (no app
  CSS); remove it when that option ships. See
  [26-orchestration-projects.md](26-orchestration-projects.md) §26.26.

- Browser-native `window.prompt` dialogs are unsupported in the desktop shell. Use in-app
  input flows; the terminal drawer's unnecessary message action was removed (HS2-JVSFES).
  The remaining filename, command-group, and AI-launch prompts use controlled in-app inputs
  (HS2-WMZ6Z5). ESLint now rejects every native `alert`, `confirm`, `prompt`, and
  `window.open` call (HS2-ECT6FE); see [tauri-desktop.md](tauri-desktop.md).
- A question before an action that undo cannot cover uses the shared in-app
  `ConfirmationDialog` through `createConfirmation().request(...)` (HS2-6S1346), never a
  per-feature dialog. Erasing a gallery annotation asks nothing, because markup undo and
  Discard Changes cover it; withdrawing an AI rating confirms because it drops the
  explanation, and declining edits the feedback instead.

- Application banners above the workspace (setup warnings, Codex hook review, update and
  source notices, repository size, paused notifications, Hot Sheet 1 migration, and the
  connection banner) use one inset treatment: Kerf's rounded `StateBanner` inset by a
  Kerf `List` (`controlInsets="trl"`) on the header surface, through `ShellBanner`
  (HS2-YCB15Z). No banner is a flush full-width strip.

- A `StateBanner` whose detail is computed (a server or validation message, or an
  interpolated sentence) sets Kerf's `copyLayout="stacked"`, so the detail wraps
  instead of ending in an ellipsis at phone width (HS2-4JRM43, HS2-NRDDPY). The
  default inline layout stays for constant short details. `kerf-ui-profile.test.ts`
  pins the rule.

- App Settings → General offers a **Theme** choice (HS2-EFTHS6): Auto (the default,
  following `prefers-color-scheme`), Light, or Dark, per browser in
  `localStorage hotsheet.theme`. This is a maintainer-requested departure from
  following the system only. A pinned choice uses Kerf's root appearance scope
  (`.kui-light` / `.kui-dark` on `<html>`), never app color overrides. Each choice
  shows a stylized Hot Sheet window in that scheme, after news-cloud2's Appearance
  picker. Kerf has no preview-tile choice, so the tiles are native radios with an
  app-owned frame (`components/theme-picker.css`) until `KF-C29WJ9` ships.
  The pin also reaches the chrome outside the document (HS2-15W35F). Both
  `theme-color` metas keep their `prefers-color-scheme` media queries; a pinned
  choice gives both the pinned scheme's color (`#ffffff` or `#1c1c1e`), so the
  browser toolbar and an installed web app's title bar match whatever the system
  is, and Auto restores each meta's own color. In the packaged desktop app the
  main window's native theme (macOS title bar and traffic-light area) is set
  through Tauri's `set_theme` with `light` or `dark`, and Auto passes `null` so
  the window follows the system again. The launch-origin capability grants only
  `core:window:allow-set-theme` for this; a browser or a debug desktop build
  without that capability skips the call.

- Ticket groups ([35-ticket-groups.md](35-ticket-groups.md)) follow Kerf's Lists
  and selection defaults with no deviation: the group header's commands open from
  a `ListHeader` `overflowAction` button and from its context menu, and the title
  aligns with the ticket rows through `leadingEdge="summary-row"` (HS2-HKXSB1).

## Hot Sheet ticket lessons

Kerf's house grammar carries the general rules. These ticket clusters explain
where Hot Sheet's own compositions exposed them:

| Hot Sheet evidence                       | Local lesson                                                                                                     |
| ---------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `HS2-AYSQ1C`, `HS2-M3CDNG`, `HS2-GGE650` | Ticket updates interrupted an open workspace; preserve draft, focus, selection, and scroll through live changes. |
| `HS2-3QXHF3`, `HS2-8H7F72`, `HS2-W3FDPM` | Restore project view, pane size, and unfinished ticket edits at their own scopes.                                |
| `HS2-VW5EW4`, `HS2-R2FGZ5`, `HS2-Z0M2VV` | A demo action passed while its shipped parent composition was inert; exercise both.                              |
| `HS2-RN6X3E`, `HS2-SWC9E4`, `HS2-MVJ85W` | Menus and dialogs needed shared placement, focus, and dismissal behavior in the actual shell.                    |
| `HS2-CSYQHX`, `HS2-E2AZ2N`, `HS2-N718B0` | Ticket lifecycle copy confused unknown, empty, busy, and done states.                                            |
| `HS2-7CTQJC`, `HS2-CV0J2E`, `HS2-E729WG` | Focused fixes changed neighboring behavior; preserve established affordances outside the request.                |

The Kerf review protocol is in `@kerfjs/ui/docs/interaction-conventions.md`.
Hot Sheet's required rendered browser pass, viewport coverage, and ticket evidence
are in [AGENTS.md](../AGENTS.md).
