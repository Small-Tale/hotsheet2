import { spawn } from 'node:child_process';

import { describe, expect, it } from 'vitest';

import {
  createProcessTable,
  descendantsIn,
  parseProcessRows,
  powershellHelperCommand,
} from './windows-process-table.mjs';

// HS2-S5BQRD: the line protocol, sharing, restart, and timeout of the Windows process-table helper,
// driven by a synthetic Node helper so they run on every platform.
const FAKE = `
const mode = process.argv[1];
let queries = 0;
process.stdin.setEncoding('utf8');
let buffer = '';
process.stdin.on('data', (chunk) => {
  buffer += chunk;
  let end;
  while ((end = buffer.indexOf('\\n')) >= 0) {
    buffer = buffer.slice(end + 1);
    queries += 1;
    if (mode === 'silent') continue;
    process.stdout.write('4\\t0\\t0\\tSystem\\t\\r\\n' + (100 + queries) + '\\t4\\t132000000000000000\\tsynthetic tool.exe\\t"C:\\\\synthetic tool.exe" --home C:\\\\home\\r\\nnot a row\\r\\n.\\r\\n');
    if (mode === 'exit-after-answer') process.exit(0);
  }
});
`;
const fake = (mode = 'answer') => ({ command: process.execPath, args: ['-e', FAKE, mode] });

describe('Windows process table (HS2-S5BQRD)', () => {
  it('answers each query from one warm helper and shares a query in flight', async () => {
    const table = createProcessTable({ command: fake() });
    try {
      const [first, shared] = await Promise.all([table.snapshot(), table.snapshot()]);
      expect(shared).toBe(first);
      expect(first.get(4)).toEqual({ pid: 4, ppid: 0, started: '0', name: 'System', command: '' });
      expect(first.get(101)).toEqual({
        pid: 101,
        ppid: 4,
        started: '132000000000000000',
        name: 'synthetic tool.exe',
        command: '"C:\\synthetic tool.exe" --home C:\\home',
      });
      expect(first.size).toBe(2);
      // A later query reaches the same helper, which counts it as its second.
      expect((await table.snapshot()).has(102)).toBe(true);
    } finally {
      table.close();
    }
  });

  it('starts a new helper after the previous one exits', async () => {
    const table = createProcessTable({ command: fake('exit-after-answer') });
    try {
      expect((await table.snapshot()).has(101)).toBe(true);
      // The first helper exited after answering; the next query may race its exit, then restarts.
      const next = await table.snapshot().catch(() => table.snapshot());
      expect(next.has(101)).toBe(true);
    } finally {
      table.close();
    }
  });

  it('rejects a query the helper never answers and recovers with a new helper', async () => {
    let started = 0;
    const table = createProcessTable({
      command: fake('silent'),
      timeoutMs: 300,
      spawnImpl: (...args) => {
        started += 1;
        return (started === 1 ? spawnSilent : spawnAnswering)(...args);
      },
    });
    try {
      await expect(table.snapshot()).rejects.toThrow(/did not answer within 300 ms/);
      expect((await table.snapshot()).has(101)).toBe(true);
      expect(started).toBe(2);
    } finally {
      table.close();
    }
  });

  it('reads names and command lines with spaces and CRLF endings and skips lines that are not rows', () => {
    const table = parseProcessRows(['12\t4\t99\ta b c.exe\tc.exe --flag  two\r', '', 'garbage', '13\t12\t100\tx']);
    expect([...table.keys()]).toEqual([12, 13]);
    expect(table.get(12)?.name).toBe('a b c.exe');
    expect(table.get(12)?.command).toBe('c.exe --flag  two');
    expect(table.get(13)?.command).toBe('');
  });

  it('walks every descendant generation of a root, nearest first', () => {
    const table = parseProcessRows([
      '10\t1\t1\troot\t',
      '11\t10\t1\tchild\t',
      '12\t11\t1\tgrandchild\t',
      '13\t1\t1\tunrelated\t',
    ]);
    expect(descendantsIn(table, 10)).toEqual([11, 12]);
    expect(descendantsIn(table, 13)).toEqual([]);
  });

  it('passes the helper script encoded, so it needs no shell quoting', () => {
    const { command, args } = powershellHelperCommand();
    expect(command).toBe('powershell.exe');
    expect(args.slice(0, 3)).toEqual(['-NoProfile', '-NonInteractive', '-EncodedCommand']);
    const script = Buffer.from(args[3] ?? '', 'base64').toString('utf16le');
    expect(script).toContain('Get-CimInstance Win32_Process');
    expect(script).toContain("[Console]::Out.WriteLine('.')");
    expect(script).toContain('-join [char]9');
  });
});

function spawnSilent(_command, _args, options) {
  return spawn(process.execPath, ['-e', FAKE, 'silent'], options);
}

function spawnAnswering(_command, _args, options) {
  return spawn(process.execPath, ['-e', FAKE, 'answer'], options);
}
