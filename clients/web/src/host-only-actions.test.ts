import { afterEach, describe, expect, it, vi } from 'vitest';

import { Api, type CodeReviewTarget, revealCorruptTicketFile } from './api';
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
