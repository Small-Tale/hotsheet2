import type { Api, CheckoutTicketCounts, CheckoutTicketQuery, CorruptTicket, TicketRow } from './api';
import { applyBoardColumnFetch, type BoardColumnPage } from './board-pagination';

export interface ProjectTicketRefresh {
  corruptTickets?: CorruptTicket[];
  corruptTicketsError?: string;
  tickets?: TicketRow[];
  ticketCounts?: CheckoutTicketCounts;
  nextCursor?: string;
  ticketsError?: string;
  /** Per-column paging state when the refresh loaded board columns independently (HS2-HNZZHC). */
  boardPages?: Record<string, BoardColumnPage>;
}

/** Rows each board column loads per page (HS2-8NBGBX, HS2-HNZZHC). */
export const BOARD_COLUMN_PAGE_SIZE = 100;
/** The server's largest page (`CHECKOUT_READ_MAX_ROWS`). */
const MAX_CHECKOUT_PAGE_SIZE = 500;

/** One status-scoped board column and the ordered wire statuses it pages (`boardColumnStatuses`). */
export interface BoardColumnSpec {
  id: string;
  statuses: readonly string[];
}

/**
 * Load board columns independently (HS2-HNZZHC): every column reads its own status-filtered pages in
 * the requested sort, so a short column is never starved by a long one and never starts empty behind
 * a "Load more" button. Each column loads at least one `BOARD_COLUMN_PAGE_SIZE` page, or back up to
 * `wants[status]` rows so a refresh keeps a column the user already expanded. A multi-status column
 * (merged Completed + Verified) pages its statuses in order, continuing into the next status only
 * once the previous one is exhausted or when that status already had rows loaded. Only the first
 * request carries counts; the rest pass `counts=false`.
 */
export async function loadBoardColumnRefresh(
  client: Pick<Api, 'checkoutTicketPage'> & Partial<Pick<Api, 'checkoutTicketRowsPage'>>,
  checkout: string,
  query: CheckoutTicketQuery,
  columns: readonly BoardColumnSpec[],
  wants: Readonly<Record<string, number>> = {},
): Promise<{ tickets: TicketRow[]; counts?: CheckoutTicketCounts; pages: Record<string, BoardColumnPage> }> {
  let counts: CheckoutTicketCounts | undefined,
    countsRequested = false;
  // A legacy server answers with a plain row array (no paging or counts); treat it as one exhausted page and
  // keep its rows as-is, as the global loader does. The board buckets rows by status when it renders.
  const normalize = (
    page: { items: TicketRow[]; next_cursor?: string; counts?: CheckoutTicketCounts | null } | TicketRow[],
  ) => (Array.isArray(page) ? { items: page, next_cursor: undefined } : page);
  const fetchPage = async (size: number, cursor: string | undefined, status: string) => {
    const statusQuery = { ...query, status };
    if (!countsRequested || !client.checkoutTicketRowsPage) {
      countsRequested = true;
      const page = await client.checkoutTicketPage(checkout, size, cursor, statusQuery);
      if (!Array.isArray(page)) counts ??= page.counts;
      return normalize(page);
    }
    return normalize(await client.checkoutTicketRowsPage(checkout, size, cursor, statusQuery));
  };
  const loadStream = async (status: string, target: number) => {
    const rows: TicketRow[] = [];
    let cursor: string | undefined;
    do {
      const page = await fetchPage(Math.min(MAX_CHECKOUT_PAGE_SIZE, target - rows.length), cursor, status);
      rows.push(...page.items);
      cursor = page.next_cursor;
    } while (cursor && rows.length < target);
    return { rows, cursor };
  };
  const loadColumn = async (column: BoardColumnSpec) => {
    let rows: TicketRow[] = [],
      page: BoardColumnPage | undefined;
    for (const [index, status] of column.statuses.entries()) {
      const previousExhausted = index === 0 || page?.streams?.[column.statuses[index - 1]]?.exhausted === true,
        remaining = BOARD_COLUMN_PAGE_SIZE - rows.length,
        target = Math.max(wants[status] ?? 0, previousExhausted ? remaining : 0);
      if (target <= 0) continue;
      const stream = await loadStream(status, target);
      rows = [...rows, ...stream.rows];
      page = applyBoardColumnFetch(column.statuses, page, status, stream.cursor, rows.length);
    }
    return { id: column.id, rows, page: page ?? { loaded: 0, exhausted: false } };
  };
  // Columns load in parallel. Starting the first column issues its first request synchronously, so
  // that request is the only one carrying counts.
  const loaded = await Promise.all(columns.map(loadColumn));
  let tickets: TicketRow[] = [];
  const pages: Record<string, BoardColumnPage> = {};
  for (const column of loaded) {
    tickets = appendUniqueTicketRows(tickets, column.rows);
    pages[column.id] = column.page;
  }
  return { tickets, counts, pages };
}

