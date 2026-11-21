import { spawn } from 'node:child_process';

/**
 * One long-lived PowerShell answers every Windows process-table query of a test file
 * (HS2-S5BQRD). Starting `powershell.exe` for each query cost 0.7-1.0 s on a quiet Windows 11
 * ARM64 VM (a warm `Get-CimInstance Win32_Process` takes 25-60 ms), and several cold starts per
 * poll pushed the owned-server and server-identity fixtures past their timeouts on the hosted
 * arm64 runner. Each request writes one line to the helper's stdin; the helper answers with one
 * tab-separated `pid ppid started name command-line` line per process and a lone `.` line.
 * It prints `#ready` once PowerShell has started, so a timeout can say which stage is slow.
 * Concurrent callers share the query in flight.
 */
const SCRIPT = [
  "$ErrorActionPreference = 'Stop'",
  "[Console]::Out.WriteLine('#ready')",
  '[Console]::Out.Flush()',
  'while ($null -ne [Console]::In.ReadLine()) {',
  '  foreach ($row in Get-CimInstance Win32_Process) {',
  '    $started = if ($row.CreationDate) { $row.CreationDate.ToFileTimeUtc() } else { 0 }',
  "    $command = if ($row.CommandLine) { $row.CommandLine -replace '[\\t\\r\\n]', ' ' } else { '' }",
  '    [Console]::Out.WriteLine((@($row.ProcessId, $row.ParentProcessId, $started, $row.Name, $command) -join [char]9))',
  '  }',
  "  [Console]::Out.WriteLine('.')",
  '  [Console]::Out.Flush()',
  '}',
].join('\n');

/** The PowerShell command line of the helper; the script is passed encoded, so no quoting applies. */
export function powershellHelperCommand() {
  return {
    command: 'powershell.exe',
    args: ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(SCRIPT, 'utf16le').toString('base64')],
  };
}

/** Parse one answer (the lines before the `.` terminator) into a pid-keyed table. */
export function parseProcessRows(lines) {
  const table = new Map();
  for (const line of lines) {
    const match = /^(\d+)\t(\d+)\t(\d+)\t([^\t]*)\t?(.*)$/u.exec(line.replace(/\r$/u, ''));
    if (!match) continue;
    const [, pid, ppid, started, name, command] = match;
    table.set(Number(pid), { pid: Number(pid), ppid: Number(ppid), started, name, command });
  }
  return table;
}

const STDERR_TAIL = 2_000;

/**
 * A process table backed by one helper process. `command` and `spawnImpl` are injectable so the
 * line protocol, restart, and timeout are tested off Windows.
 *
 * A caller that waits longer than `timeoutMs` gets an error naming the helper's stage, but the
 * helper keeps working on that query: the next caller shares its answer (HS2-S5BQRD follow-up).
 * Killing a slow helper made every later query pay the PowerShell cold start again, so a host on
 * which the first answer takes longer than the bound never answered at all. Only a query that
 * stays unanswered past `restartAfterMs` replaces the helper.
 */
