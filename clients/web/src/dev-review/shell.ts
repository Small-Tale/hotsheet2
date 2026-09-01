import { execFile, type ExecFileOptions } from 'node:child_process';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);

/** Characters that never need quoting in a POSIX shell word. */
const SHELL_SAFE = /^[A-Za-z0-9_@%+=:,./-]+$/;

/**
 * Quote a single argument so a POSIX shell parses it back as exactly this string.
 *
 * Empty and metacharacter-bearing values are wrapped in single quotes; embedded
 * single quotes are escaped with the `'\''` idiom. Newlines survive because they
 * are literal inside single quotes.
 */
export function shellQuote(argument: string): string {
  if (argument === '') return "''";
  if (SHELL_SAFE.test(argument)) return argument;
  return `'${argument.replaceAll("'", "'\\''")}'`;
}

/** Render a command plus argv as a single copy-paste-runnable shell command line. */
export function shellQuoteCommand(file: string, args: readonly string[]): string {
  return [file, ...args].map(shellQuote).join(' ');
}

/** Error thrown by {@link runCommand}, carrying the shell-quoted command line. */
export interface CommandError extends Error {
  /** The failed command, shell-quoted so it is copy-paste runnable. */
  command: string;
  code?: number | string;
  stderr: string;
}

/**
 * Run a command via `execFile` (no shell). On failure, throw a {@link CommandError}
 * whose message embeds the *shell-quoted* command so it can be copied straight into a
 * terminal — unlike Node's default space-joined `Command failed:` message, which
 * mangles any argument containing spaces or shell metacharacters.
 */
export async function runCommand(
  file: string,
  args: string[],
  options?: ExecFileOptions,
): Promise<{ stdout: string; stderr: string }> {
  const command = shellQuoteCommand(file, args);
  try {
    const { stdout, stderr } = await execFileAsync(file, args, options);
    return { stdout: stdout.toString(), stderr: stderr.toString() };
  } catch (cause) {
    const detail = cause as { code?: number | string; stderr?: string | Buffer };
    const stderr = detail.stderr ? detail.stderr.toString() : '';
    const error = new Error(
      `Command failed: ${command}${stderr.trim() ? `\n${stderr.trim()}` : ''}`,
    ) as CommandError;
    error.command = command;
    error.code = detail.code;
    error.stderr = stderr;
    throw error;
  }
}
