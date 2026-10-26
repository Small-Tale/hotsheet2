import { mkdtemp, readdir, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { desktopStorageBridge } from './desktop-storage-bridge';
import {
  DESKTOP_STORAGE_BODY_LIMIT,
  DESKTOP_STORAGE_PATH,
  DESKTOP_STORAGE_VALUE_LIMIT,
} from './desktop-storage-contract';

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
async function directory() {
  const root = await mkdtemp(join(tmpdir(), 'hs2-desktop-storage-'));
  roots.push(root);
  return join(root, 'preferences');
}
const url = `http://127.0.0.1:45678${DESKTOP_STORAGE_PATH}`;
let revision = 0;
const put = (key: string, value: string | null) => ({
  method: 'PUT',
  headers: { 'Content-Type': 'application/json', Origin: 'http://127.0.0.1:45678' },
  body: JSON.stringify({ key, value, clientId: 'synthetic-client', revision: ++revision }),
});
describe('private desktop storage bridge (HS2-RKZKX7)', () => {
  it('survives bridge recreation with private atomic files and tombstones', async () => {
    const root = await directory(),
      app = desktopStorageBridge(root);
    expect(await (await app.request(url)).json()).toEqual([]);
    expect((await app.request(url, put('hotsheet.open-projects', '["/synthetic/project"]'))).status).toBe(204);
    await app.request(url, put('hotsheet.layout', 'board'));
    await app.request(url, put('hotsheet.layout', null));
    expect(await (await desktopStorageBridge(root).request(url)).json()).toEqual(
      expect.arrayContaining([
        { key: 'hotsheet.layout', value: null },
        { key: 'hotsheet.open-projects', value: '["/synthetic/project"]' },
      ]),
    );
    const names = await readdir(root);
    expect(names).toHaveLength(2);
    expect(names.every((name) => /^[a-f0-9]{64}\.json$/.test(name))).toBe(true);
    expect((await stat(root)).mode & 0o777).toBe(0o700);
    for (const name of names) expect((await stat(join(root, name))).mode & 0o777).toBe(0o600);
  });
  it('preserves independent keys across concurrent bridges and isolates other roots', async () => {
    const root = await directory(),
      one = desktopStorageBridge(root),
      two = desktopStorageBridge(root);
    await Promise.all([one.request(url, put('hotsheet.one', '1')), two.request(url, put('hotsheet.two', '2'))]);
    expect(await (await one.request(url)).json()).toHaveLength(2);
    await one.request(url, put('hotsheet.one', 'last'));
    expect(await (await two.request(url)).json()).toContainEqual({ key: 'hotsheet.one', value: 'last' });
    expect(await (await desktopStorageBridge(await directory()).request(url)).json()).toEqual([]);
  });
  it('rejects cross-origin/non-JSON writes, oversized values and invalid/traversal keys', async () => {
    const app = desktopStorageBridge(await directory());
    expect(
      (
        await app.request(url, {
          ...put('hotsheet.valid', 'x'),
          headers: { Origin: 'https://attacker.invalid', 'Content-Type': 'application/json' },
        })
      ).status,
    ).toBe(403);
    expect((await app.request(url, { headers: { 'Sec-Fetch-Site': 'cross-site' } })).status).toBe(403);
    expect((await app.request('http://rebound.invalid:45678/__hotsheet/desktop-storage')).status).toBe(403);
    expect(
      (await app.request(url, { ...put('hotsheet.large', 'x'), body: 'x'.repeat(DESKTOP_STORAGE_BODY_LIMIT + 1) }))
        .status,
    ).toBe(413);
    expect((await app.request(url, { method: 'PUT', body: '{}' })).status).toBe(415);
    for (const [key, value] of [
      ['other', 'x'],
      ['../outside', 'x'],
      ['hotsheet.' + 'x'.repeat(512), 'x'],
      ['hotsheet.large', 'x'.repeat(DESKTOP_STORAGE_VALUE_LIMIT + 1)],
    ]) {
      expect((await app.request(url, put(key, value))).status).toBe(400);
    }
    expect((await app.request(url, { ...put('hotsheet.valid', 'x'), body: '{' })).status).toBe(400);
    expect(await (await app.request(url)).json()).toEqual([]);
  });
  it('accepts valid multibyte values whose UTF-8 JSON exceeds the former body limit', async () => {
    const app = desktopStorageBridge(await directory()),
      value = '\u754c'.repeat(2 * 1024 * 1024);
    expect((await app.request(url, put('hotsheet.draft', value))).status).toBe(204);
  });

  it('keeps the latest revision when close-time retries and earlier writes arrive out of order', async () => {
    const app = desktopStorageBridge(await directory());
    const mutation = (revision: number, value: string) => ({
      ...put('hotsheet.layout', value),
      body: JSON.stringify({ key: 'hotsheet.layout', value, clientId: 'one-window', revision }),
    });
    await Promise.all([
      app.request(url, mutation(1, 'list')),
      app.request(url, mutation(3, 'settings')),
      app.request(url, mutation(2, 'board')),
      app.request(url, mutation(3, 'settings')),
    ]);
    expect(await (await app.request(url)).json()).toEqual([{ key: 'hotsheet.layout', value: 'settings' }]);
  });

  it('ignores incomplete temporary files and refuses corrupt committed data', async () => {
    const root = await directory(),
      app = desktopStorageBridge(root);
    await app.request(url);
    await writeFile(join(root, 'interrupted.tmp'), 'unfinished');
    expect(await (await app.request(url)).json()).toEqual([]);
    await app.request(url, put('hotsheet.layout', 'board'));
    const name = (await readdir(root)).find((name) => name.endsWith('.json'))!;
    await writeFile(join(root, name), '{');
    const response = await app.request(url);
    expect(response.status).toBe(500);
    expect(await response.json()).toEqual({ error: 'Could not access desktop preferences.' });
    expect(await readFile(join(root, name), 'utf8')).toBe('{');
  });
});
