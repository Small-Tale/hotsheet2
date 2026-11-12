import { signal } from 'kerfjs';

import type { TicketGroupDialogState } from './components/ticket-group-dialog';
import type { CheckoutGroupSummary } from './server-response-parsers';
import {
  ticketGroupCollapseKey,
  ticketGroupDisplayName,
  ticketGroupKey,
  ticketGroupNameKey,
  ticketGroupSummaries,
} from './ticket-groups';

/**
 * Device-local ticket group UI state (HS2-3XNTSD): which groups are collapsed, the group a
 * dragged ticket is over, and the open group name dialog. None of
 * it is sent to the server; only the collapse preference is persisted.
 */
const STORAGE_KEY = 'hotsheet.ticket-groups.collapsed';

function browserStorage(): Storage | undefined {
  try {
    return typeof localStorage === 'undefined' ? undefined : localStorage;
  } catch {
    return undefined;
  }
}

/** The stored collapse keys; anything unreadable or malformed reads as none collapsed. */
export function readCollapsedTicketGroups(storage: Pick<Storage, 'getItem'> | undefined): ReadonlySet<string> {
  try {
    const stored: unknown = JSON.parse(storage?.getItem(STORAGE_KEY) ?? '[]');
    return new Set(Array.isArray(stored) ? stored.filter((value): value is string => typeof value === 'string') : []);
  } catch {
    return new Set();
  }
}

/**
 * Collapsed groups as `ticketGroupCollapseKey` entries: a per-viewer preference per project,
 * view, and group (HS2-WZ3JGZ). One entry collapses the group in the list and in every column.
 */
export const collapsedTicketGroups = signal<ReadonlySet<string>>(readCollapsedTicketGroups(browserStorage()));

export function ticketGroupCollapsed(projectId: string, view: string, group: string): boolean {
  return collapsedTicketGroups.value.has(ticketGroupCollapseKey(projectId, view, group));
}

/** Collapse or expand one group in one project view, and remember the choice. */
export function toggleTicketGroupCollapsed(
  projectId: string,
  view: string,
  group: string,
  storage: Pick<Storage, 'setItem'> | undefined = browserStorage(),
): void {
  const key = ticketGroupCollapseKey(projectId, view, group),
    next = new Set(collapsedTicketGroups.value);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  collapsedTicketGroups.value = next;
  try {
    storage?.setItem(STORAGE_KEY, JSON.stringify([...next]));
  } catch {
    // The preference still applies for this page when storage is unavailable.
  }
}

/** The identity keys of the groups collapsed in one project view, for a collection's `collapsedGroups` prop. */
export function collapsedTicketGroupKeys(projectId: string, view: string, groups: readonly string[]): string[] {
  return groups.filter((group) => ticketGroupCollapsed(projectId, view, group)).map(ticketGroupKey);
}

/** The group container a dragged ticket is over: its collection scope and group identity key (HS2-PRXD39). */
export interface TicketGroupDropTarget {
  scope: string;
  key: string;
}
export const ticketGroupDropTarget = signal<TicketGroupDropTarget | undefined>(undefined);

/** The open New Group or Rename Group dialog, with the tickets a new group will hold. */
export interface TicketGroupDialogSession extends TicketGroupDialogState {
  projectId: string;
  slugs: string[];
}
export const ticketGroupDialog = signal<TicketGroupDialogSession | undefined>(undefined);

/**
 * The group summaries the server last reported per project (HS2-PDSHNN, HS2-9KZSD4): each
 * stored value's display name, member count, and flags. Loaded tickets store most groups, but a
 * paginated view can leave a group's members unloaded, and only the registry knows an id's name.
 */
export const projectTicketGroups = signal<Readonly<Partial<Record<string, readonly CheckoutGroupSummary[]>>>>({});

/** The display name of a stored group value in one project, from its last reported summaries. */
export function projectTicketGroupName(projectId: string | undefined, value: string): string {
  return ticketGroupDisplayName(
    value,
    (projectId === undefined ? undefined : projectTicketGroups.value[projectId]) ?? [],
  );
}

/** Every project's reported groups, for lookups that span projects; ids are unique across them. */
function everyReportedTicketGroup(): CheckoutGroupSummary[] {
  return Object.values(projectTicketGroups.value).flatMap((groups) => groups ?? []);
}

/**
 * Whether a stored group value matches a `group:` search term (HS2-44W3TP, HS2-9KZSD4): the term
 * names the group's id or legacy value, or its display name in any casing and spacing.
 */
export function ticketGroupMatchesSearch(value: string | undefined, term: string): boolean {
  if (value === undefined) return false;
  if (ticketGroupKey(value) === ticketGroupKey(term)) return true;
  return ticketGroupNameKey(ticketGroupDisplayName(value, everyReportedTicketGroup())) === ticketGroupNameKey(term);
}

/** The distinct display names of the given tickets' groups, for `group:` completion. */
export function ticketGroupSearchNames(tickets: readonly { group?: string | null }[]): string[] {
  const names = new Map<string, string>();
  for (const group of ticketGroupSummaries(tickets, everyReportedTicketGroup()))
    if (!names.has(ticketGroupNameKey(group.name))) names.set(ticketGroupNameKey(group.name), group.name);
  return [...names.values()];
}

/** Clear every transient group surface, as when the project or view changes. */
export function resetTicketGroupSurfaces(): void {
  ticketGroupDropTarget.value = undefined;
  ticketGroupDialog.value = undefined;
}
