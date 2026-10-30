import { describe, expect, it } from 'vitest';

import type { TicketRow as WireTicketRow } from '../api';
import type { WorkerView } from '../worker-view-model';
import type { TicketRowProps } from './ticket-row';
import { WorkerColumns } from './worker-columns';

const rows: WireTicketRow[] = ['one', 'two'].map((id) => ({
  connection_id: id,
  native_id: 'same',
  qualified_id: `${id}:same`,
  id: 'same',
  slug: 'HS2-SAME',
  title: `${id} ticket`,
  category: 'task',
  priority: 'normal',
  status: 'started',
  up_next: true,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
}));
const worker = (id: string, row: WireTicketRow) => ({
  id,
  role: 'worker' as const,
  generation: 'g1',
  parent: null,
  label: id,
  tool: 'Codex',
  connection_kind: 'terminal',
  connection_id: id,
  connection_available: true,
  observed_alive: true,
  process_busy: false,
  availability: 'live' as const,
  lease_expires_at: 1000,
  tickets: [
    {
      ticket: { connection_id: row.connection_id, native_id: row.native_id },
      slug: row.slug,
      assignment_id: id,
      assignment_phase: 'offered' as const,
      claim_expires_at: null,
      started_phase: null,
      eta_at: null,
    },
  ],
  planned: [],
});
const view: WorkerView = {
  cursor: 'c',
  project_id: 'p',
  source_id: 'one',
  connection_coverage: true,
  uncovered_sources: ['GitHub Issues'],
  workers: [worker('A', rows[0]), worker('B', rows[1])],
};
const row = (ticket: WireTicketRow): TicketRowProps => ({
  slug: ticket.slug,
  title: ticket.title,
  status: 'started',
  priority: 'default',
  category: 'task',
  tags: [],
});

describe('HS2-E9SR7R Workers composition', () => {
  it('uses the shared ticket summary with qualified card identity and honest source coverage', () => {
    const markup = String(
      WorkerColumns({
        views: [view],
        details: Object.fromEntries(rows.map((item) => [item.qualified_id, item])),
        ticketRow: row,
        selectedQualifiedId: 'two:same',
        loading: false,
        sourceErrors: [],
        unsupportedSources: [],
        layout: 'grid',
      }),
    );
    expect(markup).toContain('data-component="column-track"');
    expect(markup).toContain('Some Workers May Be Missing');
    expect(markup).toContain('data-worker-ticket="one:same"');
    expect(markup).toContain('data-worker-ticket="two:same"');
    expect(markup).toContain('data-ticket-qualified-id="two:same"');
    expect(markup).toContain('data-row-selected="true"');
    expect(markup.match(/data-component="ticket-list-row-container"/g)).toHaveLength(2);
  });

  it('pages the phone layout and keeps unknown separate from a genuine zero-worker state', () => {
    const base = { details: {}, ticketRow: row, sourceErrors: [], unsupportedSources: [], layout: 'paged' as const };
    const loading = String(WorkerColumns({ ...base, views: [], loading: true }));
    const empty = String(WorkerColumns({ ...base, views: [], loading: false }));
    expect(loading).toContain('Loading workers');
    expect(empty).toContain('No workers are registered');
    const unavailable = String(WorkerColumns({ ...base, views: [], loading: false, unsupportedSources: ['Jira'] }));
    expect(unavailable).toContain('Worker availability is unknown');
  });
});
