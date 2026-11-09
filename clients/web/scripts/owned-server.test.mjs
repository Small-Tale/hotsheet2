import { EventEmitter } from 'node:events';

import { describe, expect, it } from 'vitest';

import { leadsOwnProcessGroup, parseOwnedServerArguments, runOwnedServer } from './owned-server.mjs';

// HS2-F6F8MA: the launcher's stop transitions with fake processes; owned-server.e2e.test.mjs covers
// the same paths with real process groups, pipes, and Vite.
function harness({ exitOnStdinEof = true, leadsGroup = true, graceMs = 100, platform = 'linux' } = {}) {
  const child = Object.assign(new EventEmitter(), { pid: 42, exitCode: null, signalCode: null, signals: [] });
  child.kill = (signal) => {
    // Node's ChildProcess.kill is a no-op once the child has been reaped.
    if (child.exitCode === null && child.signalCode === null) child.signals.push(signal);
  };
  const exit = (code, signal = null) => {
    child.exitCode = code;
    child.signalCode = signal;
    child.emit('exit', code, signal);
    child.emit('close', code, signal);
  };
  const processHost = Object.assign(new EventEmitter(), { groupKills: [] });
  processHost.kill = (pid, signal) => processHost.groupKills.push([pid, signal]);
  const stdin = Object.assign(new EventEmitter(), { resumed: false, destroyed: false });
  stdin.resume = () => (stdin.resumed = true);
  stdin.pause = () => (stdin.resumed = false);
  stdin.destroy = () => (stdin.destroyed = true);
  const timers = { pending: new Map(), next: 1 };
  timers.setTimeout = (callback, ms) => {
    const id = timers.next++;
    timers.pending.set(id, { callback, ms });
    return id;
  };
  timers.clearTimeout = (id) => timers.pending.delete(id);
  timers.fire = () => {
    for (const [id, { callback }] of timers.pending) {
      timers.pending.delete(id);
      callback();
    }
  };
  const spawned = [];
  const killedTrees = [];
  const result = runOwnedServer({
    program: 'node',
    args: ['vite.js', '--port', '1'],
    exitOnStdinEof,
    graceMs,
    processHost,
    stdin,
    timers,
    leadsGroup: () => leadsGroup,
    platform,
    killTree: (pid) => killedTrees.push(pid),
    spawnChild: (program, args, options) => {
      spawned.push({ program, args, options });
      return child;
    },
  });
  return { child, exit, processHost, stdin, timers, spawned, result, killedTrees };
}

describe('parseOwnedServerArguments', () => {
  it('reads options before the separator and the command after it', () => {
    expect(parseOwnedServerArguments(['--exit-on-stdin-eof', '--grace-ms', '250', '--', 'node', 'vite.js'])).toEqual({
      exitOnStdinEof: true,
      graceMs: 250,
      program: 'node',
      args: ['vite.js'],
    });
    expect(parseOwnedServerArguments(['--', 'vite'])).toEqual({
      exitOnStdinEof: false,
      graceMs: 5000,
      program: 'vite',
      args: [],
    });
  });

  it('rejects a missing command, an unknown option, and an invalid grace', () => {
    expect(() => parseOwnedServerArguments(['node'])).toThrow(/Usage/);
    expect(() => parseOwnedServerArguments(['--'])).toThrow(/Usage/);
    expect(() => parseOwnedServerArguments(['--detach', '--', 'node'])).toThrow(/Unknown/);
    expect(() => parseOwnedServerArguments(['--grace-ms', '-1', '--', 'node'])).toThrow(/grace/);
    expect(() => parseOwnedServerArguments(['--grace-ms', '--', 'node'])).toThrow(/grace/);
  });
});

describe('leadsOwnProcessGroup', () => {
  it('compares the reported group id with the pid', () => {
    expect(leadsOwnProcessGroup(42, () => ({ status: 0, stdout: '   42\n' }), 'linux')).toBe(true);
    expect(leadsOwnProcessGroup(42, () => ({ status: 0, stdout: '7\n' }), 'linux')).toBe(false);
    expect(leadsOwnProcessGroup(42, () => ({ status: 1, stdout: '' }), 'linux')).toBe(false);
    expect(
      leadsOwnProcessGroup(
        42,
        () => {
          throw new Error('ps must not run on Windows');
        },
        'win32',
      ),
    ).toBe(false);
  });
});

