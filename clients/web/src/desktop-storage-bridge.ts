import { createHash, randomUUID } from 'node:crypto';
import { mkdir, readdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

import { Hono } from 'hono';
import { bodyLimit } from 'hono/body-limit';
import { HTTPException } from 'hono/http-exception';

import {
  DESKTOP_DRAFT_FILE_HEADERS,
  DESKTOP_DRAFT_FILE_LIMIT,
  DESKTOP_DRAFT_FILES_PATH,
  DESKTOP_STORAGE_BODY_LIMIT,
  DESKTOP_STORAGE_PATH,
  desktopDraftFileKey,
  parseDesktopDraftFileMetadata,
  parseDesktopStorageEntry,
  parseDesktopStorageMutation,
} from './desktop-storage-contract';
import { parseJson } from './json-value';

/** Draft bytes live beside the preference records; the preference listing ignores directories. */
export const DESKTOP_DRAFT_FILES_DIRECTORY = 'draft-files-v1';

/** Per-key atomic records avoid losing unrelated writes from another desktop bridge. */
export function desktopStorageBridge(directory: string): Hono {
  const app = new Hono(),
    writes = new Map<string, Promise<void>>(),
    revisions = new Map<string, number>();
  // A JSON parse error can include a slice of the stored value; never log its exception.
  app.onError((error, context) =>
    error instanceof HTTPException
      ? context.json({ error: 'Desktop preference request rejected.' }, error.status)
      : context.json({ error: 'Could not access desktop preferences.' }, 500),
  );
  app.use(DESKTOP_STORAGE_PATH, bodyLimit({ maxSize: DESKTOP_STORAGE_BODY_LIMIT }));
  // A draft save must declare its length so the limit never buffers it before it joins the key's
  // queue; browsers always send a File body's Content-Length (HS2-791W19).
  app.use(DESKTOP_DRAFT_FILES_PATH, async (context, next) => {
    if (
      context.req.method === 'PUT' &&
      (!/^\d+$/.test(context.req.header('content-length') ?? '') || context.req.header('transfer-encoding'))
    )
      return context.json({ error: 'Desktop draft files require a declared length.' }, 411);
    await next();
  });
  app.use(DESKTOP_DRAFT_FILES_PATH, bodyLimit({ maxSize: DESKTOP_DRAFT_FILE_LIMIT }));
  for (const path of [DESKTOP_STORAGE_PATH, DESKTOP_DRAFT_FILES_PATH])
    app.use(path, async (context, next) => {
      const origin = context.req.header('origin'),
        url = new URL(context.req.url);
      if (!['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname))
        return context.json({ error: 'Desktop storage requires a loopback host.' }, 403);
      if ((origin && origin !== url.origin) || context.req.header('sec-fetch-site') === 'cross-site')
        return context.json({ error: 'Desktop storage requires the local application origin.' }, 403);
      context.header('Cache-Control', 'no-store');
      await next();
    });
  app.get(DESKTOP_STORAGE_PATH, async (context) => {
    await mkdir(directory, { recursive: true, mode: 0o700 });
    const names = (await readdir(directory)).filter((name) => /^[a-f0-9]{64}\.json$/.test(name));
    const entries = await Promise.all(
      names.map(async (name) => parseDesktopStorageEntry(parseJson(await readFile(join(directory, name), 'utf8')))),
    );
    return context.json(entries);
  });
  app.put(DESKTOP_STORAGE_PATH, async (context) => {
    if (!context.req.header('content-type')?.startsWith('application/json'))
      return context.json({ error: 'Desktop storage requires JSON.' }, 415);
    let entry;
    try {
      entry = parseDesktopStorageMutation(await context.req.json<unknown>());
    } catch {
      return context.json({ error: 'Invalid desktop storage entry.' }, 400);
    }
    const mutation = entry,
      revisionKey = `${mutation.clientId}:${mutation.key}`,
      previous = writes.get(mutation.key) ?? Promise.resolve(),
      write = previous
        .catch(() => undefined)
        .then(async () => {
          if ((revisions.get(revisionKey) ?? 0) >= mutation.revision) return;
          await mkdir(directory, { recursive: true, mode: 0o700 });
          const name = createHash('sha256').update(mutation.key).digest('hex'),
            destination = join(directory, `${name}.json`),
            temporary = join(directory, `${name}.${randomUUID()}.tmp`);
          try {
            await writeFile(temporary, JSON.stringify({ key: mutation.key, value: mutation.value }), {
              mode: 0o600,
              flag: 'wx',
            });
            await rename(temporary, destination);
            revisions.set(revisionKey, mutation.revision);
          } finally {
            await rm(temporary, { force: true });
          }
        });
    writes.set(mutation.key, write);
    try {
      await write;
    } finally {
      if (writes.get(mutation.key) === write) writes.delete(mutation.key);
    }
    // Null is a tombstone: an old webview origin must never import a deleted setting again.
    return context.body(null, 204);
  });
  draftFileRoutes(app, join(directory, DESKTOP_DRAFT_FILES_DIRECTORY));
  return app;
}

/**
 * Staged attachment bytes survive a new ephemeral bridge origin (HS2-791W19). Each draft file is
 * one private `<sha256(key)>.draft` record: a 4-byte big-endian metadata length, the metadata
 * JSON (which names its key), then the raw bytes. A save writes a temporary and renames it over
 * the record, so a reader sees the whole old record or the whole new one, never a mix. Content is
 * never logged.
 */
function draftFileRoutes(app: Hono, directory: string) {
  const writes = new Map<string, Promise<unknown>>();
  const keyOf = (url: string) => {
    const key = new URL(url).searchParams.get('key') ?? '';
    return desktopDraftFileKey(key) ? key : undefined;
  };
  const recordOf = (key: string) => {
    const name = createHash('sha256').update(key).digest('hex');
    return { path: join(directory, `${name}.draft`), name };
  };
  // One queue per key: a removal issued after a save must not be overtaken by it.
  const serialized = (key: string, work: () => Promise<void>) => {
    const write = (writes.get(key) ?? Promise.resolve()).catch(() => undefined).then(work);
    writes.set(key, write);
    return write.finally(() => {
      if (writes.get(key) === write) writes.delete(key);
    });
  };
  app.get(DESKTOP_DRAFT_FILES_PATH, async (context) => {
    const key = keyOf(context.req.url);
    if (!key) return context.json({ error: 'Invalid desktop draft file.' }, 400);
    await writes.get(key)?.catch(() => undefined);
    let record;
    try {
      record = await readFile(recordOf(key).path);
    } catch (error) {
      if (error instanceof Error && 'code' in error && error.code === 'ENOENT')
        return context.json({ error: 'Desktop draft file not found.' }, 404);
      throw error;
    }
    const { metadata, bytes } = parseDraftRecord(record);
    if (metadata.key !== key) return context.json({ error: 'Desktop draft file not found.' }, 404);
    return context.body(new Uint8Array(bytes), 200, {
      'Content-Type': 'application/octet-stream',
      [DESKTOP_DRAFT_FILE_HEADERS.name]: encodeURIComponent(metadata.name),
      [DESKTOP_DRAFT_FILE_HEADERS.type]: encodeURIComponent(metadata.type),
      [DESKTOP_DRAFT_FILE_HEADERS.lastModified]: String(metadata.lastModified),
    });
  });
  app.put(DESKTOP_DRAFT_FILES_PATH, async (context) => {
    const key = keyOf(context.req.url);
    let metadata;
    try {
      metadata = parseDesktopDraftFileMetadata({
        key,
        name: decodeURIComponent(context.req.header(DESKTOP_DRAFT_FILE_HEADERS.name) ?? ''),
        type: decodeURIComponent(context.req.header(DESKTOP_DRAFT_FILE_HEADERS.type) ?? ''),
        lastModified: Number(context.req.header(DESKTOP_DRAFT_FILE_HEADERS.lastModified)),
      });
    } catch {
      return context.json({ error: 'Invalid desktop draft file.' }, 400);
    }
    // Join the key's queue on arrival, before the body finishes uploading, so a removal sent
    // after this save cannot overtake it.
    const body = context.req.arrayBuffer(),
      record = recordOf(metadata.key);
    // An aborted upload can reject while an earlier write still holds the queue; the queued
    // work awaits it and reports the failure, so it must not count as unhandled meanwhile.
    body.catch(() => undefined);
    await serialized(metadata.key, async () => {
      const header = Buffer.from(JSON.stringify(metadata)),
        length = Buffer.alloc(4);
      length.writeUInt32BE(header.length);
      const contents = Buffer.concat([length, header, new Uint8Array(await body)]);
      await mkdir(directory, { recursive: true, mode: 0o700 });
      const temporary = join(directory, `${record.name}.${randomUUID()}.tmp`);
      try {
        await writeFile(temporary, contents, { mode: 0o600, flag: 'wx' });
        await rename(temporary, record.path);
      } finally {
        await rm(temporary, { force: true });
      }
    });
    return context.body(null, 204);
  });
  app.delete(DESKTOP_DRAFT_FILES_PATH, async (context) => {
    const key = keyOf(context.req.url);
    if (!key) return context.json({ error: 'Invalid desktop draft file.' }, 400);
    await serialized(key, () => rm(recordOf(key).path, { force: true }));
    return context.body(null, 204);
  });
}

/** Split a committed record; a truncated or malformed one is refused, never partially served. */
function parseDraftRecord(record: Buffer) {
  if (record.length < 4) throw new Error('Corrupt desktop draft file.');
  const length = record.readUInt32BE(0);
  if (length > record.length - 4) throw new Error('Corrupt desktop draft file.');
  return {
    metadata: parseDesktopDraftFileMetadata(parseJson(record.subarray(4, 4 + length).toString('utf8'))),
    bytes: record.subarray(4 + length),
  };
}
