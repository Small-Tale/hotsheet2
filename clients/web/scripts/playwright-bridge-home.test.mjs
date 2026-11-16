import { join, resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { bridgeCliBinary, releaseBridgeHome } from './playwright-bridge-home.mjs';

describe('Playwright bridge home release (HS2-B0WP94)', () => {
  it('resolves the CLI the bridge launches: explicit override, repo root, then the checkout debug build', () => {
    expect(bridgeCliBinary({ HOTSHEET_CLI_BIN: '/opt/hs/hotsheet-cli' }, '/repo/clients/web', 'darwin')).toBe(
      '/opt/hs/hotsheet-cli',
    );
    expect(bridgeCliBinary({ HOTSHEET_REPO_ROOT: '/snap' }, '/repo/clients/web', 'linux')).toBe(
      resolve('/snap', 'target/debug/hotsheet-cli'),
    );
    expect(bridgeCliBinary({ HOTSHEET_CLI_BIN: '  ', HOTSHEET_REPO_ROOT: ' ' }, '/repo/clients/web', 'win32')).toBe(
      resolve('/repo', 'target/debug/hotsheet-cli.exe'),
    );
  });

  it('stops the bootstrap server under the run home before removing the directories', () => {
    const order = [];
    const outcome = releaseBridgeHome({
      home: '/tmp/home',
      directories: ['/tmp/vite', '/tmp/home'],
      env: { PATH: '/bin', HOTSHEET_HOME: '/real/home' },
      cli: '/bin/hotsheet-cli',
      exists: (path) => path === join('/tmp/home', 'server-bootstrap.hs2'),
      run: (command, args, options) => {
        order.push(['run', command, args, options.env.HOTSHEET_HOME, options.env.PATH]);
        return { status: 0, stdout: '', stderr: '' };
      },
      remove: (path) => order.push(['remove', path]),
      warn: (message) => order.push(['warn', message]),
    });
    expect(outcome).toEqual({ stopped: true });
    expect(order).toEqual([
      [
        'run',
        '/bin/hotsheet-cli',
        ['serve', '-C', join('/tmp/home', 'server-bootstrap.hs2'), '--stop', '--force'],
        '/tmp/home',
        '/bin',
      ],
      ['remove', '/tmp/vite'],
      ['remove', '/tmp/home'],
    ]);
  });

  it('runs nothing when no spec opened a project through the bridge', () => {
    const removed = [];
    const outcome = releaseBridgeHome({
      home: '/tmp/home',
      directories: ['/tmp/home'],
      exists: () => false,
      run: () => {
        throw new Error('no server to stop');
      },
      remove: (path) => removed.push(path),
    });
    expect(outcome).toEqual({ stopped: false });
    expect(removed).toEqual(['/tmp/home']);
  });

  it('reports a failed or unlaunchable stop and still removes every directory', () => {
    for (const result of [
      { status: 1, stdout: '', stderr: 'no server answered\n' },
      { status: null, stdout: '', stderr: '', error: new Error('ENOENT: no such file or directory') },
      { status: 2, stdout: '', stderr: '' },
    ]) {
      const warnings = [];
      const removed = [];
      const outcome = releaseBridgeHome({
        home: '/tmp/home',
        directories: ['/tmp/a', '/tmp/home'],
        cli: '/missing/hotsheet-cli',
        exists: () => true,
        run: () => result,
        remove: (path) => removed.push(path),
        warn: (message) => warnings.push(message),
      });
      expect(outcome.stopped).toBe(false);
      expect(outcome.error).toBe(result.error?.message ?? (result.stderr.trim() || 'exit 2'));
      expect(warnings).toHaveLength(1);
      expect(warnings[0]).toContain(outcome.error);
      expect(removed).toEqual(['/tmp/a', '/tmp/home']);
    }
  });
});
