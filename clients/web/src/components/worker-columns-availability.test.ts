import { expect, it } from 'vitest';

import type { WorkerRow, WorkerView } from '../worker-view-model';
import { WorkerColumns } from './worker-columns';

const worker = (id: string): WorkerRow => ({
  id,
  role: 'worker',
  generation: 'synthetic',
  parent: null,
  label: id,
  tool: 'Codex',
  connection_kind: 'terminal',
  connection_id: id,
  connection_available: true,
  observed_alive: true,
  process_busy: false,
  availability: 'live',
  lease_expires_at: 1_000,
  tickets: [],
  planned: [],
});

const view = (source_id: string): WorkerView => ({
  cursor: 'same-cursor',
  project_id: 'synthetic-project',
  source_id,
  workers: [worker(source_id)],
  uncovered_sources: [],
  connection_coverage: true,
});

const ticketRow = () => ({
  slug: 'HS2-SYNTHETIC',
  title: 'synthetic ticket',
  status: 'started' as const,
  priority: 'default' as const,
  category: 'task' as const,
  tags: [],
});

it('HS2-KX87JC suppresses a failed source snapshot without hiding a healthy source', () => {
  const render = (failed: boolean) =>
    String(
      WorkerColumns({
        views: [view('failed-source'), view('healthy-source')],
        details: {},
        ticketRow,
        loading: false,
        sourceErrors: failed
          ? [{ connectionId: 'failed-source', name: 'Failed source', message: 'synthetic 503' }]
          : [],
        sources: [
          { connectionId: 'failed-source', name: 'Failed source', provider: 'git' },
          { connectionId: 'healthy-source', name: 'Healthy source', provider: 'git' },
        ],
        layout: 'grid',
      }),
    );

  const available = render(false);
  expect(available).toContain('data-worker-column="failed-source"');
  expect(available).toContain('data-worker-column="healthy-source"');

  const failed = render(true);
  expect(failed).toContain('Workers Not Shown for Some Sources');
  expect(failed).not.toContain('data-worker-column="failed-source"');
  expect(failed).toContain('data-worker-column="healthy-source"');
  expect(failed).toContain('Available');
});

it('HS2-KX87JC does not call missing, disconnected, or unconfirmed workers available', () => {
  const render = (row: WorkerRow) =>
    String(
      WorkerColumns({
        views: [{ ...view('synthetic-source'), workers: [row] }],
        details: {},
        ticketRow,
        loading: false,
        sourceErrors: [],
        sources: [{ connectionId: 'synthetic-source', name: 'Synthetic source', provider: 'git' }],
        layout: 'grid',
      }),
    );
  const base = worker('synthetic-worker');
  expect(render(base)).toContain('Available');
  expect(render({ ...base, process_busy: true })).toContain('Working');
  expect(render({ ...base, availability: 'disconnected' })).toContain('Disconnected');
  expect(render({ ...base, availability: 'unresolved_claim_holder' })).toContain('Unresolved claim holder');
  expect(render({ ...base, availability: 'unbound_connection' })).toContain('Connection unbound');
  for (const availability of ['disconnected', 'unresolved_claim_holder', 'unbound_connection'] as const)
    expect(render({ ...base, availability })).not.toContain('>Available<');
});
