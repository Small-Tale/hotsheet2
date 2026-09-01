import { describe, expect, it } from 'vitest';

import { type CommandError, runCommand, shellQuote, shellQuoteCommand } from './shell';

describe('shell quoting', () => {
  it('leaves shell-safe words unquoted', () => {
    expect(shellQuote('new')).toBe('new');
    expect(shellQuote('--category')).toBe('--category');
    expect(shellQuote('/tmp/store.hs2')).toBe('/tmp/store.hs2');
    expect(shellQuote('HS2-ABC123')).toBe('HS2-ABC123');
  });

  it('quotes empty, whitespace, and metacharacter-bearing arguments', () => {
    expect(shellQuote('')).toBe("''");
    expect(shellQuote('--title=UX feedback: no footer')).toBe("'--title=UX feedback: no footer'");
    expect(shellQuote('1728×971')).toBe("'1728×971'");
    expect(shellQuote('a && rm -rf /')).toBe("'a && rm -rf /'");
    expect(shellQuote('$(whoami)')).toBe("'$(whoami)'");
    expect(shellQuote('back`tick`')).toBe("'back`tick`'");
    expect(shellQuote('line one\nline two')).toBe("'line one\nline two'");
  });

  it("escapes embedded single quotes with the '\\'' idiom", () => {
    expect(shellQuote("it's")).toBe("'it'\\''s'");
    expect(shellQuote("''")).toBe("''\\'''\\'''");
  });

  it('renders a copy-paste-runnable command line', () => {
    expect(
      shellQuoteCommand('/repo/hotsheet-cli', [
        '-C',
        '/repo/store.hs2',
        'new',
        '--title=UX feedback: no need for footer',
        '--category',
        'bug',
      ]),
    ).toBe("/repo/hotsheet-cli -C /repo/store.hs2 new '--title=UX feedback: no need for footer' --category bug");
  });
});

describe('runCommand', () => {
  it('returns decoded stdout/stderr on success', async () => {
    const { stdout } = await runCommand(process.execPath, ['-e', "process.stdout.write('ok')"]);
    expect(stdout).toBe('ok');
  });

  it('throws a shell-quoted, copy-paste-runnable command on failure', async () => {
    let error: CommandError | undefined;
    try {
      await runCommand(process.execPath, ['-e', 'process.exit(3)', 'a b', "x'y"]);
    } catch (caught) {
      error = caught as CommandError;
    }
    expect(error).toBeDefined();
    expect(error!.code).toBe(3);
    expect(error!.command).toContain("'a b'");
    expect(error!.command).toContain("'x'\\''y'");
    expect(error!.message.startsWith('Command failed: ')).toBe(true);
    // The quoted command re-parses to the original argv (no accidental word splitting).
    expect(error!.command).not.toContain('a b x'); // spaces stayed inside their quotes
  });
});
