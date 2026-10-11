import { randomUUID } from 'node:crypto';
import { mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { resolve } from 'node:path';

import { isRecord, parseJson } from './json-value';

export interface ClientDiscovery {
  pid: number;
  url: string;
  started_at: string;
  id: string;
}

export function clientDiscoveryPath(home = process.env.HOTSHEET_HOME || resolve(homedir(), '.hotsheet2')): string {
  return resolve(home, 'client.json');
}

export function localClientUrl(host: string, port: number): string {
  const localHost = host === '0.0.0.0' ? '127.0.0.1' : host === '::' ? '::1' : host;
  return `http://${localHost.includes(':') ? `[${localHost}]` : localHost}:${port}`;
}

/** Publish the active local web origin; cleanup removes only this host's record. */
export async function publishClientUrl(url: string, home?: string): Promise<() => Promise<void>> {
  const path = clientDiscoveryPath(home);
  const id = randomUUID();
  const record: ClientDiscovery = { pid: process.pid, url, started_at: new Date().toISOString(), id };
  await mkdir(resolve(path, '..'), { recursive: true });
  const temporary = `${path}.${id}.tmp`;
  try {
    await writeFile(temporary, JSON.stringify(record), { mode: 0o600 });
    await rename(temporary, path);
  } catch (error) {
    await rm(temporary, { force: true });
    throw error;
  }
  return async () => {
    try {
      const current = parseJson(await readFile(path, 'utf8'));
      if (isRecord(current) && current.id === id) await rm(path, { force: true });
    } catch {
      // A stale or replaced record belongs to another host or has already been removed.
    }
  };
}
