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

  it('HS2-NEY1QH shows the integrator session strategy only on the integrator card', () => {
    const integrator = {
      ...worker('Integrator', rows[0]),
      role: 'integrator' as const,
      integration_strategy: 'serial',
    };
    const markup = String(
      WorkerColumns({
        views: [{ ...view, workers: [integrator, worker('B', rows[1])] }],
        details: {},
        ticketRow: row,
        loading: false,
        sourceErrors: [],
        unsupportedSources: [],
        layout: 'grid',
      }),
    );
    expect(markup.match(/data-integration-strategy="serial"/g)).toHaveLength(1);
    expect(markup).toContain('Serial integration');
    const older = String(
      WorkerColumns({
        views: [{ ...view, workers: [{ ...integrator, integration_strategy: null }] }],
        details: {},
        ticketRow: row,
        loading: false,
        sourceErrors: [],
        unsupportedSources: [],
        layout: 'grid',
      }),
    );
    expect(older).not.toContain('data-integration-strategy');
  });

  it('HS2-6WFCZR shows preference chips, the pending or refused request and the idle reason', () => {
    const picky = {
      ...worker('Picky', rows[0]),
      tickets: [],
      preferences: {
        prefer: ['category:bug'],
        only: ['tag:glance'],
        avoid: ['tier:T2'],
        request: 'HS2-WANT',
        refusal: null,
      },
      matching: {
        candidates: [],
        idle_reason: 'no ready tickets match only tag:glance',
        request: { slug: 'HS2-WANT', reasons: [] },
      },
    };
    const render = (workers: WorkerView['workers']) =>
      String(
        WorkerColumns({
          views: [{ ...view, workers }],
          details: {},
          ticketRow: row,
          loading: false,
          sourceErrors: [],
          unsupportedSources: [],
          layout: 'grid',
        }),
      );
    const markup = render([picky, worker('B', rows[1])]);
    expect(markup.match(/data-worker-preference=/g)).toHaveLength(3);
    expect(markup).toContain('data-worker-filter="tag:glance"');
    expect(markup).toContain('only: glance');
    expect(markup).toContain('avoid: T2');
    expect(markup).toContain('prefer: bug');
    expect(markup).toContain('Requested HS2-WANT');
    expect(markup).toContain('Idle: no ready tickets match only tag:glance');
    const refused = render([
      {
        ...picky,
        preferences: {
          ...picky.preferences,
          refusal: { slug: 'HS2-WANT', reason: 'not_ready', detail: 'not in Up Next' },
        },
      },
    ]);
    expect(refused).toContain('Request HS2-WANT refused: not ready (not in Up Next)');
    expect(refused).not.toContain('Requested HS2-WANT');
    // A disconnected worker shows no idle reason; a worker stating nothing shows no chips.
    const quiet = render([{ ...picky, availability: 'disconnected' }, worker('B', rows[1])]);
    expect(quiet).not.toContain('Idle:');
    expect(render([worker('B', rows[1])])).not.toContain('data-worker-preference');
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
