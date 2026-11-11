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

- App Settings → General offers a **Theme** choice (HS2-EFTHS6): Auto (the default,
  following `prefers-color-scheme`), Light, or Dark, per browser in
  `localStorage hotsheet.theme`. This is a maintainer-requested departure from
  following the system only. A pinned choice uses Kerf's root appearance scope
  (`.kui-light` / `.kui-dark` on `<html>`), never app color overrides. Each choice
  shows a stylized Hot Sheet window in that scheme, after news-cloud2's Appearance
  picker. Kerf has no preview-tile choice, so the tiles are native radios with an
  app-owned frame (`components/theme-picker.css`) until `KF-C29WJ9` ships.

- Ticket groups ([35-ticket-groups.md](35-ticket-groups.md)) carry two interim
  decisions that wait on Kerf. A group's Rename and Ungroup actions open from a
  context menu on the group header, because `ListHeader` cannot pair its toggle
  with a trailing action (`KF-X2MRCT`); replace it with a visible overflow button
  when that ships. The group title follows `SummaryRow`'s compaction width in an
  app container query to stay aligned with the ticket type icons (`KF-J8GMA2`);
  remove the query when Kerf offers a public alignment (HS2-8CDFKR, HS2-WZ3JGZ).

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
