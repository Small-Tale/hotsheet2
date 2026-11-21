import { spawn } from 'node:child_process';

/**
 * One long-lived PowerShell answers every Windows process-table query of a test file
 * (HS2-S5BQRD). Starting `powershell.exe` for each query cost 0.7-1.0 s on a quiet Windows 11
 * ARM64 VM (a warm `Get-CimInstance Win32_Process` takes 25-60 ms), and several cold starts per
 * poll pushed the owned-server and server-identity fixtures past their timeouts on the hosted
 * arm64 runner. Each request writes one line to the helper's stdin; the helper answers with one
 * tab-separated `pid ppid started name command-line` line per process and a lone `.` line.
 * Concurrent callers share the query in flight.
 */
const SCRIPT = [
  "$ErrorActionPreference = 'Stop'",
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

/**
 * A process table backed by one helper process. `command` and `spawnImpl` are injectable so the
 * line protocol, restart, and timeout are tested off Windows.
 */
export function createProcessTable({
  command = powershellHelperCommand(),
  spawnImpl = spawn,
  timeoutMs = 20_000,
} = {}) {
  let helper;
  let inFlight;

  const start = () => {
    const child = spawnImpl(command.command, command.args, { stdio: ['pipe', 'pipe', 'ignore'], windowsHide: true });
    const state = { child, buffer: '', lines: [], waiter: undefined, failed: undefined };
    const fail = (error) => {
      state.failed ??= error;
      if (helper === state) helper = undefined;
      state.waiter?.reject(state.failed);
      state.waiter = undefined;
    };
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (chunk) => {
      state.buffer += chunk;
      let end;
      while ((end = state.buffer.indexOf('\n')) >= 0) {
        const line = state.buffer.slice(0, end).replace(/\r$/u, '');
        state.buffer = state.buffer.slice(end + 1);
        if (line !== '.') {
          state.lines.push(line);
          continue;
        }
        const rows = state.lines.splice(0);
        state.waiter?.resolve(parseProcessRows(rows));
        state.waiter = undefined;
      }
    });
    child.once('error', fail);
    child.once('exit', (code) => fail(new Error(`The Windows process-table helper exited (${code}).`)));
    child.stdin.on('error', fail);
    // An idle helper never keeps the test process alive; a pending query holds its own timer.
    child.unref();
    child.stdout.unref?.();
    child.stdin.unref?.();
    return state;
  };

  const query = () => {
    helper ??= start();
    const state = helper;
    return new Promise((resolveQuery, rejectQuery) => {
      const timer = setTimeout(() => {
        state.waiter = undefined;
        if (helper === state) helper = undefined;
        state.child.kill();
        rejectQuery(new Error(`The Windows process-table helper did not answer within ${timeoutMs} ms.`));
      }, timeoutMs);
      state.waiter = {
        resolve: (table) => {
          clearTimeout(timer);
          resolveQuery(table);
        },
        reject: (error) => {
          clearTimeout(timer);
          rejectQuery(error);
        },
      };
      if (state.failed) state.waiter.reject(state.failed);
      else state.child.stdin.write('query\n');
    });
  };

  return {
    /** Every process now running, keyed by pid. Callers that ask while a query runs share it. */
    snapshot() {
      inFlight ??= query().finally(() => {
        inFlight = undefined;
      });
      return inFlight;
    },
    /** End the helper; the next snapshot starts a new one. */
    close() {
      const state = helper;
      helper = undefined;
      state?.child.stdin.end();
    },
  };
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
