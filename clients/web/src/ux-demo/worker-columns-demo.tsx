import type { TicketRow as WireTicketRow } from '../api';
import type { TicketRowProps } from '../components/ticket-row';
import { WorkerColumns } from '../components/worker-columns';
import type { WorkerCoverageSource, WorkerRow, WorkerTicket, WorkerView } from '../worker-view-model';
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
    integration_strategy: role === 'integrator' ? 'serial' : null,
  };
}

/** HS2-7WE7CQ: a GitHub source the server names by id and the client by name, plus real gaps. */
const DEMO_SOURCES: WorkerCoverageSource[] = [
  { connectionId: 'demo', name: 'Demo git tickets', provider: 'git' },
  { connectionId: 'github-small-tale-hotsheet2', name: 'GitHub Small-Tale/hotsheet2', provider: 'github' },
  { connectionId: 'git-archive', name: 'Archive git tickets', provider: 'git' },
  { connectionId: 'git-shared', name: 'Shared git tickets', provider: 'git' },
];
const DEMO_COVERAGE_GAPS: NonNullable<WorkerView['coverage_gaps']> = [
  {
    connection_id: 'github-small-tale-hotsheet2',
    name: 'GitHub Small-Tale/hotsheet2',
    provider: 'github',
    reason: 'unsupported',
    details: [],
  },
  { connection_id: 'git-archive', name: 'Archive git tickets', provider: 'git', reason: 'failed_read', details: [] },
  { connection_id: 'git-shared', name: 'Shared git tickets', provider: 'git', reason: 'unobserved', details: [] },
];

/** Synthetic approved-shape specimen, also composed inside the real demo shell. */
export function DemoWorkerColumns({
  layout = 'grid',
  coverage = 'complete',
}: {
  layout?: 'grid' | 'paged';
  coverage?: 'complete' | 'gaps';
}) {
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
    uncovered_sources: coverage === 'gaps' ? DEMO_COVERAGE_GAPS.map((gap) => gap.connection_id) : [],
    ...(coverage === 'gaps' ? { coverage_gaps: DEMO_COVERAGE_GAPS } : {}),
    connection_coverage: true,
    workers: [
      worker('Integrator', 'integrator', rows[0] ? [assignment(rows[0], 'integrating', true)] : [], []),
      worker('Codex B', 'worker', rows[1] ? [assignment(rows[1], 'accepted', true)] : [], planned),
      {
        // HS2-6WFCZR: a strict worker idling with its reason and a refused request.
        ...worker('Claude C', 'worker', [], []),
        preferences: {
          prefer: ['category:bug'],
          only: ['tag:glance'],
          avoid: ['tier:T2'],
          request: 'HS2-DEMO1',
          refusal: { slug: 'HS2-DEMO1', reason: 'overlap', detail: 'scope docs/README.md is in flight' },
        },
        matching: {
          candidates: [],
          idle_reason: 'no ready tickets match only tag:glance, avoiding tier:T2',
          request: { slug: 'HS2-DEMO1', reasons: [] },
        },
      },
    ],
  };
  return (
    <WorkerColumns
      views={[view]}
      details={details}
      ticketRow={(row) => originals.find((item) => item.slug === row.slug)!}
      loading={false}
      sourceErrors={[]}
      sources={coverage === 'gaps' ? DEMO_SOURCES : DEMO_SOURCES.slice(0, 1)}
      layout={layout}
    />
  );
}
