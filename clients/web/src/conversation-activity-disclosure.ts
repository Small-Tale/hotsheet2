import { signal } from 'kerfjs';

/**
 * Which AI chat activity groups are expanded (HS2-Y3SGH3). Groups start collapsed to their latest
 * item; clicking one toggles it. A group is identified by its connection and its first activity
 * id, which stays stable while later items append to the group, so a new item arriving while the
 * group is collapsed simply replaces the visible line. The state is device-session UI state: it
 * is not persisted and issues no network requests.
 */
export const expandedActivityGroups = signal<ReadonlySet<string>>(new Set());

/** The disclosure key for one activity group of one conversation connection. */
export function activityGroupKey(connectionId: string | undefined, groupId: string): string {
  return `${connectionId ?? ''}\u001f${groupId}`;
}

/** The expanded set after toggling `key`: added when collapsed, removed when expanded. */
export function toggledActivityGroups(expanded: ReadonlySet<string>, key: string): ReadonlySet<string> {
  const next = new Set(expanded);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  return next;
}

/** Toggle one activity group between collapsed and expanded. */
export function toggleActivityGroup(connectionId: string | undefined, groupId: string): void {
  expandedActivityGroups.value = toggledActivityGroups(
    expandedActivityGroups.value,
    activityGroupKey(connectionId, groupId),
  );
}

/** Whether the group is expanded; every group is collapsed until the person opens it. */
export function activityGroupExpanded(connectionId: string | undefined, groupId: string): boolean {
  return expandedActivityGroups.value.has(activityGroupKey(connectionId, groupId));
}
