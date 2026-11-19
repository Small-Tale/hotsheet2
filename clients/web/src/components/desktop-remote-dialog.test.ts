import { existsSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import type { DesktopRemoteServer } from '../remote-projects/client';
import { DesktopRemoteDialog } from './desktop-remote-dialog';

/** Synthetic fixtures only (HS2-A5CKQS). */
const server = (id: string, label: string, failure: string | null = null): DesktopRemoteServer => ({
  id,
  label,
  origin: `http://${id}.example.test:4174`,
  identity: { server_id: `srv-${id}`, ca_fingerprint: null },
  revision: 2,
  revoked: false,
  last_failure: failure,
  stale: false,
});

const render = (props: Partial<Parameters<typeof DesktopRemoteDialog>[0]>) =>
  String(DesktopRemoteDialog({ open: true, servers: [], formOpen: false, busy: false, error: '', ...props }));

describe('DesktopRemoteDialog (HS2-A5CKQS)', () => {
  it('lists approved servers as Kerf action rows with Edit and Remove in an overflow menu', () => {
    const markup = render({ servers: [server('studio', 'Studio'), server('build', 'Build', 'No answer')] });
    expect(markup).toContain('Approved Servers');
    expect(markup.match(/data-component="list-action-row"/g)).toHaveLength(2);
    expect(markup).toContain('data-remote-id="studio"');
    expect(markup).toContain('data-revision="2"');
    expect(markup).toContain('data-action="discover-desktop-remote-server"');
    expect(markup).toContain('http://studio.example.test:4174');
    expect(markup).toContain('aria-label="Actions for Studio"');
    expect(markup).toContain('aria-label="Retry Build"');
    // Edit and Remove are menu items, not inline row buttons.
    expect(markup).not.toContain('aria-label="Edit Studio"');
    expect(markup).not.toContain('aria-label="Remove Studio"');
    expect(markup).not.toContain('desktop-remote-dialog__server');
  });

  it('hides revoked servers', () => {
    const markup = render({ servers: [{ ...server('gone', 'Gone'), revoked: true }] });
    expect(markup).not.toContain('data-remote-id="gone"');
  });

  it('lists a server’s projects as rows under a heading that names it', () => {
    const remote = server('studio', 'Studio');
    const markup = render({
      servers: [remote],
      discovery: {
        remote,
        checkouts: [
          { id: 'open', name: 'Workspace', state: 'available' },
          { id: 'gone', name: 'Archive', state: 'inaccessible' },
          { id: 'moved', name: 'Moved', state: 'identity_changed' },
        ],
      },
    });
    expect(markup).toContain('Projects on Studio');
    expect(markup.match(/data-component="list-item"/g)).toHaveLength(3);
    expect(markup).toContain('data-checkout-id="open"');
    expect(markup).toContain('data-action="open-desktop-remote-checkout"');
    expect(markup).toContain('Unavailable');
    expect(markup).toContain('Identity Changed');
    expect(markup).toContain('aria-current');
  });

  it('says so when the server has no open projects', () => {
    const remote = server('studio', 'Studio');
    expect(render({ servers: [remote], discovery: { remote, checkouts: [] } })).toContain(
      'No available projects are open on this server.',
    );
  });

  it('keeps the advanced TLS fields in a closed Kerf Disclosure', () => {
    const markup = render({ formOpen: true });
    expect(markup).toContain('data-component="disclosure"');
    expect(markup).toContain('data-remote-tls-settings');
    expect(markup).toContain('Advanced TLS Settings');
    expect(markup).toContain('name="remote-ca-file"');
    expect(markup).not.toMatch(/<details\s+open/);
  });

  it('owns no stylesheet of its own', () => {
    expect(existsSync(resolve(import.meta.dirname, 'desktop-remote-dialog.css'))).toBe(false);
  });
});
