import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { expect, it } from 'vitest';

import { type ClientDiscovery, clientDiscoveryPath, localClientUrl, publishClientUrl } from './client-discovery';

it('advertises a loopback origin for wildcard listeners', () => {
  expect(localClientUrl('0.0.0.0', 4175)).toBe('http://127.0.0.1:4175');
  expect(localClientUrl('::', 4175)).toBe('http://[::1]:4175');
});

it('publishes an active client URL and cleans up only its own record', async () => {
  const home = await mkdtemp(join(tmpdir(), 'hotsheet-client-discovery-'));
  try {
    const first = await publishClientUrl('http://127.0.0.1:4175', home);
    const path = clientDiscoveryPath(home);
    const firstRecord = JSON.parse(await readFile(path, 'utf8')) as ClientDiscovery;
    expect(firstRecord).toMatchObject({ pid: process.pid, url: 'http://127.0.0.1:4175' });
    const second = await publishClientUrl('http://127.0.0.1:4180', home);
    await first();
    expect(JSON.parse(await readFile(path, 'utf8'))).toMatchObject({ url: 'http://127.0.0.1:4180' });
    await second();
    await expect(readFile(path, 'utf8')).rejects.toMatchObject({ code: 'ENOENT' });
  } finally {
    await rm(home, { recursive: true, force: true });
  }
});
