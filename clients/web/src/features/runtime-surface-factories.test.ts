import { signal } from 'kerfjs';
import { describe, expect, it } from 'vitest';

import type { Capabilities, CheckoutTicketCounts, TicketRow as WireTicketRow } from '../api';
import type { TicketStatus } from '../components/status-badge';
import type { TicketPriority } from '../components/ticket-row';
import type { Project } from '../interactions/types';
import type { TicketView } from '../ticket-views';
import { createTerminalOperationsSurface } from './terminal-operations-surface';
import { createTicketViewSurface } from './ticket-view-surface';

const row = (slug: string, status: string) => ({ slug, status, connection_id: 'git' }) as WireTicketRow;

describe('runtime surface factories (HS2-K0VC2C)', () => {
  it('projects terminal operations from the selected rows and other projects cached rows', () => {
    const counts = { completed_today: 2, started: 1 } as CheckoutTicketCounts;
    const surface = createTerminalOperationsSurface({
      partialSourcesByProject: signal({ p2: true }),
      projects: signal([{ id: 'p1', name: 'One' } as Project, { id: 'p2', name: 'Two' } as Project]),
      projectTicketCounts: () => counts,
      selectedProjectId: signal('p1'),
      ticketRowsByProject: signal({}),
      tickets: signal([row('A', 'completed')]),
    });
    const { projects } = surface.terminalOperationsSurfaceProps();
    expect(projects.map((item) => [item.id, item.completedToday, item.inProgress, item.partial])).toEqual([
      ['p1', 2, 1, false],
      ['p2', 2, 1, true],
    ]);
  });

  it('titles views and picks the view action from live capabilities', () => {
    let staging = false;
    const surface = createTicketViewSurface({
      canStageNewTicketAttachments: () => staging,
      capabilitiesFor: () => ({ update: true }) as Capabilities,
      customViewFor: (view) => (view === 'custom:x' ? { id: 'x', name: 'Mine', query: {} } : undefined) as never,
      priority: (value) => (value ?? 'default') as TicketPriority,
      selectedTicketSlugs: signal([]),
      status: (value) => (value ?? 'not_started') as TicketStatus,
      ticketCloseDialog: signal(undefined),
      ticketContextMenu: signal(undefined),
      tickets: signal([]),
    });
    expect(
      (['archive', 'trash', 'backlog', 'errors', 'custom:x', 'all'] as TicketView[]).map((view) =>
        surface.ticketViewTitle(view),
      ),
    ).toEqual(['Archive', 'Trash', 'Backlog', 'Ticket errors', 'Mine', 'Queue']);
    expect(surface.ticketViewActionSpec('trash', true)).toEqual({ kind: 'empty-trash' });
    expect(surface.ticketViewActionSpec('all', false)).toBeUndefined();
    expect(surface.ticketViewActionSpec('all', true, 'New')).toMatchObject({
      kind: 'new-ticket',
      attachmentsEnabled: false,
      label: 'New',
    });
    staging = true;
    expect(surface.ticketViewActionSpec('all', true)).toMatchObject({ attachmentsEnabled: true });
  });
});
