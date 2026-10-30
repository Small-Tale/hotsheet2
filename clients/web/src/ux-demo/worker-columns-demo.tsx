import type { TicketRow as WireTicketRow } from '../api';
import type { TicketRowProps } from '../components/ticket-row';
import { WorkerColumns } from '../components/worker-columns';
import type { WorkerRow, WorkerTicket, WorkerView } from '../worker-view-model';
import { collectionTickets } from './ticket-collections-demo';

function demoTicket(ticket: TicketRowProps): WireTicketRow {
  return {
    connection_id: 'demo',
    native_id: ticket.slug,
    qualified_id: `demo:${ticket.slug}`,
    id: ticket.slug,
    slug: ticket.slug,
    title: ticket.title,
    category: ticket.category,
    priority: ticket.priority,
    status: ticket.status,
    started_phase: ticket.startedPhase,
    up_next: Boolean(ticket.upNext),
    feedback_needed: false,
    tags: ticket.tags,
    blocked_by: [],
    claim_count: 0,
  };
}
function assignment(ticket: WireTicketRow, phase: WorkerTicket['assignment_phase'], claim: boolean): WorkerTicket {
  return {
    ticket: { connection_id: ticket.connection_id, native_id: ticket.native_id },
    slug: ticket.slug,
    assignment_id: `assignment-${ticket.slug}`,
    assignment_phase: phase,
    claim_expires_at: claim ? Date.now() / 1000 + 3600 : null,
    started_phase: ticket.started_phase ?? null,
    eta_at: claim ? new Date(Date.now() + 45 * 60_000).toISOString() : null,
  };
}
function worker(
  id: string,
  role: WorkerRow['role'],
  tickets: WorkerTicket[],
  planned: WorkerRow['planned'],
): WorkerRow {
  return {
    id,
    role,
    generation: 'demo-generation',
    parent: null,
    label: id,
    tool: 'Codex',
    connection_kind: 'terminal',
    connection_id: `terminal-${id}`,
    connection_available: true,
    observed_alive: true,
    process_busy: tickets.some((item) => item.claim_expires_at !== null),
    availability: 'live',
    lease_expires_at: Date.now() / 1000 + 3600,
    tickets,
    planned,
  };
}

/** Synthetic approved-shape specimen, also composed inside the real demo shell. */
export function DemoWorkerColumns({ layout = 'grid' }: { layout?: 'grid' | 'paged' }) {
  const originals = collectionTickets.value.slice(0, 4),
    rows = originals.map(demoTicket);
  const details = Object.fromEntries(rows.map((row) => [row.qualified_id, row]));
  const planned = rows[3]
    ? [
        {
          entry: {
            operation: 'demo-plan',
            worker: 'Codex B',
            generation: 'demo-generation',
            ticket: { connection_id: 'demo', native_id: rows[3].native_id },
            slug: rows[3].slug,
          },
          reasons: [],
        },
      ]
    : [];
  const view: WorkerView = {
    cursor: 'demo',
    project_id: 'demo',
    source_id: 'demo',
    uncovered_sources: [],
    connection_coverage: true,
    workers: [
      worker('Integrator', 'integrator', rows[0] ? [assignment(rows[0], 'integrating', true)] : [], []),
      worker('Codex B', 'worker', rows[1] ? [assignment(rows[1], 'accepted', true)] : [], planned),
    ],
  };
  return (
    <WorkerColumns
      views={[view]}
      details={details}
      ticketRow={(row) => originals.find((item) => item.slug === row.slug)!}
      loading={false}
      sourceErrors={[]}
      unsupportedSources={[]}
      layout={layout}
    />
  );
}
