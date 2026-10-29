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
- Routine ticket text edits keep a controlled draft, a 150 ms local recovery
  debounce, and one server write on blur, page hide, or explicit finish. The
  edit-start value is the merge base. The exact contract is in [AGENTS.md](../AGENTS.md).
- Permission decisions remain visible across projects. Their automatic timers are
  explicit per-project choices and pause when hidden. See
  [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.7.
- Use Lucide through the shared icon renderer on every client; each actionable
  context-menu item carries an icon. See [design-guidelines.md](design-guidelines.md)
  §9 for platform-specific Hot Sheet decisions.
- The planned Workers workspace view (HS2-T7EVFD) uses one column per observed
  worker, with the integrator first and supported background agents adjacent to
  their named parent. Phone columns page like the ticket board. Separate offered,
  live, planned, handoff, and remote CI states; label disconnected retained work
  Unresolved and unknown source coverage as unknown. Keep the shared ticket
  summary and inspector activation. See [06-clients.md](06-clients.md) §6.3 for
  the accepted design and implementation owners.

- Browser-native `window.prompt` dialogs are unsupported in the desktop shell. Use in-app
  input flows; the terminal drawer's unnecessary message action was removed (HS2-JVSFES).
  The remaining filename, command-group, and AI-launch prompts use controlled in-app inputs
  (HS2-WMZ6Z5); a source policy test prevents production native prompts.

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
