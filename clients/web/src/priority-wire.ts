import type { TicketPriority } from './components/ticket-row';

/** Convert the canonical HS2 wire priority into the terminology shown by the client. */
export function priorityFromWire(value?: string): TicketPriority {
  if (value === 'highest' || value === 'urgent') return 'urgent';
  if (value === 'high' || value === 'low') return value;
  return 'default';
}

/** Convert client-facing priority terminology into the canonical HS2 wire value. */
export function priorityToWire(value?: unknown): unknown {
  return value === 'urgent' ? 'highest' : value;
}

/** Translate priority fields without mutating the caller's request object. */
export function prioritiesToWire<T extends Record<string, unknown>>(value: T): T {
  if (!Object.hasOwn(value, 'priority')) return value;
  return { ...value, priority: priorityToWire(value.priority) };
}
