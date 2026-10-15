import './ticket-timeline.css';

import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { Text } from '@kerfjs/ui/text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';

import type { TimelineUsageRow } from '../ticket-usage-timeline';
import { TicketInspectorPanel, type TicketInspectorPanelPresentation } from './ticket-inspector-panel';

export interface TicketTimelineEntry {
  id: string;
  time: string;
  title: string;
  subtitle?: string;
  /** Elapsed time from this event to the next chronological event. */
  durationToNext?: string;
  /** AI usage recorded between this event and the next (or since the last event), for example
   * `38K tokens · opus` (HS2-ZA7W6E). */
  usageToNext?: string;
  emphasized?: boolean;
}
export const DEFAULT_TIMELINE_ENTRIES: readonly TicketTimelineEntry[] = [
  {
    id: 'started',
    time: '1h ago',
    title: 'Claude started work',
    subtitle: 'Plan: build the shared inspector and timeline states.',
    emphasized: true,
  },
  {
    id: 'metadata',
    time: '42m ago',
    title: 'Updated ticket metadata',
    subtitle: 'Set category, priority, and status.',
  },
  {
    id: 'implementation',
    time: '18m ago',
    title: 'Implemented inspector interactions',
    subtitle: 'Added blocked-state editing and reader navigation.',
    emphasized: true,
  },
  {
    id: 'review',
    time: 'Now',
    title: 'Ready for review',
    subtitle: 'All checks pass. Please review the changes.',
    emphasized: true,
  },
];
export function TicketTimeline({
  entries = DEFAULT_TIMELINE_ENTRIES,
  presentation = 'sidebar',
  usage,
}: {
  entries?: readonly TicketTimelineEntry[];
  presentation?: TicketInspectorPanelPresentation;
  /** The whole ticket's AI usage under the rail: one row per model, then the total (HS2-J14225). */
  usage?: readonly TimelineUsageRow[];
}) {
  return (
    <TicketInspectorPanel component="ticket-timeline" className="ticket-timeline" presentation={presentation}>
      {/* Kerf owns the section stack, heading, and text; only the rail is app-drawn (HS2-52D1JZ). */}
      <List gap="xs">
        <ListHeader label="Timeline" />
        <ol class="ticket-timeline__list">
          {entries.map((entry, index) => (
            <li data-entry-id={entry.id} data-emphasized={String(Boolean(entry.emphasized))}>
              {/* The time reads like the event title beside it, apart from the quiet elapsed-time
                  labels between events (HS2-J14225). */}
              <Text variant="span" size="compact" lineHeight="tight" wrap="nowrap">
                <strong>
                  <time>{entry.time}</time>
                </strong>
              </Text>
              <div>
                <Text variant="p" size="compact" lineHeight="tight" flush>
                  <strong>{entry.title}</strong>
                </Text>
                {entry.subtitle && (
                  <Text variant="p" tone="quiet" size="compact" flush>
                    {entry.subtitle}
                  </Text>
                )}
              </div>
              {((index < entries.length - 1 && entry.durationToNext) || entry.usageToNext) && (
                <span
                  class="ticket-timeline__duration"
                  aria-label={[
                    index < entries.length - 1 && entry.durationToNext
                      ? `Elapsed to next event: ${entry.durationToNext}`
                      : undefined,
                    entry.usageToNext
                      ? `AI usage ${index < entries.length - 1 ? 'until the next event' : 'since this event'}: ${entry.usageToNext}`
                      : undefined,
                  ]
                    .filter(Boolean)
                    .join('. ')}
                >
                  {index < entries.length - 1 && entry.durationToNext}
                  {index < entries.length - 1 && entry.durationToNext && entry.usageToNext && ' · '}
                  {entry.usageToNext && <span class="ticket-timeline__usage">{entry.usageToNext}</span>}
                </span>
              )}
            </li>
          ))}
        </ol>
        {usage?.length ? (
          <ValueTable label="AI usage for this ticket" density="compact">
            {usage.map((row) => (
              <ValueTableRow label={row.label} value={row.value} />
            ))}
          </ValueTable>
        ) : undefined}
        <Text variant="p" size="compact" flush>
          {entries.length} {entries.length === 1 ? 'event' : 'events'} total
        </Text>
      </List>
    </TicketInspectorPanel>
  );
}
