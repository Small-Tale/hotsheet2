import type { Request } from '@playwright/test';

import { projectTabTicketState } from '../src/active-ticket-work';
import type { TicketRow } from '../src/api';

const closed = new Set(['completed', 'verified', 'backlog', 'archive', 'deleted']);

/**
 * The real `GET /checkouts/{ref}/tickets` answers a `page_size` request with the page envelope
 * and an unpaged request with a plain row list; fakes mirror that (HS2-5T7QHE). Counts are
 * derived from the rows the way the client derives them for an unpaged list.
 */
export function ticketsJson(request: Request, rows: readonly object[]) {
  const params = new URL(request.url()).searchParams;
  if (!params.has('page_size')) return rows;
  const work = projectTabTicketState(rows as TicketRow[]),
    statuses = rows.map((row) => (row as { status?: string }).status ?? 'not_started'),
    count = (status: string) => statuses.filter((item) => item === status).length;
  return {
    items: rows,
    counts:
      params.get('counts') === 'false'
        ? null
        : {
            total: rows.length,
            queued: statuses.filter((status) => !closed.has(status)).length,
            backlog: count('backlog'),
            archive: count('archive'),
            open: statuses.filter((status) => !closed.has(status)).length,
            up_next: work.upNextCount,
            active: work.activeTicketCount,
            started: count('started'),
            completed_today: 0,
          },
  };
}
