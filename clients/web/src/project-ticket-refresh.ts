import type { Api, CheckoutTicketCounts, CheckoutTicketQuery, CorruptTicket, TicketRow } from './api';
import { applyBoardColumnFetch, type BoardColumnPage } from './board-pagination';

export interface ProjectTicketRefresh {
  corruptTickets?: CorruptTicket[];
  corruptTicketsError?: string;
  tickets?: TicketRow[];
  ticketCounts?: CheckoutTicketCounts;
  nextCursor?: string;
  /** The exact query `nextCursor` belongs to, when a caller records it with the cursor (HS2-38MPMC). */
  nextCursorQuery?: CheckoutTicketQuery;
  ticketsError?: string;
  sourceErrors?: string[];
  /** Per-column paging state when the refresh loaded board columns independently (HS2-HNZZHC). */
  boardPages?: Record<string, BoardColumnPage>;
}

/** Rows each board column displays per page (HS2-8NBGBX, HS2-HNZZHC). */
export const BOARD_COLUMN_PAGE_SIZE = 100;
/** Keep one extra Completed page ready for an optimistic bulk move (HS2-KA1VJS). */
const COMPLETED_COLUMN_INITIAL_ROWS = BOARD_COLUMN_PAGE_SIZE * 2;
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
): Promise<{
  tickets: TicketRow[];
  counts?: CheckoutTicketCounts;
  pages: Record<string, BoardColumnPage>;
  sourceErrors: string[];
}> {
  let counts: CheckoutTicketCounts | undefined,
    countsRequested = false;
  const sourceErrors = new Set<string>();
  const fetchPage = async (size: number, cursor: string | undefined, status: string) => {
    const statusQuery = { ...query, status };
    if (!countsRequested || !client.checkoutTicketRowsPage) {
      countsRequested = true;
      const page = await client.checkoutTicketPage(checkout, size, cursor, statusQuery);
      counts ??= page.counts;
      page.source_errors?.forEach((error) => sourceErrors.add(error));
      return page;
    }
    const page = await client.checkoutTicketRowsPage(checkout, size, cursor, statusQuery);
    page.source_errors?.forEach((error) => sourceErrors.add(error));
    return page;
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
        remaining = (column.id === 'completed' ? COMPLETED_COLUMN_INITIAL_ROWS : BOARD_COLUMN_PAGE_SIZE) - rows.length,
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
  return { tickets, counts, pages, sourceErrors: [...sourceErrors] };
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
/** Rows a single-collection refresh requests when nothing has been paged yet. */
export const TICKET_PAGE_SIZE = 200;

/**
 * One single-collection page sequence of at least `target` rows (fewer when the collection ends):
 * the first request carries counts, later ones skip them, and each asks for at most the server's
 * maximum page size. A refresh of a list the user already paged keeps every loaded row instead of
 * snapping back to the first page (HS2-9DF0DA).
 */
async function loadCollectionRows(
  client: Pick<Api, 'checkoutTicketPage'> & Partial<Pick<Api, 'checkoutTicketRowsPage'>>,
  checkout: string,
  query: CheckoutTicketQuery,
  target: number,
) {
  const first = await client.checkoutTicketPage(checkout, Math.min(MAX_CHECKOUT_PAGE_SIZE, target), undefined, query);
  const items = [...first.items],
    sourceErrors = new Set(first.source_errors ?? []);
  let cursor = first.next_cursor;
  while (cursor && items.length < target) {
    const size = Math.min(MAX_CHECKOUT_PAGE_SIZE, target - items.length),
      page = client.checkoutTicketRowsPage
        ? await client.checkoutTicketRowsPage(checkout, size, cursor, query)
        : await client.checkoutTicketPage(checkout, size, cursor, query);
    items.push(...page.items);
    page.source_errors?.forEach((error) => sourceErrors.add(error));
    cursor = page.next_cursor;
  }
  return {
    ...first,
    items: appendUniqueTicketRows([], items),
    next_cursor: cursor,
    ...(sourceErrors.size ? { source_errors: [...sourceErrors] } : {}),
  };
}

export async function loadProjectTicketRefresh(
  client: Pick<Api, 'checkoutCorruptTickets' | 'checkoutTicketPage'> & Partial<Pick<Api, 'checkoutTicketRowsPage'>>,
  checkout: string,
  query: CheckoutTicketQuery = { collection: 'queue' },
  board?: BoardRefreshSpec,
  /** Rows already loaded for this collection; a refresh brings back at least this many (HS2-9DF0DA). */
  loadedRows = 0,
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
            ...(columns.value.sourceErrors.length ? { sourceErrors: columns.value.sourceErrors } : {}),
            ...(columns.value.sourceErrors.length ? { ticketsError: columns.value.sourceErrors.join(' · ') } : {}),
          }
        : { ticketsError: message(columns.reason) }),
      ...(corruptTickets.status === 'fulfilled'
        ? { corruptTickets: corruptTickets.value }
        : { corruptTicketsError: message(corruptTickets.reason) }),
    };
  }
  const [tickets, corruptTickets] = await Promise.allSettled([
    loadedRows > TICKET_PAGE_SIZE
      ? loadCollectionRows(client, checkout, query, loadedRows)
      : client.checkoutTicketPage(checkout, TICKET_PAGE_SIZE, undefined, query),
    client.checkoutCorruptTickets(checkout),
  ]);
  const diagnostics = corruptTickets.status === 'fulfilled' ? corruptTickets.value : undefined;
  const corruptSlugs = new Set(diagnostics?.flatMap((item) => (item.slug ? [item.slug] : [])) ?? []);
  const ticketPage = tickets.status === 'fulfilled' ? tickets.value : undefined;
  const ticketRows: TicketRow[] = ticketPage?.items ?? [];
  return {
    ...(tickets.status === 'fulfilled'
      ? {
          tickets: ticketRows.filter((ticket) => !corruptSlugs.has(ticket.slug)),
          ticketCounts: ticketPage?.counts,
          nextCursor: ticketPage?.next_cursor,
          ...(ticketPage?.source_errors?.length ? { sourceErrors: ticketPage.source_errors } : {}),
          ...(ticketPage?.source_errors?.length ? { ticketsError: ticketPage.source_errors.join(' · ') } : {}),
        }
      : { ticketsError: message(tickets.reason) }),
    ...(corruptTickets.status === 'fulfilled'
      ? { corruptTickets: corruptTickets.value }
      : { corruptTicketsError: message(corruptTickets.reason) }),
  };
}

/** Whether an API error is the server rejecting a checkout cursor its query no longer matches (HS2-38MPMC). */
export function isStaleTicketCursorError(message: string) {
  return message.includes('stale checkout ticket cursor');
}
