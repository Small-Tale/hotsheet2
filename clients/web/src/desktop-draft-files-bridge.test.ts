import { mkdtemp, readdir, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it, vi } from 'vitest';

import { DESKTOP_DRAFT_FILES_DIRECTORY, desktopStorageBridge } from './desktop-storage-bridge';
import { DESKTOP_DRAFT_FILE_LIMIT, DESKTOP_DRAFT_FILES_PATH, DESKTOP_STORAGE_PATH } from './desktop-storage-contract';

// Fault injection for the commit point of a record write (HS2-791W19).
const faults = vi.hoisted(() => ({ rename: false }));
vi.mock('node:fs/promises', async (original) => {
  const actual = await original<typeof import('node:fs/promises')>();
  return {
    ...actual,
    rename: (from: string, to: string) =>
      faults.rename ? Promise.reject(new Error('synthetic crash before commit')) : actual.rename(from, to),
  };
});

const roots: string[] = [];
afterEach(async () => {
  faults.rename = false;
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
async function directory() {
  const root = await mkdtemp(join(tmpdir(), 'hs2-desktop-drafts-'));
  roots.push(root);
  return join(root, 'preferences');
}
const origin = 'http://127.0.0.1:45678';
const draftUrl = (key: string) => `${origin}${DESKTOP_DRAFT_FILES_PATH}?key=${encodeURIComponent(key)}`;
const putDraft = (
  bytes: Uint8Array<ArrayBuffer>,
  name = 'synthetic proof.bin',
  headers: Record<string, string> = {},
) => ({
  method: 'PUT',
  headers: {
    'Content-Type': 'application/octet-stream',
    Origin: origin,
    'X-Hotsheet-Draft-Name': encodeURIComponent(name),
    'X-Hotsheet-Draft-Type': encodeURIComponent('application/x-synthetic'),
    'X-Hotsheet-Draft-Modified': '1700000000000',
    // A browser always sends a File body's length.
    'Content-Length': String(bytes.byteLength),
    ...headers,
  },
  body: bytes,
});
/** A declared-length body that stays open until released, like a slow upload. */
function heldBody(bytes: Uint8Array<ArrayBuffer>) {
  let release = () => undefined as unknown;
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      release = () => {
        controller.enqueue(bytes);
        controller.close();
      };
    },
  });
  return { body, release: () => release() };
}
const streamed = (body: ReadableStream<Uint8Array>, length: number, name?: string) =>
  ({ ...putDraft(new Uint8Array(length), name), body, duplex: 'half' }) as RequestInit;
async function draft(app: ReturnType<typeof desktopStorageBridge>, key: string) {
  const response = await app.request(draftUrl(key));
  return response.status === 200
    ? {
        name: decodeURIComponent(response.headers.get('x-hotsheet-draft-name') ?? ''),
        bytes: [...new Uint8Array(await response.arrayBuffer())],
      }
    : response.status;
}
const records = async (root: string) => (await readdir(join(root, DESKTOP_DRAFT_FILES_DIRECTORY))).sort();

