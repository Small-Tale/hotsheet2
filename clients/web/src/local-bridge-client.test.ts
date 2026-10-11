import { describe, expect, it, vi } from 'vitest';

import {
  bridgeErrorMessage,
  localBridge,
  LocalBridgeHttpError,
  LocalBridgeResponseError,
  parseCheckouts,
  parseConversationExportDestination,
  parseConversationExportOpen,
  parseConversationExportWrite,
  parseFolderChoice,
  parseOpenedProject,
  parseRemovedHs1Data,
} from './local-bridge-client';

const respond = (status: number, body: string) => vi.fn(async () => new Response(body, { status }));
const manifest = {
  format: 'hotsheet-conversation-export',
  exportId: 'e',
  revision: 3,
  source: {},
  reopen: {},
  selectedMessageIds: [],
  entries: [],
  assets: [],
};

describe('local bridge adapter (HS2-313JET)', () => {
  it('sends JSON bodies and returns parsed responses', async () => {
    const request = respond(200, JSON.stringify({ ticketStore: '/t', connectionId: 'git-1' }));
    await expect(localBridge.setupGit('/root', undefined, request)).resolves.toEqual({
      ticketStore: '/t',
      connectionId: 'git-1',
      error: undefined,
    });
    expect(request).toHaveBeenCalledWith('/__hotsheet/projects/setup-git', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ root: '/root' }),
    });
  });

  it('surfaces the server error, else the fallback, for a failed status', async () => {
    await expect(localBridge.removeHs1Data('p', respond(409, '{"error":"busy"}'))).rejects.toMatchObject({
      name: 'LocalBridgeHttpError',
      message: 'busy',
      status: 409,
    });
    await expect(localBridge.checkouts(undefined, respond(500, ''))).rejects.toBeInstanceOf(LocalBridgeHttpError);
    await expect(localBridge.checkouts(undefined, respond(500, '{}'))).rejects.toThrow(
      'Could not find registered Hot Sheet projects.',
    );
  });

  it('rejects non-JSON and wrongly shaped bodies instead of casting them', async () => {
    await expect(localBridge.chooseFolder(undefined, respond(200, '<html>'))).rejects.toThrow(
      'The folder chooser returned an invalid response (200).',
    );
    await expect(localBridge.openConversationExport(respond(502, 'Bad gateway'))).rejects.toBeInstanceOf(
      LocalBridgeResponseError,
    );
    await expect(localBridge.checkouts(undefined, respond(200, '{"root":"/x"}'))).rejects.toMatchObject({
      status: 200,
      message: 'The local bridge returned an invalid checkout list response.',
    });
    await expect(localBridge.chooseFolder(undefined, respond(200, ''))).resolves.toEqual({ path: undefined });
  });

  it('validates each response shape', () => {
    expect(parseFolderChoice({ path: '/a' })).toEqual({ path: '/a' });
    expect(() => parseFolderChoice({ path: 3 })).toThrow(LocalBridgeResponseError);
    expect(() => parseFolderChoice([])).toThrow(LocalBridgeResponseError);
    const checkout = { id: 'c', root: '/r', alias: 'r', stores: ['/s'] };
    expect(parseCheckouts([checkout])).toEqual([checkout]);
    expect(() => parseCheckouts([{ ...checkout, stores: [1] }])).toThrow(LocalBridgeResponseError);
    const project = { id: 'p', root: '/r', name: 'r', apiPath: '/api', stores: [] };
    expect(parseOpenedProject(project)).toBe(project);
    expect(() => parseOpenedProject({ ...project, apiPath: 1 })).toThrow(LocalBridgeResponseError);
    expect(parseRemovedHs1Data({})).toEqual({ removed: [] });
    expect(() => parseRemovedHs1Data({ removed: 'x' })).toThrow(LocalBridgeResponseError);
    expect(parseConversationExportDestination({})).toEqual({});
    const destination = { selectionToken: 't', displayPath: '/d', kind: 'archive' };
    expect(parseConversationExportDestination({ destination })).toEqual({ destination });
    expect(() => parseConversationExportDestination({ destination: { ...destination, kind: 'zip' } })).toThrow(
      LocalBridgeResponseError,
    );
    expect(parseConversationExportWrite({ displayPath: '/d', manifest }).manifest.revision).toBe(3);
    expect(() => parseConversationExportWrite({ displayPath: '/d', manifest: { ...manifest, revision: '3' } })).toThrow(
      LocalBridgeResponseError,
    );
    expect(parseConversationExportOpen({ conversation: null })).toEqual({});
    expect(
      parseConversationExportOpen({ conversation: { displayPath: '/d', manifest, messages: [], activity: [] } })
        .conversation?.displayPath,
    ).toBe('/d');
    expect(() => parseConversationExportOpen({ conversation: { displayPath: '/d', manifest } })).toThrow(
      LocalBridgeResponseError,
    );
    expect(bridgeErrorMessage({ error: '' })).toBeUndefined();
    expect(bridgeErrorMessage('x')).toBeUndefined();
  });
});
