import { mkdtemp, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it, vi } from 'vitest';

import { deleteDesktopDraftFile, loadDesktopDraftFile, saveDesktopDraftFile } from './desktop-draft-files';
import { DESKTOP_DRAFT_FILES_DIRECTORY, desktopStorageBridge } from './desktop-storage-bridge';
import { deleteDraftFiles, loadDraftFiles, saveDraftFile } from './workspace-session';

const roots: string[] = [];
afterEach(async () => {
  vi.unstubAllGlobals();
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
async function appData() {
  const root = await mkdtemp(join(tmpdir(), 'hs2-desktop-drafts-'));
  roots.push(root);
  return join(root, 'preferences');
}
let port = 40_000;
/** The real bridge behind a same-origin fetch, as a desktop page at one ephemeral port sees it. */
async function bridgeAt(existing?: string) {
  const directory = existing ?? (await appData()),
    app = desktopStorageBridge(directory),
    origin = `http://127.0.0.1:${String(++port)}`;
  const fetcher = vi.fn((input: string, init: RequestInit = {}) => {
    // A browser adds the page origin, and a File body's length, to each request it sends.
    const headers = new Headers(init.headers);
    headers.set('Origin', origin);
    if (init.body instanceof Blob) headers.set('Content-Length', String(init.body.size));
    return app.request(new URL(input, origin).href, { ...init, headers });
  });
  return { directory, fetcher: fetcher as unknown as typeof fetch };
}
const synthetic = (text: string, name = 'synthetic-proof.txt') =>
  new File([text], name, { type: 'text/plain', lastModified: 1_700_000_000_000 });

describe('desktop draft file client (HS2-791W19)', () => {
  it('round trips bytes, name, type, and modification time across a new origin', async () => {
    const first = await bridgeAt(),
      second = await bridgeAt(first.directory);
    await saveDesktopDraftFile(
      'composer:project-a:one',
      synthetic('persisted evidence', 'café 证据.txt'),
      first.fetcher,
    );
    const file = await loadDesktopDraftFile('composer:project-a:one', second.fetcher);
    expect(file?.name).toBe('café 证据.txt');
    expect(file?.type).toBe('text/plain');
    expect(file?.lastModified).toBe(1_700_000_000_000);
    expect(await file?.text()).toBe('persisted evidence');
    await deleteDesktopDraftFile('composer:project-a:one', second.fetcher);
    expect(await loadDesktopDraftFile('composer:project-a:one', first.fetcher)).toBeUndefined();
  });
  it('reports a missing file as absent and a failed request as an error', async () => {
    const { fetcher } = await bridgeAt();
    expect(await loadDesktopDraftFile('composer:project-a:missing', fetcher)).toBeUndefined();
    const failing = vi.fn(() => Promise.resolve(new Response(null, { status: 500 }))) as unknown as typeof fetch;
    await expect(loadDesktopDraftFile('composer:project-a:one', failing)).rejects.toThrow('Could not load');
    await expect(saveDesktopDraftFile('composer:project-a:one', synthetic('x'), failing)).rejects.toThrow(
      'Could not save',
    );
    await expect(deleteDesktopDraftFile('composer:project-a:one', failing)).rejects.toThrow('Could not remove');
  });
});

describe('draft file storage routing (HS2-791W19)', () => {
  it('keeps desktop drafts in the bridge with project-scoped recovery and cleanup', async () => {
    const first = await bridgeAt();
    vi.stubGlobal('window', { __HOTSHEET_DESKTOP_STORAGE__: true });
    vi.stubGlobal('indexedDB', {
      open: () => {
        throw new Error('desktop drafts must not use origin-scoped IndexedDB');
      },
    });
    vi.stubGlobal('fetch', first.fetcher);
    await saveDraftFile('composer:project-a', 'one', synthetic('first'));
    await saveDraftFile('composer:project-a', 'two', synthetic('second'));
    await saveDraftFile('not-working:project-a', 'one', synthetic('report'));
    // A relaunch serves the client from another ephemeral port with the same app data.
    vi.stubGlobal('fetch', (await bridgeAt(first.directory)).fetcher);
    const restored = await loadDraftFiles('composer:project-a', [
      { id: 'one', name: 'synthetic-proof.txt' },
      { id: 'two', name: 'synthetic-proof.txt' },
      { id: 'gone', name: 'lost.txt' },
    ]);
    expect(await Promise.all(restored.files.map(async (item) => [item.id, await item.file.text()]))).toEqual([
      ['one', 'first'],
      ['two', 'second'],
    ]);
    // An absent copy is gone, not a failure to retry.
    expect(restored.failed).toEqual([]);
    expect(await loadDraftFiles('composer:project-b', [{ id: 'one', name: 'synthetic-proof.txt' }])).toEqual({
      files: [],
      failed: [],
    });
    await deleteDraftFiles('composer:project-a', ['one', 'two']);
    expect((await loadDraftFiles('composer:project-a', [{ id: 'one', name: 'x' }])).files).toEqual([]);
    const remaining = await loadDraftFiles('not-working:project-a', [{ id: 'one', name: 'x' }]);
    expect(await remaining.files[0]?.file.text()).toBe('report');
    await deleteDraftFiles('not-working:project-a', ['one']);
    expect(await readdir(join(first.directory, DESKTOP_DRAFT_FILES_DIRECTORY))).toEqual([]);
  });
  it('reports one failed read as recoverable without losing the other file', async () => {
    const { fetcher } = await bridgeAt();
    vi.stubGlobal('window', { __HOTSHEET_DESKTOP_STORAGE__: true });
    vi.stubGlobal('fetch', fetcher);
    await saveDraftFile('composer:project-a', 'one', synthetic('first'));
    await saveDraftFile('composer:project-a', 'two', synthetic('second'));
    vi.stubGlobal('fetch', (input: string, init?: RequestInit) =>
      input.includes(encodeURIComponent('composer:project-a:two'))
        ? Promise.resolve(new Response(null, { status: 500 }))
        : fetcher(input, init),
    );
    const restored = await loadDraftFiles('composer:project-a', [
      { id: 'one', name: 'a.txt' },
      { id: 'two', name: 'b.txt' },
    ]);
    expect(restored.files.map((item) => item.id)).toEqual(['one']);
    expect(restored.failed).toEqual([{ id: 'two', name: 'b.txt' }]);
  });
  it('keeps ordinary browser drafts in origin-scoped IndexedDB', async () => {
    const fetcher = vi.fn(),
      open = vi.fn(() => {
        throw new Error('synthetic IndexedDB');
      });
    vi.stubGlobal('window', {});
    vi.stubGlobal('fetch', fetcher);
    vi.stubGlobal('indexedDB', { open });
    await expect(saveDraftFile('composer:project-a', 'one', synthetic('x'))).rejects.toThrow('synthetic IndexedDB');
    await expect(loadDraftFiles('composer:project-a', [{ id: 'one', name: 'x' }])).rejects.toThrow(
      'synthetic IndexedDB',
    );
    await expect(deleteDraftFiles('composer:project-a', ['one'])).rejects.toThrow('synthetic IndexedDB');
    expect(open).toHaveBeenCalledTimes(3);
    expect(fetcher).not.toHaveBeenCalled();
  });
});