export function appendUniqueTicketRows(current: readonly TicketRow[], incoming: readonly TicketRow[]): TicketRow[] {
  const known = new Set(current.map((ticket) => `${ticket.connection_id}:${ticket.native_id}`)),
    appended: TicketRow[] = [];
  for (const ticket of incoming) {
    const key = `${ticket.connection_id}:${ticket.native_id}`;
    if (known.has(key)) continue;
    known.add(key);
    appended.push(ticket);
  }
  return [...current, ...appended];
}

const message = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));

/** Board-mode refresh input: the status columns to load independently, plus rows already loaded per status. */
export interface BoardRefreshSpec {
  columns: readonly BoardColumnSpec[];
  wants?: Readonly<Record<string, number>>;
}

/**
 * Load healthy and corrupt ticket indexes without either request suppressing the other. With a board
 * spec, healthy rows load per column (`loadBoardColumnRefresh`) instead of as one global page.
 */
export async function loadProjectTicketRefresh(
  client: Pick<Api, 'checkoutCorruptTickets' | 'checkoutTicketPage'> & Partial<Pick<Api, 'checkoutTicketRowsPage'>>,
  checkout: string,
  query: CheckoutTicketQuery = { collection: 'queue' },
  board?: BoardRefreshSpec,
): Promise<ProjectTicketRefresh> {
  if (board) {
    const [columns, corruptTickets] = await Promise.allSettled([
      loadBoardColumnRefresh(client, checkout, query, board.columns, board.wants),
      client.checkoutCorruptTickets(checkout),
    ]);
    const diagnostics = corruptTickets.status === 'fulfilled' ? corruptTickets.value : undefined,
      corruptSlugs = new Set(diagnostics?.flatMap((item) => (item.slug ? [item.slug] : [])) ?? []);
    return {
      ...(columns.status === 'fulfilled'
        ? {
            tickets: columns.value.tickets.filter((ticket) => !corruptSlugs.has(ticket.slug)),
            ticketCounts: columns.value.counts,
            boardPages: columns.value.pages,
          }
        : { ticketsError: message(columns.reason) }),
      ...(corruptTickets.status === 'fulfilled'
        ? { corruptTickets: corruptTickets.value }
        : { corruptTicketsError: message(corruptTickets.reason) }),
    };
  }
  const [tickets, corruptTickets] = await Promise.allSettled([
    client.checkoutTicketPage(checkout, 200, undefined, query),
    client.checkoutCorruptTickets(checkout),
  ]);
  const diagnostics = corruptTickets.status === 'fulfilled' ? corruptTickets.value : undefined;
  const corruptSlugs = new Set(diagnostics?.flatMap((item) => (item.slug ? [item.slug] : [])) ?? []);
  const ticketPage = tickets.status === 'fulfilled' ? tickets.value : undefined;
  const ticketRows: TicketRow[] = Array.isArray(ticketPage) ? (ticketPage as TicketRow[]) : (ticketPage?.items ?? []);
  return {
    ...(tickets.status === 'fulfilled'
      ? {
          tickets: ticketRows.filter((ticket) => !corruptSlugs.has(ticket.slug)),
          ...(Array.isArray(ticketPage)
            ? {}
            : { ticketCounts: ticketPage?.counts, nextCursor: ticketPage?.next_cursor }),
        }
      : { ticketsError: message(tickets.reason) }),
    ...(corruptTickets.status === 'fulfilled'
      ? { corruptTickets: corruptTickets.value }
      : { corruptTicketsError: message(corruptTickets.reason) }),
  };
}
