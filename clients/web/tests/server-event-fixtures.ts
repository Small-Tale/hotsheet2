/**
 * Server-shaped ticket change, nudge and pending-permission builders shared by the web E2E specs
 * (HS2-GM6A8W, HS2-CSPDW8, HS2-Y59J3K). Each builder mirrors one server emitter and is checked against the client's
 * contract types with `satisfies`, so a fixture cannot drift into a shape the server never sends.
 */
import type { ChangeEvent, PermissionRequest, Ticket } from '../src/api';
import type { PermissionDecision, PermissionScope } from '../src/permission-notifications';

/**
 * The per-ticket change kinds the server publishes (`changed_in` / `removed_in` in
 * `crates/hotsheet-server/src/events.rs`, the store watcher, and provider write-behind and webhooks).
 */
export type TicketChangeKind =
  | 'created'
  | 'updated'
  | 'deleted'
  | 'changed'
  | 'closed'
  | 'moved'
  | 'claimed'
  | 'released'
  | 'renewed'
  | 'assigned'
  | 'note_added'
  | 'attachment_added'
  | 'attachment_removed'
  | 'attachment_renamed'
  | 'attachment_metadata_updated'
  | 'attachment_annotations_updated'
  | 'attachment_markup_updated';

/**
 * The server's per-ticket change event: `store` is the ticket's connection id (a Markdown store's
 * `store_url_id`, or the provider connection), `id` is the native ticket id, and `slug` is the
 * ticket's slug; `message` is omitted. The watcher's `deleted` for a vanished file has an empty
 * slug, so pass `slug: ''` for that case (HS2-Y59J3K).
 */
export const ticketChangeEvent = (
  kind: TicketChangeKind,
  ticket: Pick<Ticket, 'connection_id' | 'native_id'> & { slug: string },
) => ({ store: ticket.connection_id, kind, id: ticket.native_id, slug: ticket.slug }) satisfies ChangeEvent;

/**
 * The mocked project's permission identity: `ask_project` (`crates/hotsheet-server/src/routes/permissions.rs`)
 * names a registered checkout's ask `checkout:<id>` (HS2-K7CZR2, HS2-GM6A8W).
 */
export const DEMO_PERMISSION_PROJECT = 'checkout:demo-checkout';

/**
 * The server's `permission_asked` nudge (`crates/hotsheet-server/src/lib.rs`): `store` is the asking
 * project's identity (`checkout:<id>` for a registered checkout), `id` is the request id, `slug` is
 * the tool, and `message` is omitted rather than null (HS2-K7CZR2). It never carries a `turn`: a
 * driven turn's permission ask arrives separately as a `turn_event` (HS2-CSPDW8).
 */
export const permissionAskedEvent = (id: string, tool = 'Bash', store = DEMO_PERMISSION_PROJECT) =>
  ({ store, kind: 'permission_asked', id, slug: tool }) satisfies ChangeEvent;

/**
 * The server's `permission_resolved` nudge for an answered request (`routes/terminals/signals.rs`):
 * `store` is the request's project, `slug` is empty, and `message` is `<decision>:<scope>` (HS2-GM6A8W).
 */
export const permissionAnsweredEvent = (
  id: string,
  answer: `${PermissionDecision}:${PermissionScope}`,
  store = DEMO_PERMISSION_PROJECT,
) => ({ store, kind: 'permission_resolved', id, slug: '', message: answer }) satisfies ChangeEvent;

/**
 * The server's `permission_resolved` nudge for a request removed without an answer from this
 * client's routes (timeout, withdrawn ask; `set_on_removed` in `crates/hotsheet-server/src/lib.rs`):
 * `store` is the request's project, `slug` is the tool, and `message` is omitted (HS2-GM6A8W).
 */
export const permissionRemovedEvent = (id: string, tool = 'Bash', store = DEMO_PERMISSION_PROJECT) =>
  ({ store, kind: 'permission_resolved', id, slug: tool }) satisfies ChangeEvent;

/**
 * One `GET /permissions` entry as the server's `PermissionInfo` (`routes/permissions.rs`) serializes it:
 * `project`, `always_allow_supported` and `suggestions` are always present; `agent`, `requester` and
 * `orchestrator` are omitted when absent (HS2-GM6A8W).
 */
export type ServerPermissionInfo = { [Key in keyof PermissionRequest]: PermissionRequest[Key] } & Required<
  Pick<PermissionRequest, 'project' | 'always_allow_supported' | 'suggestions'>
>;
export const pendingPermission = <Request extends Pick<PermissionRequest, 'id' | 'connection' | 'tool' | 'action'>>(
  request: Request & Partial<ServerPermissionInfo>,
): ServerPermissionInfo & Request => ({
  project: DEMO_PERMISSION_PROJECT,
  always_allow_supported: false,
  suggestions: [],
  ...request,
});

/**
 * The server's terminal nudges (`emit_terminal_question` / `emit_terminal_halted` in
 * `routes/terminals/signals.rs`) are not store-scoped: `store` and `slug` are empty, and only a
 * detected halt carries its message (HS2-GM6A8W).
 */
export const terminalEvent = (kind: 'terminal_question' | 'terminal_halted', id: string, message?: string) =>
  ({ store: '', kind, id, slug: '', ...(message === undefined ? {} : { message }) }) satisfies ChangeEvent;
