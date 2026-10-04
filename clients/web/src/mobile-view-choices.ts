import { type SelectChoice } from '@kerfjs/ui/select';

/** What the narrow-layout view picker needs to know about the project (HS2-0VPMFS). */
export interface MobileViewChoiceInput {
  /** The selected ticket view, including `errors` and custom view ids. */
  selectedView: string;
  /** Trashed tickets in the project; Trash is offered while any exist or it is selected. */
  trashCount: number;
  /** Corrupt ticket files the project's diagnostics found. */
  corruptCount: number;
  customViews: ReadonlyArray<{ value: string; label: string }>;
}

/**
 * The narrow-layout view picker's choices, mirroring the sidebar: Queue, Backlog, Archive, Trash
 * while it holds tickets or is open, custom views, and Ticket errors while diagnostics found corrupt
 * tickets or that view is open, so the picker always names the visible view (HS2-0VPMFS).
 */
export function mobileViewChoices({
  selectedView,
  trashCount,
  corruptCount,
  customViews,
}: MobileViewChoiceInput): SelectChoice[] {
  return [
    { value: 'all', label: 'Queue' },
    { value: 'backlog', label: 'Backlog' },
    { value: 'archive', label: 'Archive' },
    ...(trashCount > 0 || selectedView === 'trash' ? [{ value: 'trash', label: 'Trash' }] : []),
    ...customViews,
    ...(corruptCount > 0 || selectedView === 'errors' ? [{ value: 'errors', label: 'Ticket errors' }] : []),
  ];
}
