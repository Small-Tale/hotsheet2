import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { corruptTicketIdentity, CorruptTicketInspector, CorruptTicketRow, revealFileLabel } from './corrupt-ticket-row';

const corrupt = {
  store: 'local',
  store_path: '/project.hs2',
  path: '/project.hs2/tickets/01/01M1.md',
  id: '01M1',
  slug: 'HS2-BROKEN',
  error: 'unsupported content follows the bounded Notes section',
};

describe('CorruptTicketRow', () => {
  it('shows recovered identity, diagnostics, and both recovery actions', () => {
    const markup = String(CorruptTicketRow({ ticket: corrupt }));
    expect(markup).toContain('data-component="corrupt-ticket-row"');
    expect(markup).toContain('role="group"');
    expect(markup).toContain('data-lucide="file-warning"');
    expect(markup).toContain('HS2-BROKEN');
    expect(markup).toContain('data-corrupt-key="local:/project.hs2/tickets/01/01M1.md"');
    expect(markup).not.toContain('unsupported content follows the bounded Notes section');
    expect(markup).toContain('data-action="select-corrupt-ticket"');
    expect(markup).not.toContain('data-action="reveal-corrupt-ticket"');
    expect(markup).not.toContain('data-action="repair-corrupt-ticket"');
  });

  it('presents the full error and recovery actions in an inspector', () => {
    const markup = String(CorruptTicketInspector({ ticket: corrupt, collapseControl: true }));
    expect(markup).toContain('data-component="corrupt-ticket-inspector"');
    expect(markup).toContain('Ticket parsing error');
    expect(markup).toContain('unsupported content follows the bounded Notes section');
    expect(markup).toContain('data-action="reveal-corrupt-ticket"');
    expect(markup).toContain('Attempt AI repair');
    expect(markup).toContain('data-action="toggle-ticket-inspector"');
    expect(markup).toContain('aria-label="Hide ticket inspector"');
    expect(markup).toContain('data-component="corrupt-ticket-inspector-header"');
  });

  it('falls back through id, filename, and a generic label', () => {
    expect(corruptTicketIdentity({ ...corrupt, slug: undefined })).toBe('01M1');
    expect(corruptTicketIdentity({ ...corrupt, slug: undefined, id: undefined })).toBe('01M1.md');
    expect(corruptTicketIdentity({ ...corrupt, slug: undefined, id: undefined, path: '' })).toBe('Unreadable ticket');
  });

  it('presents a newer ticket as upgrade-required rather than corrupt', () => {
    const markup = String(
      CorruptTicketRow({
        ticket: {
          ...corrupt,
          error_code: 'upgrade_required',
          error: 'This ticket was created by a newer version of Hot Sheet 2. Update Hot Sheet 2 to open it.',
        },
      }),
    );
    expect(markup).toContain('Hot Sheet 2 update required');
    expect(markup).toContain('data-lucide="refresh-cw"');
    expect(markup).not.toContain('Ticket file could not be read');
    expect(markup).toContain('data-action="select-corrupt-ticket"');
    expect(markup).not.toContain('data-action="repair-corrupt-ticket"');
  });

  it('uses a visibly distinct actionable treatment and platform labels', () => {
    const markup = String(CorruptTicketRow({ ticket: corrupt, selected: true }));
    const css = readFileSync(new URL('./corrupt-ticket-row.css', import.meta.url), 'utf8');
    // The row owns its list-row shell; it never borrows TicketRow's block classes (HS2-QSR1TG).
    expect(markup).toContain('class="corrupt-ticket-row" data-component="corrupt-ticket-row"');
    expect(markup).toContain('data-selected="true"');
    expect(markup).not.toContain('ticket-list-row');
    expect(css).not.toMatch(/\.ticket-list-row/);
    expect(css).toContainSource(
      ".corrupt-ticket-row[data-selected='true'] { border-color:var(--wa-color-brand-border-normal); background:var(--wa-color-brand-fill-quiet); box-shadow:inset 0 0 0 1px color-mix(in srgb, var(--wa-color-brand-fill-loud) 12%, transparent); animation:corrupt-ticket-selected-wiggle 150ms ease-out; }",
    );
    expect(css).toContainSource(".corrupt-ticket-row[data-list-edge='only'] { border-radius:remify(10.4px); }");
    expect(css).toMatch(
      /corrupt-ticket-row::before[^}]*width: remify\(4px\)[^}]*background: var\(--wa-color-danger-fill-loud\)/,
    );
    expect(css).not.toContain('border-left-width');
    expect(css).toContain('cursor: pointer');
    expect(revealFileLabel('MacIntel')).toBe('Reveal in Finder');
    expect(revealFileLabel('Win32')).toBe('Show in File Explorer');
    expect(revealFileLabel('Linux x86_64')).toBe('Show file location');
  });

  it('wiggles newly selected corrupt content and honors reduced motion', () => {
    const css = readFileSync(new URL('./corrupt-ticket-row.css', import.meta.url), 'utf8');
    expect(css).toMatch(
      /\.corrupt-ticket-row\[data-selected='true'\] \{[^}]*animation: corrupt-ticket-selected-wiggle 150ms ease-out;/,
    );
    expect(css).toContainSource('45% { transform:translateX(remify(5.6px)); }');
    expect(css).toContainSource(
      "@media (prefers-reduced-motion: reduce) { .corrupt-ticket-row[data-selected='true'] { animation:none; } }",
    );
  });

  it('reports pending and completed recovery state accessibly', () => {
    const pending = String(CorruptTicketInspector({ ticket: corrupt, recovery: { pending: 'repair' } }));
    expect(pending).toContain('Queuing…');
    expect(pending.match(/disabled/g)).toHaveLength(2);
    const completed = String(
      CorruptTicketInspector({ ticket: corrupt, recovery: { message: 'Queued HS2-REPAIR for AI repair.' } }),
    );
    expect(completed).toContain('role="status"');
    expect(completed).toContain('Queued HS2-REPAIR for AI repair.');
  });
});
