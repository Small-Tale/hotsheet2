// HS2-BND42N: one local main writer from preflight through gate, push and confirmation.
import { randomUUID, createHash } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, readFileSync, realpathSync, statSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseMainPushArgs } from './main-integration-options.mjs';

const project = resolve(fileURLToPath(new URL('..', import.meta.url)));
const ref = 'refs/hotsheet/main-integration-window';
const zero = '0'.repeat(40);
const leaseMs = 120_000;
const heartbeatMs = 20_000;
const fullSha = /^[0-9a-f]{40}$/i;
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

function git(repo, args, input) {
  const result = spawnSync('git', args, {
    cwd: repo,
    input,
    encoding: 'utf8',
    timeout: 10_000,
    maxBuffer: 1024 * 1024,
  });
  if (result.error || result.status !== 0)
    throw new Error(`git ${args[0]} failed: ${result.error?.message ?? result.stderr.trim()}`);
  return result.stdout.trim();
}

function gitResult(repo, args) {
  return spawnSync('git', args, { cwd: repo, encoding: 'utf8', timeout: 10_000 });
}

function commonDir(repo) {
  return realpathSync(resolve(repo, git(repo, ['rev-parse', '--git-common-dir'])));
}

function snapshot(repo, requested = 'HEAD') {
  const head = git(repo, ['rev-parse', 'HEAD']);
  const tree = git(repo, ['rev-parse', 'HEAD^{tree}']);
  const selected = git(repo, ['rev-parse', requested]);
  if (head !== selected) throw new Error('Push source is no longer the checked-out HEAD');
  if (git(repo, ['status', '--porcelain', '--untracked-files=normal']))
    throw new Error('Main integration requires a clean checkout and exact-tree gate receipt');
  return { head, tree };
}

function readWindow(repo) {
  const resolved = gitResult(repo, ['show-ref', '--verify', '--quiet', ref]);
  if (resolved.status === 1) return null;
  if (resolved.error || resolved.status !== 0) throw new Error('Cannot read main integration ref');
  const oid = git(repo, ['rev-parse', ref]);
  const value = JSON.parse(git(repo, ['cat-file', 'blob', oid]));
  if (
    value.version !== 1 ||
    !value.token ||
    !Number.isInteger(value.pid) ||
    value.pid <= 0 ||
    !Number.isFinite(value.expiresAt) ||
    !['gating', 'ready'].includes(value.phase) ||
    !fullSha.test(value.head) ||
    !fullSha.test(value.tree) ||
    !/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(value.remote) ||
    value.target !== 'refs/heads/main' ||
    (value.phase === 'ready' &&
      (!Array.isArray(value.gateArgs) ||
        !value.gateArgs.every((arg) => typeof arg === 'string') ||
        !(value.oldTip === null || fullSha.test(value.oldTip))))
  )
    throw new Error('Invalid main integration record; inspect the Git ref before recovery');
  return { oid, value };
}

function writeWindow(repo, oldOid, value) {
  const blob = git(repo, ['hash-object', '-w', '--stdin'], `${JSON.stringify(value)}\n`);
  const result = gitResult(repo, ['-c', 'core.filesRefLockTimeout=1000', 'update-ref', ref, blob, oldOid ?? zero]);
  if (result.status === 0) return { oid: blob, value };
  if ((readWindow(repo)?.oid ?? null) !== (oldOid ?? null)) return null;
  throw new Error(`Cannot update main integration ref: ${result.stderr?.trim() || 'unknown Git error'}`);
}

function deleteWindow(repo, window) {
  const result = gitResult(repo, ['-c', 'core.filesRefLockTimeout=1000', 'update-ref', '-d', ref, window.oid]);
  if (result.status !== 0 && readWindow(repo)?.oid === window.oid)
    throw new Error('Cannot release main integration ref');
}

function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error.code === 'EPERM';
  }
}

function assertOwner(repo, token, phase) {
  const current = readWindow(repo);
  if (
    !current ||
    current.value.token !== token ||
    current.value.expiresAt <= Date.now() ||
    !alive(current.value.pid) ||
    (phase && current.value.phase !== phase)
  )
    throw new Error('Main integration reservation was lost or expired');
  return current;
}

function renew(repo, token) {
  const current = assertOwner(repo, token);
  const updated = writeWindow(repo, current.oid, { ...current.value, expiresAt: Date.now() + leaseMs });
  if (!updated) throw new Error('Main integration reservation changed during renewal');
  return updated;
}

