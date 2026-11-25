import { Bug, ListChecks, Plus, Sparkles } from 'lucide';
import { describe, expect, it } from 'vitest';

import { AppShell } from './app-shell';
import { MinimizedTicketsControl, type MinimizedTicketSummary } from './minimized-tickets-control';

const reader = (
  id: string,
  slug: string,
  title: string,
  icon: MinimizedTicketSummary['icon'],
  draft?: string,
): MinimizedTicketSummary => ({
  id,
  kind: 'ticket-reader',
  label: `${slug} ${title}`,
  shortLabel: slug,
  icon,
  draft,
});

const tickets: MinimizedTicketSummary[] = [
  reader('frame-1', 'HS2-DEMO01', 'Keep reader drafts', { icon: Sparkles, name: 'sparkles' }, 'Unsent note'),
  reader('frame-2', 'HS2-START02', 'Retry a failed upload', { icon: Bug, name: 'bug' }, 'Reply in progress'),
  reader('frame-3', 'HS2-NEXT01', 'Show CI run duration', { icon: ListChecks, name: 'list-checks' }),
];
const composer: MinimizedTicketSummary = {
  id: 'new-ticket:project-a',
  kind: 'new-ticket',
  label: 'New ticket: Draft a release note',
  shortLabel: 'new ticket',
  icon: { icon: Plus, name: 'plus' },
  draft: 'Unsaved ticket',
};

describe('MinimizedTicketsControl (HS2-Q8WF64)', () => {
  it('renders nothing while no ticket is minimized', () => {
    expect(MinimizedTicketsControl({ tickets: [] })).toBeNull();
  });

  it('restores a single minimized ticket with one layers-2 button that names it and its draft', () => {
    const markup = String(MinimizedTicketsControl({ tickets: [tickets[0]] }));
    expect(markup).toContain('data-component="toolbar-control-group"');
    expect(markup).toContain('data-action="restore-minimized-ticket"');
    expect(markup).toContain('data-minimized-item-id="frame-1"');
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
      expect(markup).toContain(`data-minimized-item-id="${ticket.id}"`);
      expect(markup).toContain(ticket.label);
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

  it('restores a lone minimized composer by its own name (HS2-3JR0ZH)', () => {
    const markup = String(MinimizedTicketsControl({ tickets: [composer] }));
    expect(markup).toContain('data-minimized-item-id="new-ticket:project-a"');
    expect(markup).toContain('data-minimized-kind="new-ticket"');
    expect(markup).toContain('aria-label="Restore New ticket: Draft a release note (Unsaved ticket)"');
    expect(markup).toContain('title="Restore new ticket"');
  });

  it('lists a minimized composer beside minimized readers with its plus icon and draft (HS2-3JR0ZH)', () => {
    const markup = String(MinimizedTicketsControl({ tickets: [tickets[0], composer] }));
    expect(markup).toContain('2 Minimized Tickets');
    expect(markup).toContain('New ticket: Draft a release note');
    expect(markup).toContain('data-minimized-kind="new-ticket"');
    expect(markup).toContain('data-minimized-kind="ticket-reader"');
    expect(markup).toContain('data-lucide="plus"');
    expect(markup).toContain('>Unsaved ticket</span>');
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
