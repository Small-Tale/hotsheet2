import { describe, expect, it } from 'vitest';

import { TicketBlockers } from './ticket-blockers';

const rows = [
  { nativeId: 'a', reference: '@alpha/git-alpha:a', slug: 'HS2-A', title: 'Open blocker', state: 'open' as const },
  { nativeId: 'b', reference: '@alpha/git-alpha:b', slug: 'HS2-B', title: 'Done blocker', state: 'done' as const },
  { nativeId: 'gone', reference: '@alpha/git-alpha:gone', state: 'unavailable' as const },
];

describe('TicketBlockers (HS2-CHH28E)', () => {
  it('lists each blocker with an open action, a Done status, and a named Remove button', () => {
    const markup = String(TicketBlockers({ slug: 'HS2-SOURCE', rows, availability: { enabled: true } }));
    expect(markup).toContain('aria-label="Blocked By"');
    expect(markup).toContain('Add Blocker…');
    expect(markup).toContain('data-action="open-blocker-dialog"');
    expect(markup).toContain('3 blockers');
    // The primary action opens the qualified reference through the shared duplicate-target route.
    expect(markup).toContain('data-action="open-duplicate-target"');
    expect(markup).toContain('data-item-id="@alpha/git-alpha:a"');
    expect(markup).toContain('Open blocker HS2-B: Done blocker (done)');
    expect(markup).toContain('>Done<');
    expect(markup).toContain('Ticket unavailable');
    expect(markup).toContain('aria-label="Remove HS2-A as a blocker of HS2-SOURCE"');
    expect(markup).toContain('aria-label="Remove gone as a blocker of HS2-SOURCE"');
    expect(markup.match(/data-action="remove-blocker"/g)).toHaveLength(3);
    expect(markup).toContain('data-blocker-id="a"');
  });

  it('keeps an empty section with its action and no count', () => {
    const markup = String(TicketBlockers({ slug: 'HS2-SOURCE', rows: [], availability: { enabled: true } }));
    expect(markup).toContain('Add Blocker…');
    expect(markup).not.toContain('blockers</');
    expect(markup).not.toContain('data-action="remove-blocker"');
  });

  it('keeps the action mounted but disabled with the reason when the source lacks dependencies', () => {
    const reason = 'GitHub Issues tickets cannot record blocking dependencies.';
    const markup = String(
      TicketBlockers({
        slug: 'GH-1',
        rows: rows.slice(0, 1),
        availability: { enabled: false, disabledReason: reason },
      }),
    );
    expect(markup).toContain('data-action="open-blocker-dialog"');
    expect(markup).toContain(reason);
    expect(markup).toMatch(/<button(?=[^>]*data-action="open-blocker-dialog")(?=[^>]*disabled)[^>]*>/);
    expect(markup).toMatch(/<button(?=[^>]*data-action="remove-blocker")(?=[^>]*disabled)[^>]*>/);
  });

  it('disables only the row whose removal is in flight', () => {
    const markup = String(
      TicketBlockers({ slug: 'HS2-SOURCE', rows: rows.slice(0, 2), availability: { enabled: true }, removing: ['a'] }),
    );
    const removeButtons = markup.match(/<button[^>]*data-action="remove-blocker"[^>]*>/g) ?? [];
    expect(removeButtons).toHaveLength(2);
    expect(removeButtons.filter((button) => / disabled/.test(button))).toHaveLength(1);
    expect(removeButtons.find((button) => / disabled/.test(button))).toContain('data-blocker-id="a"');
  });
});