async function acquire(repo, options) {
  const token = randomUUID();
  const started = Date.now();
  while (Date.now() - started < 10 * 60_000) {
    const current = readWindow(repo);
    if (!current || current.value.expiresAt <= Date.now()) {
      const selected = snapshot(repo, options.source);
      const written = writeWindow(repo, current?.oid, {
        version: 1,
        token,
        pid: process.pid,
        expiresAt: Date.now() + leaseMs,
        phase: 'gating',
        remote: options.remote,
        target: options.target,
        head: selected.head,
        tree: selected.tree,
        gateArgs: options.gateArgs,
        identity: options.identity,
        assignment: options.assignment,
        standalone: options.standalone,
        sourceStore: options.sourceStore,
      });
      if (written) return written;
    }
    await sleep(1000);
  }
  throw new Error('Timed out waiting for the local main integration window');
}

function assertSnapshot(repo, value) {
  const current = snapshot(repo, value.head);
  if (current.head !== value.head || current.tree !== value.tree)
    throw new Error('Checked-out main changed during integration');
}

export function authorizeMainState(state, value, projectId, sourceId, now) {
  if (!state) {
    if (value.identity) throw new Error('No coordinator pool for integrator push');
    return 'standalone';
  }
  if (
    !Number.isInteger(state.version) ||
    state.version < 3 ||
    state.version > 6 ||
    state.project_id !== projectId ||
    state.source_id !== sourceId
  )
    throw new Error('Incompatible coordination pool or ticket source');
  const live = state.integrator && state.integrator.expires_at > now;
  const fence = Object.values(state.assignments ?? {}).filter((item) =>
    ['verifying', 'integrating', 'suspended_integration'].includes(item.phase),
  );
  if (live) {
    if (
      !value.identity ||
      value.standalone ||
      state.integrator.id !== value.identity.id ||
      state.integrator.generation !== value.identity.session
    )
      throw new Error('Live integrator owns main; supply its exact id and session');
    if (value.assignment) {
      if (
        fence.length !== 1 ||
        fence[0].id !== value.assignment ||
        fence[0].phase !== 'integrating' ||
        !(fence[0].handoff?.kind === 'code' || fence[0].commit)
      )
        throw new Error('Assignment is not the sole integrating code handoff');
    } else if (fence.length) throw new Error('Specify the active integrating code assignment');
    return 'integrator';
  }
  if (value.identity) throw new Error('Integrator session expired or was replaced');
  if (fence.length) throw new Error('An integration/review fence needs recovery before standalone push');
  if (!value.standalone) throw new Error('Existing pool requires explicit --standalone without a live integrator');
  return 'standalone';
}

function authority(repo, value) {
  const common = commonDir(repo);
  const directory = join(common, 'hotsheet-coordination');
  if (!existsSync(directory)) return authorizeMainState(null, value);
  const pool = join(directory, 'state.json');
  if (!statSync(directory).isDirectory() || !existsSync(pool))
    throw new Error('Coordination pool is partial; repair it before pushing main');
  const state = JSON.parse(readFileSync(pool, 'utf8'));
  const shipping = realpathSync(resolve(common, '..'));
  const slug = basename(shipping)
    .replace(/[^A-Za-z0-9]/g, '-')
    .toLowerCase()
    .replace(/^-+|-+$/g, '');
  const projectId = `${slug}-${createHash('sha256').update(shipping).digest('hex').slice(0, 12)}`;
  const sourceId = createHash('sha256').update(realpathSync(value.sourceStore)).digest('hex');
  return authorizeMainState(state, value, projectId, sourceId, Math.floor(Date.now() / 1000));
}