describe('runOwnedServer', () => {
  it('keeps the server in its own group with a private stdin', () => {
    const { spawned, stdin } = harness();
    expect(spawned).toEqual([
      { program: 'node', args: ['vite.js', '--port', '1'], options: { stdio: ['ignore', 'inherit', 'inherit'] } },
    ]);
    expect(stdin.resumed).toBe(true);
  });

  it('stops the server once when the parent pipe ends, then sweeps its own group', async () => {
    const { child, exit, processHost, stdin, timers, result } = harness();
    stdin.emit('end');
    stdin.emit('close');
    processHost.emit('SIGTERM');
    expect(child.signals).toEqual(['SIGTERM']);
    exit(0);
    expect(await result).toBe(0);
    expect(processHost.groupKills).toEqual([[0, 'SIGKILL']]);
    expect(timers.pending.size).toBe(0);
    expect(processHost.listenerCount('SIGTERM')).toBe(0);
    expect(stdin.listenerCount('end')).toBe(0);
    expect(stdin.destroyed).toBe(true);
  });

  it('treats a stdin error as the parent ending', () => {
    const { child, stdin } = harness();
    stdin.emit('error', new Error('EPIPE'));
    expect(child.signals).toEqual(['SIGTERM']);
  });

  for (const [signal, code] of [
    ['SIGINT', 130],
    ['SIGTERM', 143],
    ['SIGHUP', 129],
  ])
    it(`reports ${signal} as exit ${code}`, async () => {
      const { exit, processHost, result } = harness();
      processHost.emit(signal);
      exit(null, 'SIGTERM');
      expect(await result).toBe(code);
    });

  it('force-kills a server that outlives the grace, and only while it is unreaped', async () => {
    const { child, exit, stdin, timers, result } = harness({ graceMs: 250 });
    stdin.emit('end');
    expect([...timers.pending.values()].map(({ ms }) => ms)).toEqual([250]);
    timers.fire();
    expect(child.signals).toEqual(['SIGTERM', 'SIGKILL']);
    exit(null, 'SIGKILL');
    expect(await result).toBe(137);
  });

  it('kills only the live owned Windows child tree before its pid can be reused (HS2-XP3Y4D)', async () => {
    const { exit, stdin, result, killedTrees, child, timers } = harness({ platform: 'win32' });
    stdin.emit('end');
    stdin.emit('close');
    expect(killedTrees).toEqual([child.pid]);
    expect(child.signals).toEqual([]);
    expect(timers.pending.size).toBe(0);
    exit(null, 'SIGTERM');
    expect(await result).toBe(143);
    const alreadyExited = harness({ platform: 'win32' });
    alreadyExited.child.exitCode = 0;
    alreadyExited.stdin.emit('end');
    expect(alreadyExited.killedTrees).toEqual([]);
    alreadyExited.exit(0);
    await alreadyExited.result;
  });

  it('never signals a server that already exited (no reused pid can receive it)', async () => {
    const { child, exit, processHost, stdin, timers, result } = harness();
    child.exitCode = 0;
    stdin.emit('end');
    expect(child.signals).toEqual([]);
    expect(timers.pending.size).toBe(0);
    child.exitCode = null;
    // A force-kill timer that fires after the child exits sends nothing.
    const late = harness();
    late.stdin.emit('end');
    late.child.exitCode = 0;
    late.timers.fire();
    expect(late.child.signals).toEqual(['SIGTERM']);
    exit(0);
    expect(await result).toBe(0);
    expect(processHost.groupKills).toEqual([[0, 'SIGKILL']]);
  });

  it('propagates the exit code of a server that stops by itself without sweeping', async () => {
    const { exit, processHost, result } = harness();
    exit(7);
    expect(await result).toBe(7);
    expect(processHost.groupKills).toEqual([]);
  });

  it('does not sweep a group it does not lead', async () => {
    const { exit, processHost, stdin, result } = harness({ leadsGroup: false });
    stdin.emit('end');
    exit(0);
    expect(await result).toBe(0);
    expect(processHost.groupKills).toEqual([]);
  });

  it('ignores stdin unless asked to bind to it', async () => {
    const { child, exit, stdin, result } = harness({ exitOnStdinEof: false });
    expect(stdin.resumed).toBe(false);
    stdin.emit('end');
    expect(child.signals).toEqual([]);
    exit(0);
    expect(await result).toBe(0);
  });

  it('reports a launch error once', async () => {
    const { child, result } = harness();
    child.emit('error', new Error('ENOENT'));
    child.emit('close', -2, null);
    expect(await result).toBe(1);
  });
});
