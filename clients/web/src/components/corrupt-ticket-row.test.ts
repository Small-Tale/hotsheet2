import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { corruptTicketIdentity,CorruptTicketRow } from './corrupt-ticket-row';

const corrupt = { store: 'local', store_path: '/project.hs2', path: '/project.hs2/tickets/01/01M1.md', id: '01M1', slug: 'HS2-BROKEN', error: 'unsupported content follows the bounded Notes section' };

describe('CorruptTicketRow', () => {
  it('shows recovered identity, path, and parse error without ticket capabilities', () => {
    const markup = String(CorruptTicketRow({ ticket: corrupt }));
    expect(markup).toContain('data-component="corrupt-ticket-row"');
    expect(markup).toContain('aria-disabled="true"');
    expect(markup).toContain('data-lucide="file-warning"');
    expect(markup).toContain('HS2-BROKEN');
    expect(markup).toContain('/project.hs2/tickets/01/01M1.md');
    expect(markup).toContain('unsupported content follows the bounded Notes section');
    expect(markup).not.toContain('data-action=');
  });

  it('falls back through id, filename, and a generic label', () => {
    expect(corruptTicketIdentity({ ...corrupt, slug: undefined })).toBe('01M1');
    expect(corruptTicketIdentity({ ...corrupt, slug: undefined, id: undefined })).toBe('01M1.md');
    expect(corruptTicketIdentity({ ...corrupt, slug: undefined, id: undefined, path: '' })).toBe('Unreadable ticket');
  });

  it('uses a visibly distinct disabled treatment', () => {
    const css = readFileSync(new URL('./corrupt-ticket-row.css', import.meta.url), 'utf8');
    expect(css).toMatch(/border-left: \.25rem solid/);
    expect(css).toContain('cursor: not-allowed');
  });
});
