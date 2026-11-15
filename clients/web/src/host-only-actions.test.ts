import { afterEach, describe, expect, it, vi } from 'vitest';

import { Api, type CodeReviewTarget, type ProviderConnection, revealCorruptTicketFile } from './api';
import { assertHostAction, HostOnlyActionError, hostOnlyActionMessage } from './host-only-actions';
import { localBridge } from './local-bridge-client';

const ok = () => vi.fn(async () => new Response('{"path":"/host/file"}', { status: 200 }));

function asDevice(hostname: string, search = '') {
  vi.stubGlobal('window', { location: { hostname, search } });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('host-only actions (HS2-F78B1S)', () => {
  it('refuses only remote clients, with an explanation', () => {
    expect(() => {
      assertHostAction('diff-tool', false);
    }).not.toThrow();
    expect(() => {
      assertHostAction('diff-tool', true);
    }).toThrow(HostOnlyActionError);
    expect(hostOnlyActionMessage('choose-folder')).toBe(
      "The folder chooser opens on the computer running Hot Sheet, so it isn't available from this device.",
    );
  });

  it('treats window-free contexts and loopback origins as the host', () => {
    expect(() => {
      assertHostAction('open-file');
    }).not.toThrow();
    asDevice('localhost');
    expect(() => {
      assertHostAction('open-file');
    }).not.toThrow();
    asDevice('localhost', '?device=remote');
    expect(() => {
      assertHostAction('open-file');
    }).toThrow(HostOnlyActionError);
  });

  it('rejects every host-launching API call on a remote client without a request', async () => {
    const fetchMock = ok();
    vi.stubGlobal('fetch', fetchMock);
    asDevice('phone.tailnet.ts.net');
    const api = new Api('/api');
    const target: CodeReviewTarget = { mode: 'worktree_file', paths: ['a'], area: 'staged' };
    const refused = [
      api.openCodeReview('c', 't', target),
      api.openRepositoryReview('c', target),
      api.repositoryFileAction('c', 'a', 'open'),
      api.repositoryFileAction('c', 'a', 'reveal'),
      api.checkoutAttachmentAction('c', 't', 'x', 'open'),
      api.checkoutAttachmentByNameAction('c', 't', 'f.png', 'reveal'),
      revealCorruptTicketFile('p', '/x.md'),
      localBridge.chooseFolder(),
      localBridge.chooseConversationExportDestination('chat'),
      localBridge.openConversationExport(),
    ];
    for (const call of refused) await expect(call).rejects.toBeInstanceOf(HostOnlyActionError);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('keeps the host path action available remotely and every action available locally', async () => {
    const fetchMock = ok();
    vi.stubGlobal('fetch', fetchMock);
    asDevice('phone.tailnet.ts.net');
    await expect(new Api('/api').checkoutAttachmentAction('c', 't', 'x', 'path')).resolves.toBeDefined();
    asDevice('127.0.0.1');
    await new Api('/api').repositoryFileAction('c', 'a', 'open');
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });
});

describe('Git source link and relink (HS2-CGXGWY)', () => {
  const git: ProviderConnection = {
    id: 'git-a',
    provider: 'git',
    locator: '/host/a.hs2',
    name: 'Git',
    default: false,
    settings: {},
  };
  const checkout = '{"id":"c","root":"/host/c","alias":"c","stores":["/host/a.hs2"]}';

  it('refuses linking a Git source by path and relinking it on a remote client without a request', async () => {
    const fetchMock = ok();
    vi.stubGlobal('fetch', fetchMock);
    asDevice('phone.tailnet.ts.net');
    const api = new Api('/api');
    await expect(api.addCheckoutSource('c', git)).rejects.toThrow(hostOnlyActionMessage('git-source-link'));
    await expect(api.relinkCheckoutGitSource('c', 'git-a', '/host/b.hs2')).rejects.toThrow(
      hostOnlyActionMessage('git-source-relink'),
    );
    expect(fetchMock).not.toHaveBeenCalled();
    expect(hostOnlyActionMessage('git-source-link')).toBe(
      "Linking a Git ticket repository happens on the computer running Hot Sheet, so it isn't available from this device.",
    );
    expect(hostOnlyActionMessage('git-source-relink')).toBe(
      "Changing a Git ticket repository location happens on the computer running Hot Sheet, so it isn't available from this device.",
    );
  });

  it('keeps provider source links remote and Git links local', async () => {
    const fetchMock = vi.fn(async () => new Response(checkout, { status: 200 }));
    vi.stubGlobal('fetch', fetchMock);
    asDevice('phone.tailnet.ts.net');
    const github: ProviderConnection = { ...git, id: 'gh', provider: 'github', locator: 'o/r' };
    await expect(new Api('/api').addCheckoutSource('c', github)).resolves.toMatchObject({ id: 'c' });
    asDevice('localhost');
    await expect(new Api('/api').addCheckoutSource('c', git)).resolves.toMatchObject({ id: 'c' });
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('shows a host_access_required refusal that still arrives as the same explanation', async () => {
    const refusal = () =>
      new Response('{"error":"host access required","code":"host_access_required"}', { status: 403 });
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => refusal()),
    );
    asDevice('localhost');
    const api = new Api('/api');
    const link = api.addCheckoutSource('c', git);
    await expect(link).rejects.toBeInstanceOf(HostOnlyActionError);
    await expect(link).rejects.toThrow(hostOnlyActionMessage('git-source-link'));
    await expect(api.relinkCheckoutGitSource('c', 'git-a', '/host/b.hs2')).rejects.toThrow(
      hostOnlyActionMessage('git-source-relink'),
    );
    // Other failures keep the server's own message.
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('{"error":"not a ticket store"}', { status: 400 })),
    );
    await expect(api.relinkCheckoutGitSource('c', 'git-a', '/host/b.hs2')).rejects.toThrow('not a ticket store');
  });
});
