import { spawnSync } from 'node:child_process';

import { describe, expect, it } from 'vitest';

import { fixtureShell, printfOutput, printThenEcho, printThenReportSize } from '../tests/terminal-fixture-command.ts';

const OSC7 = '\\033]7;file://localhost/work/demo\\007';

describe('portable terminal fixture commands (HS2-TMZ13Z)', () => {
  it('selects /bin/sh on Unix, Node on Windows, and Node when forced', () => {
    expect(fixtureShell('darwin', {})).toBe('posix');
    expect(fixtureShell('linux', {})).toBe('posix');
    expect(fixtureShell('win32', {})).toBe('node');
    expect(fixtureShell('linux', { HOTSHEET_TEST_TERMINAL_FIXTURE: 'node' })).toBe('node');
    expect(fixtureShell('win32', { HOTSHEET_TEST_TERMINAL_FIXTURE: 'posix' })).toBe('node');
  });

  it('keeps the exact Unix commands the specs always ran', () => {
    expect(printThenEcho(OSC7, 'posix')).toEqual({
      command: '/bin/sh',
      args: ['-c', 'printf "\\033]7;file://localhost/work/demo\\007"; exec cat'],
    });
    expect(printThenEcho('', 'posix')).toEqual({ command: '/bin/sh', args: ['-c', 'exec cat'] });
    expect(printThenReportSize('ZOOM REFIT\\r\\n', 300, 'posix')).toEqual({
      command: '/bin/sh',
      args: ['-c', 'printf "ZOOM REFIT\\r\\n"; while true; do stty size; sleep 0.3; done'],
    });
  });

  it('decodes the printf escapes the specs use', () => {
    expect(printfOutput(OSC7)).toBe('\x1b]7;file://localhost/work/demo\x07');
    expect(printfOutput('\\033[2J\\033[HREPLAY\\r\\n')).toBe('\x1b[2J\x1b[HREPLAY\r\n');
    expect(printfOutput('a\\tb\\\\c\\0')).toBe('a\tb\\c\0');
  });

  it('builds Node scripts without characters Windows command-line quoting rewrites', () => {
    for (const { command, args } of [
      printThenEcho(OSC7, 'node', 'C:\\Program Files\\nodejs\\node.exe'),
      printThenEcho('', 'node', 'node'),
      printThenReportSize('ZOOM REFIT\\r\\n', 300, 'node', 'node'),
    ]) {
      expect(args[0]).toBe('-e');
      expect(args[1]).not.toMatch(/["\\]/);
      expect(command).toMatch(/node(\.exe)?$/);
    }
  });

  it('writes the printf output and then echoes its input like exec cat', () => {
    const { command, args } = printThenEcho('LIVE READY\\r\\n', 'node');
    const result = spawnSync(command, args, { input: 'typed keys\n', encoding: 'utf8' });
    expect(result.status, result.stderr).toBe(0);
    expect(result.stdout).toBe('LIVE READY\r\ntyped keys\n');
    const silent = printThenEcho('', 'node');
    expect(spawnSync(silent.command, silent.args, { input: 'x', encoding: 'utf8' }).stdout).toBe('x');
  });
});
