/**
 * Synthetic PTY commands for real-server terminal specs (HS2-TMZ13Z).
 *
 * Unix runs the exact `/bin/sh -c 'printf …; exec cat'` fixtures the specs always used. Windows has
 * no `/bin/sh` for `CreateProcessW`, so the same behavior runs in the Node binary that runs the
 * spec: write the printf output, then echo the PTY's input (like `exec cat`) or report the PTY
 * size on an interval (like `stty size`). The Node script holds no `"` or `\`, so it survives
 * Windows command-line quoting unchanged; its output travels as base64.
 */

/** A `POST /terminals` command and argument list. */
export interface FixtureTerminalCommand {
  command: string;
  args: string[];
}

/** `posix` runs `/bin/sh`; `node` runs the Node binary (Windows, or forced for a local check). */
export type FixtureShell = 'posix' | 'node';

/**
 * The fixture shell for this host. `HOTSHEET_TEST_TERMINAL_FIXTURE=node` forces the Node fixture on
 * Unix, so the Windows command shape can be exercised by the same specs on any host.
 */
export function fixtureShell(
  platform: NodeJS.Platform = process.platform,
  environment: Readonly<Record<string, string | undefined>> = process.env,
): FixtureShell {
  if (environment.HOTSHEET_TEST_TERMINAL_FIXTURE === 'node') return 'node';
  return platform === 'win32' ? 'node' : 'posix';
}

/** The bytes a `printf` format writes, for the escapes the specs use: octal, `\r`, `\n`, `\\`. */
export function printfOutput(format: string): string {
  return format.replace(/\\([0-7]{1,3}|[rnt\\])/g, (_, escape: string) => {
    if (escape === 'r') return '\r';
    if (escape === 'n') return '\n';
    if (escape === 't') return '\t';
    if (escape === '\\') return '\\';
    return String.fromCharCode(Number.parseInt(escape, 8));
  });
}

function nodeCommand(script: string, node: string): FixtureTerminalCommand {
  return { command: node, args: ['-e', script] };
}

function writeScript(format: string): string {
  if (!format) return '';
  return `process.stdout.write(Buffer.from('${Buffer.from(printfOutput(format), 'utf8').toString('base64')}','base64'));`;
}

/** `printf "<format>"; exec cat`: write the output, then echo whatever the PTY sends. */
export function printThenEcho(
  format: string,
  shell: FixtureShell = fixtureShell(),
  node: string = process.execPath,
): FixtureTerminalCommand {
  if (shell === 'posix')
    return { command: '/bin/sh', args: ['-c', format ? `printf "${format}"; exec cat` : 'exec cat'] };
  return nodeCommand(`${writeScript(format)}process.stdin.pipe(process.stdout);`, node);
}

/** `printf "<format>"; while true; do stty size; sleep <s>; done`: report `rows cols` lines. */
export function printThenReportSize(
  format: string,
  intervalMs: number,
  shell: FixtureShell = fixtureShell(),
  node: string = process.execPath,
): FixtureTerminalCommand {
  if (shell === 'posix')
    return {
      command: '/bin/sh',
      args: ['-c', `printf "${format}"; while true; do stty size; sleep ${intervalMs / 1000}; done`],
    };
  return nodeCommand(
    `${writeScript(format)}setInterval(()=>{const [c,r]=process.stdout.getWindowSize();process.stdout.write(r+' '+c+String.fromCharCode(10))},${intervalMs});`,
    node,
  );
}