async function child(repo, program, args, { env = process.env, signal, output = false, timeoutMs } = {}) {
  return new Promise((done, reject) => {
    if (signal?.aborted) {
      reject(new Error('Integration command canceled before launch'));
      return;
    }
    const processChild = spawn(program, args, {
      cwd: repo,
      env,
      detached: true,
      stdio: ['inherit', output ? 'pipe' : 'inherit', output ? 'pipe' : 'inherit'],
    });
    let stdout = '';
    let stderr = '';
    let failure;
    let killTimer;
    const stop = () => {
      if (processChild.pid && processChild.exitCode === null) {
        try {
          process.kill(-processChild.pid, 'SIGTERM');
        } catch {
          /* already exited */
        }
        killTimer ??= setTimeout(() => {
          if (processChild.exitCode === null)
            try {
              process.kill(-processChild.pid, 'SIGKILL');
            } catch {
              /* already exited */
            }
        }, 5000);
      }
    };
    const deadline =
      timeoutMs &&
      setTimeout(() => {
        failure = new Error(`${program} ${args[0]} exceeded ${timeoutMs} ms`);
        stop();
      }, timeoutMs);
    if (output) {
      processChild.stdout.on('data', (chunk) => {
        if (stdout.length <= 1024 * 1024) stdout += chunk;
        if (stdout.length > 1024 * 1024 && !failure) {
          failure = new Error('Command output exceeded 1 MiB');
          stop();
        }
      });
      processChild.stderr.on('data', (chunk) => {
        if (stderr.length <= 1024 * 1024) stderr += chunk;
        if (stderr.length > 1024 * 1024 && !failure) {
          failure = new Error('Command error output exceeded 1 MiB');
          stop();
        }
      });
    }
    signal?.addEventListener('abort', stop, { once: true });
    processChild.once('error', reject);
    processChild.once('close', (code) => {
      if (deadline) clearTimeout(deadline);
      if (killTimer) clearTimeout(killTimer);
      signal?.removeEventListener('abort', stop);
      if (failure) reject(failure);
      else if (code === 0 && !signal?.aborted) done(stdout.trim());
      else reject(new Error(`${program} ${args[0]} failed (${code}): ${stderr.trim()}`));
    });
  });
}

async function remoteTip(repo, remote, signal) {
  const line = await child(repo, 'git', ['ls-remote', '--heads', remote, 'main'], {
    output: true,
    signal,
    timeoutMs: 120_000,
  });
  const tip = line ? line.split(/\s+/)[0] : null;
  if (tip && !fullSha.test(tip)) throw new Error('Remote main returned an invalid SHA');
  return tip;
}

async function preflight(repo, value, signal) {
  assertSnapshot(repo, value);
  const fetchUrls = git(repo, ['remote', 'get-url', '--all', value.remote]).split('\n');
  const pushUrls = git(repo, ['remote', 'get-url', '--push', '--all', value.remote]).split('\n');
  if (fetchUrls.length !== 1 || pushUrls.length !== 1 || fetchUrls[0] !== pushUrls[0])
    throw new Error('Main reservation requires one matching fetch/push URL for the named remote');
  const tip = await remoteTip(repo, value.remote, signal);
  if (tip) {
    if (gitResult(repo, ['cat-file', '-e', `${tip}^{commit}`]).status !== 0)
      await child(repo, 'git', ['fetch', '--no-tags', '--no-write-fetch-head', value.remote, 'main'], {
        signal,
        timeoutMs: 120_000,
      });
    if (gitResult(repo, ['merge-base', '--is-ancestor', tip, value.head]).status !== 0)
      throw new Error('Remote main is not an ancestor of the checked-out commit; fetch and integrate first');
  }
  return tip;
}

async function ticketPhase(repo, options, phase, signal) {
  const index = options.gateArgs.indexOf('--ticket');
  if (index < 0) return;
  await child(
    repo,
    process.env.HOTSHEET_MAIN_WINDOW_CLI ?? 'hotsheet-cli',
    [
      'edit',
      options.gateArgs[index + 1],
      '-C',
      options.sourceStore,
      '--started-phase',
      phase,
      '--actor-role',
      'ai',
      '--actor-id',
      process.env.HOTSHEET_WORKER_ID ?? options.identity?.id ?? 'hotsheet-main-window',
    ],
    { signal, timeoutMs: 30_000 },
  );
}

export function verifyMainPush(repo, token, remote, target, localSha, remoteSha) {
  const current = assertOwner(repo, token, 'ready');
  const value = current.value;
  if (
    value.remote !== remote ||
    value.target !== target ||
    value.head !== localSha ||
    (value.oldTip ?? zero) !== remoteSha
  )
    throw new Error(
      `Push ref update does not match the reserved main integration: ${JSON.stringify({
        expected: [value.remote, value.target, value.head, value.oldTip ?? zero],
        received: [remote, target, localSha, remoteSha],
      })}`,
    );
  assertSnapshot(repo, value);
  authority(repo, value);
  return value;
}

