export interface TicketCompletion {
  completed_at?: string;
}

function localDayKey(value: Date): string {
  return `${value.getFullYear()}-${String(value.getMonth() + 1).padStart(2, '0')}-${String(value.getDate()).padStart(2, '0')}`;
}

export function ticketCompletionTrend(tickets: readonly TicketCompletion[], now = new Date(), days = 7): number[] {
  if (days <= 0) return [];
  const keys = Array.from({ length: days }, (_, index) => {
    const day = new Date(now);
    day.setHours(0, 0, 0, 0);
    day.setDate(day.getDate() - (days - index - 1));
    return localDayKey(day);
  });
  const indexes = new Map(keys.map((key, index) => [key, index]));
  const counts = keys.map(() => 0);
  for (const ticket of tickets) {
    if (!ticket.completed_at) continue;
    const completed = new Date(ticket.completed_at);
    if (Number.isNaN(completed.valueOf())) continue;
    const index = indexes.get(localDayKey(completed));
    if (index !== undefined) counts[index] += 1;
  }
  return counts;
}

export function completionDayStarts(now = new Date(), days = 7): string[] {
  return Array.from({ length: days + 1 }, (_, index) => {
    const day = new Date(now);
    day.setHours(0, 0, 0, 0);
    day.setDate(day.getDate() - (days - index - 1));
    return day.toISOString();
  });
}
