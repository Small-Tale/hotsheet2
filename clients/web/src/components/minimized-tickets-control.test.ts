import { describe, expect, it } from 'vitest';

import { AppShell } from './app-shell';
import { MinimizedTicketsControl, type MinimizedTicketSummary } from './minimized-tickets-control';

const tickets: MinimizedTicketSummary[] = [
  { id: 'frame-1', slug: 'HS2-DEMO01', title: 'Keep reader drafts', category: 'feature', draft: 'Unsent note' },
  { id: 'frame-2', slug: 'HS2-START02', title: 'Retry a failed upload', category: 'bug', draft: 'Reply in progress' },
  { id: 'frame-3', slug: 'HS2-NEXT01', title: 'Show CI run duration', category: 'task' },
];

describe('MinimizedTicketsControl (HS2-Q8WF64)', () => {
  it('renders nothing while no ticket is minimized', () => {
    expect(MinimizedTicketsControl({ tickets: [] })).toBeNull();
  });

  it('restores a single minimized ticket with one layers-2 button that names it and its draft', () => {
    const markup = String(MinimizedTicketsControl({ tickets: [tickets[0]] }));
    expect(markup).toContain('data-component="toolbar-control-group"');
    expect(markup).toContain('data-action="restore-minimized-ticket"');
    expect(markup).toContain('data-minimized-reader-id="frame-1"');
    expect(markup).toContain('data-minimized-tickets-control="single"');
    expect(markup).toContain('aria-label="Restore HS2-DEMO01 Keep reader drafts (Unsent note)"');
    expect(markup).toContain('data-lucide="layers-2"');
    expect(markup).not.toContain('square-stack');
    expect(markup).not.toContain('data-placement="corner"');
    expect(markup).not.toContain('role="menu"');
  });

  it('lists several minimized tickets in a menu with a corner count, draft badges, Restore All, and Close All…', () => {
    const markup = String(MinimizedTicketsControl({ tickets }));
    expect(markup).toContain('data-minimized-tickets-control="menu"');
    expect(markup).toContain('3 Minimized Tickets');
    expect(markup).toContain('data-placement="corner" aria-hidden="true">3</span>');
    expect(markup).toContain('Minimized Tickets');
    for (const ticket of tickets) {
      expect(markup).toContain(`data-minimized-reader-id="${ticket.id}"`);
      expect(markup).toContain(`${ticket.slug} ${ticket.title}`);
    }
    expect(markup.match(/data-action="restore-minimized-ticket"/g)).toHaveLength(3);
    expect(markup).toContain('>Unsent note</span>');
    expect(markup).toContain('>Reply in progress</span>');
    expect(markup).toContain('data-action="restore-all-minimized-tickets"');
    expect(markup).toContain('data-action="close-all-minimized-tickets"');
    expect(markup).toContain('Close All…');
    // Category icons lead each ticket row.
    expect(markup).toContain('data-lucide="sparkles"');
    expect(markup).toContain('data-lucide="bug"');
    expect(markup).toContain('data-lucide="list-checks"');
  });

  it('drops the ellipsis from Close All when no draft would be lost', () => {
    const markup = String(
      MinimizedTicketsControl({ tickets: tickets.map((ticket) => ({ ...ticket, draft: undefined })) }),
    );
    expect(markup).toContain('>Close All<');
    expect(markup).not.toContain('Close All…');
  });
});

describe('AppShell minimized tickets placement (HS2-Q8WF64)', () => {
  const control = MinimizedTicketsControl({ tickets: [tickets[0]] }) ?? undefined;
  const shell = (props: Partial<Parameters<typeof AppShell>[0]>) =>
    String(
      AppShell({
        tabs: [],
        header: 'head' as never,
        workspace: 'work' as never,
        terminalDrawer: 'drawer' as never,
        minimizedTickets: control,
        ...props,
      }),
    );

  it('joins the terminal drawer restore corner, before the drawer button, while the drawer is collapsed', () => {
    const markup = shell({ terminalDrawerVisible: false });
    const corner = markup.indexOf('aria-label="Terminal Drawer Controls"');
    expect(corner).toBeGreaterThan(-1);
    expect(markup.indexOf('data-minimized-tickets-control')).toBeGreaterThan(corner);
    expect(markup.indexOf('data-minimized-tickets-control')).toBeLessThan(markup.indexOf('Show Terminal Drawer'));
    expect(markup).not.toContain('role="toolbar" aria-label="Minimized Tickets"');
  });

  it('floats alone at the work area corner while the drawer is open or absent, and hides while it animates', () => {
    for (const markup of [shell({ terminalDrawerVisible: true }), shell({ terminalDrawer: undefined })]) {
      expect(markup).toContain(
        'data-position="bottom-end" data-placement="floating" role="toolbar" aria-label="Minimized Tickets"',
      );
      expect(markup.match(/data-minimized-tickets-control/g)).toHaveLength(1);
      expect(markup.indexOf('data-minimized-tickets-control')).toBeLessThan(markup.indexOf('</main>'));
    }
    expect(shell({ terminalDrawerVisible: true, terminalDrawerTransitioning: true })).not.toContain(
      'data-minimized-tickets-control',
    );
  });

  it('adds nothing when no ticket is minimized', () => {
    const markup = String(
      AppShell({ tabs: [], header: 'head' as never, workspace: 'work' as never, terminalDrawerVisible: true }),
    );
    expect(markup).not.toContain('Minimized Tickets');
  });
});