export function createProcessTable({
  command = powershellHelperCommand(),
  spawnImpl = spawn,
  timeoutMs = 20_000,
  restartAfterMs = 120_000,
  now = () => Date.now(),
} = {}) {
  let helper;

  const start = () => {
    const child = spawnImpl(command.command, command.args, { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
    const state = {
      child,
      buffer: '',
      lines: [],
      pending: undefined,
      failed: undefined,
      startedAt: now(),
      readyAt: 0,
      stderr: '',
    };
    const fail = (error) => {
      state.failed ??= error;
      if (helper === state) helper = undefined;
      state.pending?.reject(state.failed);
      state.pending = undefined;
    };
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (chunk) => {
      state.buffer += chunk;
      let end;
      while ((end = state.buffer.indexOf('\n')) >= 0) {
        const line = state.buffer.slice(0, end).replace(/\r$/u, '');
        state.buffer = state.buffer.slice(end + 1);
        if (line === '#ready') {
          state.readyAt ||= now();
          continue;
        }
        if (line !== '.') {
          state.lines.push(line);
          continue;
        }
        const rows = state.lines.splice(0);
        state.pending?.resolve(parseProcessRows(rows));
        state.pending = undefined;
      }
    });
    child.stderr?.setEncoding('utf8');
    child.stderr?.on('data', (chunk) => {
      state.stderr = (state.stderr + chunk).slice(-STDERR_TAIL);
    });
    child.once('error', fail);
    child.once('exit', (code) =>
      fail(new Error(`The Windows process-table helper exited (${code}).${stderrNote(state)}`)),
    );
    child.stdin.on('error', fail);
    // An idle helper never keeps the test process alive; a pending query holds its own timer.
    child.unref();
    child.stdout.unref?.();
    child.stderr?.unref?.();
    child.stdin.unref?.();
    return state;
  };

  /** The answer to the helper's current query, asking a new one only when none is outstanding. */
  const answer = (state) => {
    if (state.pending) return state.pending.promise;
    const pending = { askedAt: now() };
    pending.promise = new Promise((resolveAnswer, rejectAnswer) => {
      pending.resolve = resolveAnswer;
      pending.reject = rejectAnswer;
    });
    // A caller that timed out leaves this promise unobserved; its rejection is reported to the next caller.
    pending.promise.catch(() => {});
    state.pending = pending;
    if (state.failed) pending.reject(state.failed);
    else state.child.stdin.write('query\n');
    return pending.promise;
  };

  const describe = (state) => {
    const age = now() - state.startedAt;
    const stage = state.readyAt
      ? `PowerShell started after ${state.readyAt - state.startedAt} ms; the query has run ` +
        `${now() - (state.pending?.askedAt ?? now())} ms with ${state.lines.length} rows so far`
      : `PowerShell has not started after ${age} ms`;
    return `${stage}.${stderrNote(state)}`;
  };

  const query = () => {
    if (helper?.pending && now() - helper.pending.askedAt > restartAfterMs) {
      const stale = helper;
      helper = undefined;
      stale.pending.reject(new Error(`The Windows process-table helper was replaced: ${describe(stale)}`));
      stale.pending = undefined;
      stale.child.kill();
    }
    helper ??= start();
    const state = helper;
    return new Promise((resolveQuery, rejectQuery) => {
      const timer = setTimeout(() => {
        rejectQuery(
          new Error(`The Windows process-table helper did not answer within ${timeoutMs} ms: ${describe(state)}`),
        );
      }, timeoutMs);
      answer(state).then(
        (table) => {
          clearTimeout(timer);
          resolveQuery(table);
        },
        (error) => {
          clearTimeout(timer);
          rejectQuery(error);
        },
      );
    });
  };

  return {
    /** Every process now running, keyed by pid. Callers that ask while a query runs share it. */
    snapshot: query,
    /**
     * Start the helper and wait for its first answer, up to `waitMs`, so a test's own polls start
     * warm. Use it in a `beforeAll` whose timeout covers `waitMs`.
     */
    async warm(waitMs = restartAfterMs) {
      helper ??= start();
      const state = helper;
      let timer;
      try {
        await Promise.race([
          answer(state),
          new Promise((_, rejectWarm) => {
            timer = setTimeout(
              () =>
                rejectWarm(
                  new Error(`The Windows process-table helper did not warm within ${waitMs} ms: ${describe(state)}`),
                ),
              waitMs,
            );
          }),
        ]);
      } finally {
        clearTimeout(timer);
      }
    },
    /** End the helper; the next snapshot starts a new one. */
    close() {
      const state = helper;
      helper = undefined;
      state?.child.stdin.end();
    },
  };
}

function stderrNote(state) {
  const text = state.stderr.trim();
  return text ? ` Helper stderr: ${text}` : '';
}

let shared;

/** The process table shared by this test file's Windows fixtures. */
export function windowsProcessTable() {
  shared ??= createProcessTable();
  return shared;
}

/** Every process whose parent chain leads to `rootPid` in `table`, nearest first. */
export function descendantsIn(table, rootPid) {
  const found = [rootPid];
  for (let index = 0; index < found.length; index += 1)
    for (const row of table.values()) if (row.ppid === found[index] && !found.includes(row.pid)) found.push(row.pid);
  return found.slice(1);
}
