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
    const css = readFileSync(new URL('./corrupt-ticket-row.css', import.meta.url), 'utf8'),
      shell = readFileSync(new URL('./ticket-row-shell.css', import.meta.url), 'utf8');
    // The row composes the shared ticket row shell in its danger tone, never TicketRow's block
    // classes (HS2-QSR1TG, HS2-1FH8BG).
    expect(markup).toContain('class="ticket-row-shell corrupt-ticket-row" data-component="corrupt-ticket-row"');
    expect(markup).toContain('data-selected="true"');
    expect(markup).toContain('data-tone="danger"');
    expect(markup).not.toContain('ticket-list-row');
    expect(css).not.toMatch(/\.ticket-list-row/);
    // The shell, not this stylesheet, owns the surface, selection, edges, and danger rail.
    expect(css).not.toMatch(/data-selected|data-list-edge|::before|:hover|border-bottom-color/);
    expect(shell).toContainSource(
      ".ticket-row-shell[data-selected='true'] { border-color:var(--wa-color-brand-border-normal); background:var(--wa-color-brand-fill-quiet); box-shadow:inset 0 0 0 1px color-mix(in srgb, var(--wa-color-brand-fill-loud) 12%, transparent); }",
    );
    expect(shell).toContainSource(".ticket-row-shell[data-list-edge='only'] { border-radius:remify(10.4px); }");
    expect(shell).toMatch(
      /ticket-row-shell\[data-tone='danger'\]::before[^}]*width: remify\(4px\)[^}]*background: var\(--wa-color-danger-fill-loud\)/,
    );
    expect(css).not.toContain('border-left-width');
    // The warning icon has its own column beside the identity and message (HS2-SJVM8C), and the select
    // button sizes to that stack instead of Web Awesome's native form-control height.
    expect(css).toMatch(
      /\.corrupt-ticket-row \{[^}]*display: grid;\s*grid-template-columns: remify\(32px\) minmax\(0, 1fr\);\s*align-items: start;/,
    );
    expect(css).toMatch(/\.corrupt-ticket-row__select \{[^}]*height: auto;/);
    expect(css).toContain('cursor: pointer');
    expect(revealFileLabel('MacIntel')).toBe('Reveal in Finder');
    expect(revealFileLabel('Win32')).toBe('Show in File Explorer');
    expect(revealFileLabel('Linux x86_64')).toBe('Show file location');
  });

  it('wiggles newly selected danger rows and honors reduced motion', () => {
    const shell = readFileSync(new URL('./ticket-row-shell.css', import.meta.url), 'utf8');
    expect(shell).toMatch(
      /\.ticket-row-shell\[data-tone='danger'\]\[data-selected='true'\] \{[^}]*animation: ticket-row-danger-selected-wiggle 150ms ease-out;/,
    );
    expect(shell).toContainSource('45% { transform:translateX(remify(5.6px)); }');
    expect(shell).toContainSource(
      "@media (prefers-reduced-motion: reduce) { .ticket-row-shell[data-tone='danger'][data-selected='true'] { animation:none; } }",
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
