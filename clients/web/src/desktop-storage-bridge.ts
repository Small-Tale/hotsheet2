import { createHash, randomUUID } from 'node:crypto';
import { mkdir, readdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

import { Hono } from 'hono';
import { bodyLimit } from 'hono/body-limit';
import { HTTPException } from 'hono/http-exception';

import {
  DESKTOP_STORAGE_BODY_LIMIT,
  DESKTOP_STORAGE_PATH,
  parseDesktopStorageEntry,
  parseDesktopStorageMutation,
} from './desktop-storage-contract';
import { parseJson } from './json-value';

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
  app.use(DESKTOP_STORAGE_PATH, async (context, next) => {
    const origin = context.req.header('origin'),
      url = new URL(context.req.url);
    if (!['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname))
      return context.json({ error: 'Desktop storage requires a loopback host.' }, 403);
    if ((origin && origin !== new URL(context.req.url).origin) || context.req.header('sec-fetch-site') === 'cross-site')
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
  return app;
}
