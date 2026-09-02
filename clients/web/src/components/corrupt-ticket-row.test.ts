import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { corruptTicketIdentity,CorruptTicketRow,revealFileLabel } from './corrupt-ticket-row';

const corrupt = { store: 'local', store_path: '/project.hs2', path: '/project.hs2/tickets/01/01M1.md', id: '01M1', slug: 'HS2-BROKEN', error: 'unsupported content follows the bounded Notes section' };

describe('CorruptTicketRow', () => {
  it('shows recovered identity, diagnostics, and both recovery actions', () => {
    const markup = String(CorruptTicketRow({ ticket: corrupt }));
    expect(markup).toContain('data-component="corrupt-ticket-row"');
    expect(markup).toContain('role="group"');
    expect(markup).toContain('data-lucide="file-warning"');
    expect(markup).toContain('HS2-BROKEN');
    expect(markup).toContain('/project.hs2/tickets/01/01M1.md');
    expect(markup).toContain('unsupported content follows the bounded Notes section');
    expect(markup).toContain('data-action="reveal-corrupt-ticket"');
    expect(markup).toContain('data-action="repair-corrupt-ticket"');
    expect(markup).toContain('data-lucide="folder-open"');
    expect(markup).toContain('data-lucide="bot"');
  });

  it('falls back through id, filename, and a generic label', () => {
    expect(corruptTicketIdentity({ ...corrupt, slug: undefined })).toBe('01M1');
    expect(corruptTicketIdentity({ ...corrupt, slug: undefined, id: undefined })).toBe('01M1.md');
    expect(corruptTicketIdentity({ ...corrupt, slug: undefined, id: undefined, path: '' })).toBe('Unreadable ticket');
  });

  it('presents a newer ticket as upgrade-required rather than corrupt', () => {
    const markup = String(CorruptTicketRow({ ticket: {
      ...corrupt,
      error_code: 'upgrade_required',
      error: 'This ticket was created by a newer version of Hot Sheet 2. Update Hot Sheet 2 to open it.',
    } }));
    expect(markup).toContain('Hot Sheet 2 update required');
    expect(markup).toContain('data-lucide="refresh-cw"');
    expect(markup).not.toContain('Ticket file could not be read');
    expect(markup).toContain('data-action="reveal-corrupt-ticket"');
    expect(markup).not.toContain('data-action="repair-corrupt-ticket"');
  });

  it('uses a visibly distinct actionable treatment and platform labels', () => {
    const css = readFileSync(new URL('./corrupt-ticket-row.css', import.meta.url), 'utf8');
    expect(css).toMatch(/border-left: \.25rem solid/);
    expect(css).toContain('cursor: pointer');
    expect(revealFileLabel('MacIntel')).toBe('Reveal in Finder');
    expect(revealFileLabel('Win32')).toBe('Show in File Explorer');
    expect(revealFileLabel('Linux x86_64')).toBe('Show file location');
  });

  it('reports pending and completed recovery state accessibly', () => {
    const pending=String(CorruptTicketRow({ticket:corrupt,recovery:{pending:'repair'}}));
    expect(pending).toContain('Queuing…');
    expect(pending.match(/disabled/g)).toHaveLength(2);
    const completed=String(CorruptTicketRow({ticket:corrupt,recovery:{message:'Queued HS2-REPAIR for AI repair.'}}));
    expect(completed).toContain('role="status"');
    expect(completed).toContain('Queued HS2-REPAIR for AI repair.');
  });
});