describe('private desktop draft files (HS2-791W19)', () => {
  it('returns exact bytes and metadata from a recreated bridge in one private record', async () => {
    const root = await directory(),
      bytes = new Uint8Array([0, 255, 128, 10, 13, 0xef, 0xbb, 0xbf]),
      key = 'composer:synthetic-project-1a2b:draft-one';
    expect((await desktopStorageBridge(root).request(draftUrl(key), putDraft(bytes, 'résumé 证据.bin'))).status).toBe(
      204,
    );
    const response = await desktopStorageBridge(root).request(draftUrl(key));
    expect(response.status).toBe(200);
    expect(response.headers.get('cache-control')).toBe('no-store');
    expect(new Uint8Array(await response.arrayBuffer())).toEqual(bytes);
    expect(decodeURIComponent(response.headers.get('x-hotsheet-draft-name')!)).toBe('résumé 证据.bin');
    expect(decodeURIComponent(response.headers.get('x-hotsheet-draft-type')!)).toBe('application/x-synthetic');
    expect(response.headers.get('x-hotsheet-draft-modified')).toBe('1700000000000');
    // Preference listing ignores the draft directory; the record name never reveals the key.
    expect(await (await desktopStorageBridge(root).request(`${origin}${DESKTOP_STORAGE_PATH}`)).json()).toEqual([]);
    const drafts = join(root, DESKTOP_DRAFT_FILES_DIRECTORY),
      names = await records(root);
    expect(names).toHaveLength(1);
    expect(names[0]).toMatch(/^[a-f0-9]{64}\.draft$/);
    expect((await stat(drafts)).mode & 0o777).toBe(0o700);
    expect((await stat(join(drafts, names[0]))).mode & 0o777).toBe(0o600);
  });

  it('isolates projects, draft kinds, and storage roots, and removes the record', async () => {
    const root = await directory(),
      app = desktopStorageBridge(root),
      key = 'composer:project-a:file';
    await app.request(draftUrl(key), putDraft(new Uint8Array([1])));
    for (const other of ['composer:project-b:file', 'not-working:project-a:file'])
      expect((await app.request(draftUrl(other))).status).toBe(404);
    expect((await desktopStorageBridge(await directory()).request(draftUrl(key))).status).toBe(404);
    expect((await app.request(draftUrl(key), { method: 'DELETE' })).status).toBe(204);
    expect(await draft(app, key)).toBe(404);
    expect(await records(root)).toEqual([]);
    expect((await app.request(draftUrl(key), { method: 'DELETE' })).status).toBe(204);
  });

  it('applies a removal sent while a slow save is still uploading after that save', async () => {
    const root = await directory(),
      app = desktopStorageBridge(root),
      key = 'not-working:project-a:file',
      upload = heldBody(new Uint8Array([7, 7])),
      saving = app.request(draftUrl(key), streamed(upload.body, 2)),
      removing = app.request(draftUrl(key), { method: 'DELETE' });
    let removedEarly = false;
    void Promise.resolve(removing).then(() => {
      removedEarly = true;
    });
    // An unrelated removal completes a full file-system round trip; this key's removal must not.
    await app.request(draftUrl('composer:project-a:unrelated'), { method: 'DELETE' });
    expect(removedEarly).toBe(false);
    upload.release();
    expect([(await saving).status, (await removing).status]).toEqual([204, 204]);
    expect(await draft(app, key)).toBe(404);
    // And a save sent after a removal wins.
    await Promise.all([
      app.request(draftUrl(key), { method: 'DELETE' }),
      app.request(draftUrl(key), putDraft(new Uint8Array([9]))),
    ]);
    expect(await draft(app, key)).toEqual({ name: 'synthetic proof.bin', bytes: [9] });
    expect((await records(root)).some((name) => name.endsWith('.tmp'))).toBe(false);
  });

  it('refuses a save without a declared length before buffering it', async () => {
    const root = await directory(),
      app = desktopStorageBridge(root),
      key = 'composer:project-a:file',
      unknownLength = { ...putDraft(new Uint8Array([1])) };
    delete (unknownLength.headers as Partial<Record<string, string>>)['Content-Length'];
    const held = heldBody(new Uint8Array([1]));
    // An unknown-length stream is refused at once rather than held open or buffered.
    expect(
      (await app.request(draftUrl(key), { ...unknownLength, body: held.body, duplex: 'half' } as RequestInit)).status,
    ).toBe(411);
    expect(
      (await app.request(draftUrl(key), putDraft(new Uint8Array([1]), 'x', { 'Transfer-Encoding': 'chunked' }))).status,
    ).toBe(411);
    expect(
      (await app.request(draftUrl(key), putDraft(new Uint8Array([1]), 'x', { 'Content-Length': 'many' }))).status,
    ).toBe(411);
    expect(await draft(app, key)).toBe(404);
  });

  it('keeps the complete previous record when an overwrite fails at its commit point', async () => {
    const root = await directory(),
      app = desktopStorageBridge(root),
      key = 'composer:project-a:file';
    await app.request(draftUrl(key), putDraft(new Uint8Array([1, 1]), 'first.bin'));
    faults.rename = true;
    expect((await app.request(draftUrl(key), putDraft(new Uint8Array([2, 2, 2]), 'second.bin'))).status).toBe(500);
    faults.rename = false;
    // Old name with old bytes: never the new name with old bytes or the reverse.
    expect(await draft(app, key)).toEqual({ name: 'first.bin', bytes: [1, 1] });
    expect(await records(root)).toHaveLength(1);
    // A crash that leaves its temporary behind is ignored and replaced by the next save.
    const name = (await records(root))[0].replace('.draft', '');
    await writeFile(join(root, DESKTOP_DRAFT_FILES_DIRECTORY, `${name}.crashed.tmp`), 'partial');
    expect(await draft(app, key)).toEqual({ name: 'first.bin', bytes: [1, 1] });
    await app.request(draftUrl(key), putDraft(new Uint8Array([3]), 'third.bin'));
    expect(await draft(app, key)).toEqual({ name: 'third.bin', bytes: [3] });
  });

  it('reports an upload aborted behind a slower save and keeps the earlier complete record', async () => {
    const root = await directory(),
      app = desktopStorageBridge(root),
      key = 'composer:project-a:file',
      upload = heldBody(new Uint8Array([1, 2]));
    const aborted = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.error(new Error('synthetic client abort'));
      },
    });
    const slow = app.request(draftUrl(key), streamed(upload.body, 2, 'slow.bin')),
      failed = app.request(draftUrl(key), streamed(aborted, 4, 'aborted.bin'));
    // Let the aborted body settle while the slower save still owns the key's queue.
    await new Promise((resolve) => setImmediate(resolve));
    upload.release();
    expect((await slow).status).toBe(204);
    expect((await failed).status).toBe(500);
    expect(await draft(app, key)).toEqual({ name: 'slow.bin', bytes: [1, 2] });
    expect((await records(root)).some((name) => name.endsWith('.tmp'))).toBe(false);
  });

  it('rejects foreign origins, invalid keys and metadata, and files beyond the attachment limit', async () => {
    const root = await directory(),
      app = desktopStorageBridge(root),
      key = 'composer:project-a:file',
      body = new Uint8Array([1]);
    expect((await app.request(draftUrl(key), putDraft(body, 'x', { Origin: 'https://attacker.invalid' }))).status).toBe(
      403,
    );
    expect((await app.request(draftUrl(key), { headers: { 'Sec-Fetch-Site': 'cross-site' } })).status).toBe(403);
    expect(
      (await app.request(`http://rebound.invalid:45678${DESKTOP_DRAFT_FILES_PATH}?key=${encodeURIComponent(key)}`))
        .status,
    ).toBe(403);
    for (const invalid of ['', 'hotsheet.layout', '../composer:x', `composer:${'x'.repeat(512)}`]) {
      expect((await app.request(draftUrl(invalid))).status).toBe(400);
      expect((await app.request(draftUrl(invalid), putDraft(body))).status).toBe(400);
      expect((await app.request(draftUrl(invalid), { method: 'DELETE' })).status).toBe(400);
    }
    for (const headers of [
      { 'X-Hotsheet-Draft-Name': '' } as Record<string, string>,
      { 'X-Hotsheet-Draft-Name': '%E0%A4%A' },
      { 'X-Hotsheet-Draft-Name': 'x'.repeat(1025) },
      { 'X-Hotsheet-Draft-Type': 'x'.repeat(257) },
      { 'X-Hotsheet-Draft-Modified': 'soon' },
      { 'X-Hotsheet-Draft-Modified': '-1' },
    ])
      expect((await app.request(draftUrl(key), putDraft(body, 'x', headers))).status).toBe(400);
    expect(
      (
        await app.request(
          draftUrl(key),
          putDraft(body, 'x', { 'Content-Length': String(DESKTOP_DRAFT_FILE_LIMIT + 1) }),
        )
      ).status,
    ).toBe(413);
    expect(await draft(app, key)).toBe(404);
  });

  it('refuses truncated and corrupt records without serving part of them or their content', async () => {
    const root = await directory(),
      app = desktopStorageBridge(root),
      key = 'composer:project-a:file';
    await app.request(draftUrl(key), putDraft(new Uint8Array([1])));
    const path = join(root, DESKTOP_DRAFT_FILES_DIRECTORY, (await records(root))[0]),
      intact = await readFile(path);
    for (const corrupt of [
      intact.subarray(0, 2),
      Buffer.concat([Buffer.from([0, 0, 255, 255]), Buffer.from('{"synthetic secret')]),
      Buffer.concat([Buffer.from([0, 0, 0, 5]), Buffer.from('{"a":'), Buffer.from([1])]),
    ]) {
      await writeFile(path, corrupt);
      const response = await app.request(draftUrl(key));
      expect(response.status).toBe(500);
      expect(await response.json()).toEqual({ error: 'Could not access desktop preferences.' });
    }
  });
});
