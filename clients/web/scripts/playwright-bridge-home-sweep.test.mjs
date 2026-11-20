import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import {
  BRIDGE_OWNER_FILE,
  processAlive,
  recordBridgeOwner,
  sweepAbandonedBridgeHomes,
} from './playwright-bridge-home.mjs';

describe('abandoned Playwright bridge homes (HS2-P69XHN)', () => {
  it('records the owning process and the run directories in the home', () => {
    const writes = [];
    recordBridgeOwner({
      home: '/tmp/hotsheet-playwright-home-a',
      directories: ['/tmp/hotsheet-playwright-vite-a', '/tmp/hotsheet-playwright-home-a'],
      pid: 42,
      write: (path, text) => writes.push([path, JSON.parse(text)]),
    });
    expect(writes).toEqual([
      [
        join('/tmp/hotsheet-playwright-home-a', BRIDGE_OWNER_FILE),
        { pid: 42, directories: ['/tmp/hotsheet-playwright-vite-a', '/tmp/hotsheet-playwright-home-a'] },
      ],
    ]);
  });

  it('treats a process it may not signal as alive and a missing one as gone', () => {
    const fail = (code) => () => {
      throw Object.assign(new Error(code), { code });
    };
    expect(processAlive(1, () => true)).toBe(true);
    expect(processAlive(1, fail('EPERM'))).toBe(true);
    expect(processAlive(1, fail('ESRCH'))).toBe(false);
  });

  it('releases only homes whose recorded owner is gone, and only run directories under the root', () => {
    const root = '/tmp/root';
    const records = {
      [join(root, 'hotsheet-playwright-home-dead', BRIDGE_OWNER_FILE)]: JSON.stringify({
        pid: 7,
        directories: [
          join(root, 'hotsheet-playwright-vite-dead'),
          join(root, 'unrelated-dir'),
          '/elsewhere/hotsheet-playwright-vite-x',
          join(root, 'nested', 'hotsheet-playwright-vite-y'),
          42,
        ],
      }),
      [join(root, 'hotsheet-playwright-home-live', BRIDGE_OWNER_FILE)]: JSON.stringify({ pid: 8, directories: [] }),
      [join(root, 'hotsheet-playwright-home-bad', BRIDGE_OWNER_FILE)]: '{not json',
      [join(root, 'hotsheet-playwright-home-nopid', BRIDGE_OWNER_FILE)]: JSON.stringify({ pid: 'x' }),
    };
    const released = [];
    const swept = sweepAbandonedBridgeHomes({
      root,
      list: () => [
        'hotsheet-playwright-home-dead',
        'hotsheet-playwright-home-live',
        'hotsheet-playwright-home-bad',
        'hotsheet-playwright-home-nopid',
        'hotsheet-playwright-home-unrecorded',
        'hotsheet-playwright-vite-dead',
        'other',
      ],
      read: (path) => {
        if (!(path in records)) throw new Error('ENOENT');
        return records[path];
      },
      alive: (pid) => pid === 8,
      release: (input) => released.push(input),
    });
    expect(swept).toEqual([join(root, 'hotsheet-playwright-home-dead')]);
    expect(released).toEqual([
      {
        home: join(root, 'hotsheet-playwright-home-dead'),
        directories: [join(root, 'hotsheet-playwright-vite-dead'), join(root, 'hotsheet-playwright-home-dead')],
      },
    ]);
  });

  it('keeps recorded run directories when the root is not in normal form (HS2-41WX3Q)', () => {
    // A Windows `join` rewrites separators, so a raw string match against `root` dropped every
    // recorded directory there; a trailing separator or `.` segment does the same on any platform.
    const normal = join('/tmp', 'root');
    for (const root of [`${normal}/`, '/tmp/./root']) {
      const released = [];
      const swept = sweepAbandonedBridgeHomes({
        root,
        list: () => ['hotsheet-playwright-home-dead'],
        read: () =>
          JSON.stringify({
            pid: 7,
            directories: [
              join(normal, 'hotsheet-playwright-vite-dead'),
              join(normal, 'nested', 'hotsheet-playwright-vite-y'),
            ],
          }),
        alive: () => false,
        release: (input) => released.push(input),
      });
      expect(swept).toEqual([join(root, 'hotsheet-playwright-home-dead')]);
      expect(released).toEqual([
        {
          home: join(root, 'hotsheet-playwright-home-dead'),
          directories: [join(normal, 'hotsheet-playwright-vite-dead'), join(root, 'hotsheet-playwright-home-dead')],
        },
      ]);
    }
  });

  it('does nothing when the root cannot be listed', () => {
    expect(
      sweepAbandonedBridgeHomes({
        root: '/missing',
        list: () => {
          throw new Error('ENOENT');
        },
      }),
    ).toEqual([]);
  });

  describe('with a real SIGKILLed owner', () => {
    let root = '';
    afterEach(() => {
      if (root) rmSync(root, { recursive: true, force: true });
    });

    it('releases the run directories a killed runner left behind and keeps a live run', async () => {
      root = mkdtempSync(join(tmpdir(), 'hs2-p69xhn-'));
      const owner = (name) => {
        const child = spawn(
          process.execPath,
          [
            '--input-type=module',
            '-e',
            `import { mkdirSync } from 'node:fs';
             import { join } from 'node:path';
             import { recordBridgeOwner } from ${JSON.stringify(new URL('./playwright-bridge-home.mjs', import.meta.url).href)};
             const root = ${JSON.stringify(root)};
             const home = join(root, 'hotsheet-playwright-home-${name}');
             const vite = join(root, 'hotsheet-playwright-vite-${name}');
             mkdirSync(home); mkdirSync(vite);
             recordBridgeOwner({ home, directories: [vite, home] });
             process.stdout.write('ready\\n');
             setInterval(() => {}, 1000);`,
          ],
          { stdio: ['ignore', 'pipe', 'inherit'] },
        );
        return new Promise((resolve, reject) => {
          child.once('error', reject);
          child.stdout.once('data', () => resolve(child));
        });
      };
      const killed = await owner('killed');
      const live = await owner('live');
      try {
        const exited = new Promise((resolve) => killed.once('exit', resolve));
        killed.kill('SIGKILL');
        await exited;
        const swept = sweepAbandonedBridgeHomes({ root });
        expect(swept).toEqual([join(root, 'hotsheet-playwright-home-killed')]);
        expect(existsSync(join(root, 'hotsheet-playwright-home-killed'))).toBe(false);
        expect(existsSync(join(root, 'hotsheet-playwright-vite-killed'))).toBe(false);
        expect(existsSync(join(root, 'hotsheet-playwright-home-live'))).toBe(true);
        expect(existsSync(join(root, 'hotsheet-playwright-vite-live'))).toBe(true);
      } finally {
        live.kill('SIGKILL');
      }
    });
  });
});
