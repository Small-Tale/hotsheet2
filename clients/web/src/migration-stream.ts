import { spawn, spawnSync } from 'node:child_process';
import { StringDecoder } from 'node:string_decoder';

import type { MigrationProgress } from './migration-progress';

/** Decode split UTF-8, CR updates, coalesced records, and a final unterminated line. */
export function lineDecoder(onLine: (line: string) => void) {
  const decoder = new StringDecoder('utf8');
  let pending = '';
  const consume = (text: string) => {
    pending += text;
    const lines = pending.split(/[\r\n]+/);
    pending = lines.pop() ?? '';
    for (const line of lines) {
      if (line.length > 1024 * 1024) throw new Error('Migration progress record exceeds 1 MB.');
      if (line) onLine(line);
    }
    if (pending.length > 1024 * 1024) throw new Error('Migration progress record exceeds 1 MB.');
  };
  return {
    push: (chunk: Buffer) => {
      consume(decoder.write(chunk));
    },
    end: () => {
      consume(decoder.end());
      if (pending) onLine(pending);
      pending = '';
    },
  };
}
export function parseMigrationProgress(
  line: string,
): MigrationProgress & { result?: { tickets: number; attachments: number }; error?: string } {
  const event = JSON.parse(line) as Omit<MigrationProgress, 'version'> & {
    version: number;
    result?: { tickets: number; attachments: number };
    error?: string;
  };
  if (
    event.version !== 1 ||
    typeof event.phase !== 'string' ||
    [event.completed, event.total].some((value) => value !== undefined && (!Number.isFinite(value) || value < 0))
  )
    throw new Error('Unsupported migration progress record.');
  return { ...event, version: 1 };
}
export function parseGitProgress(line: string): MigrationProgress | undefined {
  const match = line.match(/(Enumerating|Counting|Compressing|Writing) objects:\s*(?:\d+%\s*\((\d+)\/(\d+)\)|(\d+))/);
  if (!match) return undefined;
  const phase = (
    {
      Enumerating: 'enumerate_objects',
      Counting: 'count_objects',
      Compressing: 'compress_objects',
      Writing: 'write_objects',
    } as Record<string, string>
  )[match[1]];
  const bytes = line.match(/,\s*([\d.]+)\s*(bytes|[KMGT]?i?B)(?:\s*\|\s*([\d.]+)\s*([KMGT]?i?B)\/s)?/i);
  const quantity = (value: string, unit: string) =>
    Number(value) * 1024 ** Math.max(0, ['B', 'K', 'M', 'G', 'T'].indexOf(unit[0].toUpperCase()));
  return {
    version: 1,
    phase,
    ...(match[2] ? { completed: Number(match[2]), total: Number(match[3]), unit: 'objects' } : {}),
    ...(bytes
      ? {
          transferredBytes: quantity(bytes[1], bytes[2]),
          ...(bytes[3] ? { bytesPerSecond: quantity(bytes[3], bytes[4]) } : {}),
        }
      : {}),
  };
}

/** Child lifetime belongs to the bridge; an HTTP subscriber never owns its signal. */
export function runMigrationProcess(
  command: string,
  args: string[],
  cwd: string,
  onLine: (line: string) => void,
  gitProgress = false,
  childOwner?: (pid: number) => Promise<void>,
): Promise<void> {
  return new Promise((resolveRun, reject) => {
    // The wrapper cannot execute work until the parent checkpoints its PID and
    // opens stdin. Parent death before acknowledgment closes stdin and exits.
    const gate = `const {spawn}=require('node:child_process');let started=false;process.stdin.once('data',()=>{started=true;const child=spawn(process.argv[1],process.argv.slice(2),{stdio:['ignore','inherit','inherit']});child.on('error',error=>{console.error(error.message);process.exitCode=1});child.on('exit',(code)=>{process.exitCode=code??1})});process.stdin.once('end',()=>{if(!started)process.exitCode=1});`;
    const child = spawn(process.execPath, ['-e', gate, command, ...args], {
      cwd,
      detached: process.platform !== 'win32',
      stdio: ['pipe', 'pipe', 'pipe'],
      env: { ...process.env, LC_ALL: 'C', GIT_TERMINAL_PROMPT: '0' },
    });
    const killGroup = (signal: NodeJS.Signals) => {
      if (!child.pid) return;
      try {
        if (process.platform === 'win32')
          spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore', timeout: 10_000 });
        else process.kill(-child.pid, signal);
      } catch {
        /* Already exited. */
      }
    };
    let forceStop: ReturnType<typeof setTimeout> | undefined;
    const stopChild = () => {
      killGroup('SIGTERM');
      forceStop ??= setTimeout(() => {
        killGroup('SIGKILL');
      }, 200);
    };
    const exiting = () => {
      killGroup('SIGKILL');
    };
    process.once('exit', exiting);
    let tail = '',
      failed: Error | undefined;
    const stdout = lineDecoder(onLine),
      stderr = lineDecoder((line) => {
        tail = `${tail}\n${line}`.slice(-16_384);
        if (gitProgress) onLine(line);
      });
    const consume = (decoder: ReturnType<typeof lineDecoder>, chunk: Buffer) => {
      try {
        decoder.push(chunk);
      } catch (error) {
        failed = error instanceof Error ? error : new Error(String(error));
        stopChild();
      }
    };
    child.stdout.on('data', (chunk: Buffer) => {
      consume(stdout, chunk);
    });
    child.stderr.on('data', (chunk: Buffer) => {
      consume(stderr, chunk);
    });
    child.once('error', reject);
    child.once('spawn', () => {
      void (async () => {
        if (child.pid) await childOwner?.(child.pid);
        child.stdin.end('start\n');
      })().catch((error: unknown) => {
        failed = error instanceof Error ? error : new Error(String(error));
        stopChild();
      });
    });
    child.once('close', (code) => {
      process.removeListener('exit', exiting);
      if (forceStop) {
        clearTimeout(forceStop);
        killGroup('SIGKILL');
      }
      try {
        stdout.end();
        stderr.end();
      } catch (error) {
        failed = error instanceof Error ? error : new Error(String(error));
      }
      void (async () => {
        await childOwner?.(0);
        if (failed) reject(failed);
        else if (code !== 0) reject(new Error(tail.trim() || `${command} exited with status ${code ?? 'unknown'}.`));
        else resolveRun();
      })().catch(reject);
    });
  });
}