export async function pushMain(repo, args) {
  const options = parseMainPushArgs(args);
  options.sourceStore = resolve(options.sourceStore ?? repo);
  const hooks = git(repo, ['config', '--get', 'core.hooksPath']);
  if (realpathSync(resolve(repo, hooks)) !== realpathSync(resolve(repo, '.githooks')))
    throw new Error('Run scripts/install-git-hooks.sh before main push');
  let window = await acquire(repo, options);
  const token = window.value.token;
  const control = new AbortController();
  const onSignal = () => control.abort();
  process.once('SIGINT', onSignal);
  process.once('SIGTERM', onSignal);
  let heartbeatError;
  const heartbeat = setInterval(() => {
    try {
      window = renew(repo, token);
      authority(repo, window.value);
    } catch (error) {
      heartbeatError = error;
      control.abort();
    }
  }, heartbeatMs);
  const check = () => {
    if (heartbeatError) throw heartbeatError;
    return assertOwner(repo, token);
  };
  try {
    authority(repo, window.value);
    await preflight(repo, window.value, control.signal);
    check();
    await child(repo, 'bash', ['scripts/check.sh', ...options.gateArgs], { signal: control.signal });
    check();
    await child(repo, 'bash', ['scripts/check.sh', '--verify-receipt', ...options.gateArgs], {
      signal: control.signal,
    });
    check();
    authority(repo, window.value);
    const oldTip = await preflight(repo, window.value, control.signal);
    check();
    await ticketPhase(repo, options, 'integrating', control.signal);
    check();
    const ready = writeWindow(repo, readWindow(repo).oid, {
      ...window.value,
      phase: 'ready',
      oldTip,
    });
    if (!ready) throw new Error('Main reservation changed before push');
    window = ready;
    const expected = oldTip ?? zero;
    verifyMainPush(repo, token, options.remote, options.target, window.value.head, expected);
    await child(repo, 'git', ['push', ...options.gitFlags, options.remote, `${window.value.head}:refs/heads/main`], {
      signal: control.signal,
      timeoutMs: 10 * 60_000,
      env: { ...process.env, HOTSHEET_MAIN_WINDOW_TOKEN: token },
    });
    check();
    const confirmed = await remoteTip(repo, options.remote, control.signal);
    if (confirmed !== window.value.head) {
      if (!confirmed) throw new Error('Remote main disappeared after push');
      if (gitResult(repo, ['cat-file', '-e', `${confirmed}^{commit}`]).status !== 0)
        await child(repo, 'git', ['fetch', '--no-tags', '--no-write-fetch-head', options.remote, 'main'], {
          signal: control.signal,
          timeoutMs: 120_000,
        });
      if (gitResult(repo, ['merge-base', '--is-ancestor', window.value.head, confirmed]).status !== 0)
        throw new Error('Push returned, but remote main does not contain the reserved commit');
    }
    try {
      await ticketPhase(repo, options, 'final_testing', control.signal);
    } catch (error) {
      console.error(
        `Remote main contains ${window.value.head}; set the ticket to final_testing manually: ${error.message}`,
      );
    }
    return window.value.head;
  } finally {
    clearInterval(heartbeat);
    control.abort();
    process.removeListener('SIGINT', onSignal);
    process.removeListener('SIGTERM', onSignal);
    const current = readWindow(repo);
    if (current?.value.token === token) deleteWindow(repo, current);
  }
}

async function main() {
  const [command, ...args] = process.argv.slice(2);
  if (command === 'verify') {
    const [remote, target, localSha, remoteSha] = args;
    if (args.length !== 4) throw new Error('verify needs remote, target, local SHA and remote SHA');
    const value = verifyMainPush(project, process.env.HOTSHEET_MAIN_WINDOW_TOKEN, remote, target, localSha, remoteSha);
    await child(project, 'bash', ['scripts/check.sh', '--verify-receipt', ...value.gateArgs]);
    verifyMainPush(project, process.env.HOTSHEET_MAIN_WINDOW_TOKEN, remote, target, localSha, remoteSha);
  } else if (command === 'push') {
    const sha = await pushMain(project, args);
    console.log(`Confirmed remote main contains ${sha}`);
  } else throw new Error('Expected push or verify');
}

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url)))
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
