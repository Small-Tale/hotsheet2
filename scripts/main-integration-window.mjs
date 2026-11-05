// HS2-BND42N: one local main writer from preflight through gate, push and confirmation.
// HS2-GT57BQ: the same window reserves a ci-staged integrator's push to its integration branch
// (`--staged`) and its fast-forward of main to an exact green staged SHA (`--promote`).
import { randomUUID, createHash } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, lstatSync, readFileSync, realpathSync, statSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseMainPushArgs, validStagingBranch } from './main-integration-options.mjs';
import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';

const project = resolve(fileURLToPath(new URL('..', import.meta.url)));
const ref = 'refs/hotsheet/main-integration-window';
const zero = '0'.repeat(40);
const leaseMs = 120_000;
// HS2-RXYJ6G: tests shorten the renewal interval; production keeps 20 s against the 120 s lease.
const heartbeatMs = Number(process.env.HOTSHEET_MAIN_WINDOW_HEARTBEAT_MS) || 20_000;
const fullSha = /^[0-9a-f]{40}$/i;
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

function git(repo, args, input) {
  const result = spawnSync('git', args, {
    cwd: repo,
    env: withoutGitRepositoryEnv(process.env),
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
  return spawnSync('git', args, { cwd: repo, env: withoutGitRepositoryEnv(process.env), encoding: 'utf8', timeout: 10_000 });
}

function commonDir(repo) {
  return realpathSync(resolve(repo, git(repo, ['rev-parse', '--git-common-dir'])));
}

// HS2-GT57BQ: a promotion pushes a recorded SHA, not the checkout, so it needs no clean HEAD.
function promotionSnapshot(repo, sha) {
  if (gitResult(repo, ['cat-file', '-e', `${sha}^{commit}`]).status !== 0)
    throw new Error(`Promotion commit ${sha} is not in this repository; fetch the staging branch first`);
  return { head: sha, tree: git(repo, ['rev-parse', `${sha}^{tree}`]) };
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

// HS2-BYDMV4: only acquisition may wait for an unresolved, physically present ref.
class UnresolvedWindowError extends Error {}

export function readWindow(repo, query = gitResult) {
  // HS2-W110HQ: release/replacement may follow this lookup; only read its immutable oid.
  const resolved = query(repo, ['rev-parse', '--verify', '--quiet', ref]);
  if (resolved.error || resolved.signal || resolved.stderr) throw new Error('Cannot read main integration ref');
  if (resolved.status === 1) {
    if (resolved.stdout) throw new Error('Cannot read main integration ref: unexpected lookup output');
    // No OID was captured. Git's typed existence result distinguishes absence from quiet corruption.
    const exists = query(repo, ['show-ref', '--exists', ref]);
    if (exists.error || exists.signal || exists.stdout)
      throw new Error('Cannot determine main integration ref existence');
    // Status 2 has ordinary diagnostic stderr; use the typed status, never its message language.
    if (exists.status === 2) {
      // Git can call a directory at this loose-ref path missing. It still blocks
      // zero-OID acquisition, so reject that physical collision explicitly.
      const path = resolve(repo, git(repo, ['rev-parse', '--git-path', ref]));
      try {
        lstatSync(path);
        throw new Error('Main integration ref path exists although Git reports it missing');
      } catch (error) {
        if (error?.code !== 'ENOENT') throw error;
      }
      return null;
    }
    if (exists.status === 0 && !exists.stderr)
      throw new UnresolvedWindowError('Main integration ref exists but its reservation is unresolved');
    throw new Error(
      'Cannot determine main integration ref existence; Git show-ref --exists support and a readable ref database are required',
    );
  }
  if (resolved.status !== 0) throw new Error('Cannot read main integration ref');
  const oid = resolved.stdout.trim();
  if (!fullSha.test(oid)) throw new Error('Cannot read main integration ref: invalid object id');
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
    !['main', 'staged', 'promote'].includes(value.mode ?? 'main') ||
    ((value.mode ?? 'main') === 'staged'
      ? !value.target?.startsWith('refs/heads/') || !validStagingBranch(value.target.slice(11))
      : value.target !== 'refs/heads/main') ||
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
  // HS2-RXYJ6G: name the failed condition, so a stopped gate says why.
  const reason = !current
    ? 'the reservation ref is missing'
    : current.value.token !== token
      ? 'another owner replaced the reservation'
      : current.value.expiresAt <= Date.now()
        ? `the reservation expired ${Date.now() - current.value.expiresAt} ms ago`
        : !alive(current.value.pid)
          ? `the reserving process ${current.value.pid} is gone`
          : phase && current.value.phase !== phase
            ? `the reservation phase is ${current.value.phase}, not ${phase}`
            : null;
  if (reason) throw new Error(`Main integration reservation was lost or expired: ${reason}`);
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
  let unresolved = false;
  while (Date.now() - started < 10 * 60_000) {
    try {
      const current = readWindow(repo);
      unresolved = false;
      if (!current || current.value.expiresAt <= Date.now()) {
        const selected =
          options.mode === 'promote' ? promotionSnapshot(repo, options.promote) : snapshot(repo, options.source);
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
          assignments: options.assignments,
          standalone: options.standalone,
          sourceStore: options.sourceStore,
          mode: options.mode,
        });
        if (written) return written;
      }
    } catch (error) {
      if (!(error instanceof UnresolvedWindowError)) throw error;
      unresolved = true;
    }
    await sleep(1000);
  }
  throw new Error(
    unresolved
      ? 'Timed out resolving the main integration reservation ref; inspect the ref for corruption before recovery'
      : 'Timed out waiting for the local main integration window',
  );
}

function assertSnapshot(repo, value) {
  if (value.mode === 'promote') {
    promotionSnapshot(repo, value.head);
    return;
  }
  const current = snapshot(repo, value.head);
  if (current.head !== value.head || current.tree !== value.tree)
    throw new Error('Checked-out main changed during integration');
}

/**
 * HS2-GT57BQ: what a ci-staged integrator may push. `staged` sends the opened batch to the
 * integration branch; `promote` fast-forwards main to a green staged batch's exact SHA; a plain
 * main push is serial fallback, allowed only while the batch circuit breaker is tripped.
 */
function authorizeStaged(state, value, ledger) {
  const mode = value.mode ?? 'main';
  const strategy = state.integrator.strategy;
  if (mode !== 'main' && strategy !== 'ci-staged') throw new Error(`--${mode} needs a ci-staged integrator`);
  if (strategy !== 'ci-staged') return;
  if (mode === 'promote') {
    const green = Object.values(ledger?.batches ?? {}).find(
      (batch) => batch?.state === 'green' && batch.staging?.staged_commit === value.head,
    );
    if (!green) throw new Error(`No green staged batch has staged commit ${value.head}; promotion refused`);
    return;
  }
  if (mode === 'main' && !ledger?.breaker?.tripped)
    throw new Error(
      'A ci-staged integrator reaches main only through --promote of a green staged SHA, or serially while its circuit breaker is tripped',
    );
}

export function authorizeMainState(state, value, projectId, sourceId, now, ledger = null) {
  if (!state) {
    if (value.identity) throw new Error('No coordinator pool for integrator push');
    if (value.gateArgs?.includes('--smoke')) throw new Error('The --smoke gate is only for a batch integrator push');
    return 'standalone';
  }
  if (
    !Number.isInteger(state.version) ||
    state.version < 3 ||
    // HS2-9G0M3V: schema seven adds next plans and eight integrator strategies; HS2-JBAD2Z:
    // nine adds worker preferences (HS2-6WFCZR). None changes the fields read here.
    state.version > 9 ||
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
    // HS2-9G0M3V: the smoke gate replaces the affected gate only for a project-allowed batch
    // (HS2-GT57BQ: or ci-staged) integrator.
    const batchStrategy = ['batch', 'ci-staged'].includes(state.integrator.strategy);
    if (value.gateArgs?.includes('--smoke') && !batchStrategy)
      throw new Error('The --smoke gate is only for a batch integrator push');
    authorizeStaged(state, value, ledger);
    // HS2-GT57BQ: a promotion moves no assignment; the green ledger batch is its authority.
    if (value.mode === 'promote') return 'integrator';
    const batch = value.assignments?.length > 1 ? value.assignments : null;
    if (batch) {
      // HS2-9G0M3V: one push for an opened batch: its members are exactly the integrating set.
      if (!batchStrategy) throw new Error('A multi-assignment push needs a batch integrator');
      const members = new Set(batch);
      if (
        fence.length !== members.size ||
        !fence.every(
          (item) =>
            members.has(item.id) &&
            item.phase === 'integrating' &&
            (item.handoff?.kind === 'code' || item.commit),
        )
      )
        throw new Error('Assignments are not exactly the integrating code handoffs of the open batch');
    } else if (value.assignment) {
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
  if ((value.mode ?? 'main') !== 'main') throw new Error(`--${value.mode} needs a live ci-staged integrator`);
  if (value.gateArgs?.includes('--smoke')) throw new Error('The --smoke gate is only for a batch integrator push');
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
  // HS2-GT57BQ: the batch ledger beside the pool decides promotion and serial fallback.
  const ledgerPath = join(directory, 'batches.json');
  const ledger = existsSync(ledgerPath) ? JSON.parse(readFileSync(ledgerPath, 'utf8')) : null;
  const shipping = realpathSync(resolve(common, '..'));
  const slug = basename(shipping)
    .replace(/[^A-Za-z0-9]/g, '-')
    .toLowerCase()
    .replace(/^-+|-+$/g, '');
  const projectId = `${slug}-${createHash('sha256').update(shipping).digest('hex').slice(0, 12)}`;
  const sourceId = createHash('sha256').update(realpathSync(value.sourceStore)).digest('hex');
  return authorizeMainState(state, value, projectId, sourceId, Math.floor(Date.now() / 1000), ledger);
}

async function child(repo, program, args, { env = withoutGitRepositoryEnv(process.env), signal, output = false, timeoutMs } = {}) {
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

async function remoteTip(repo, remote, signal, ref = 'refs/heads/main') {
  const line = await child(repo, 'git', ['ls-remote', remote, ref], {
    output: true,
    signal,
    timeoutMs: 120_000,
  });
  const tip = line ? line.split(/\s+/)[0] : null;
  if (tip && !fullSha.test(tip)) throw new Error(`Remote ${ref} returned an invalid SHA`);
  return tip;
}

async function ensureCommit(repo, remote, tip, ref, signal) {
  if (gitResult(repo, ['cat-file', '-e', `${tip}^{commit}`]).status !== 0)
    await child(repo, 'git', ['fetch', '--no-tags', '--no-write-fetch-head', remote, ref], {
      signal,
      timeoutMs: 120_000,
    });
}

const isAncestor = (repo, ancestor, descendant) =>
  gitResult(repo, ['merge-base', '--is-ancestor', ancestor, descendant]).status === 0;

async function preflight(repo, value, signal) {
  assertSnapshot(repo, value);
  const fetchUrls = git(repo, ['remote', 'get-url', '--all', value.remote]).split('\n');
  const pushUrls = git(repo, ['remote', 'get-url', '--push', '--all', value.remote]).split('\n');
  if (fetchUrls.length !== 1 || pushUrls.length !== 1 || fetchUrls[0] !== pushUrls[0])
    throw new Error('Main reservation requires one matching fetch/push URL for the named remote');
  const tip = await remoteTip(repo, value.remote, signal);
  if (tip) {
    await ensureCommit(repo, value.remote, tip, 'main', signal);
    if (!isAncestor(repo, tip, value.head)) {
      // HS2-GT57BQ: main is never merged or rewritten into a promotion; the batch re-stages.
      if (value.mode === 'promote')
        throw new Error(
          `Remote main moved and no longer fast-forwards to ${value.head}; re-stage the batch on the new main (coordination batch-restage)`,
        );
      if (value.mode === 'staged')
        throw new Error('Remote main is not an ancestor of the staged commit; rebuild the integration branch on main');
      throw new Error('Remote main is not an ancestor of the checked-out commit; fetch and integrate first');
    }
  }
  if (value.mode !== 'staged') return tip;
  // HS2-GT57BQ: the integration branch is integrator-owned scratch. It moves forward, or is
  // rebuilt over a tip that main does not contain; never over a SHA main already holds.
  const staged = await remoteTip(repo, value.remote, signal, value.target);
  if (staged && !isAncestor(repo, staged, value.head)) {
    await ensureCommit(repo, value.remote, staged, value.target, signal);
    if (!isAncestor(repo, staged, value.head) && tip && isAncestor(repo, staged, tip))
      throw new Error(`Remote ${value.target} is ${staged}, which main contains; the staged commit must descend from it`);
  }
  return staged;
}

async function ticketPhase(repo, options, phase, signal) {
  const index = options.gateArgs.indexOf('--ticket');
  // HS2-9G0M3V: a batch push moves every listed ticket; a serial push its gated ticket.
  const tickets = options.phaseTickets?.length ? options.phaseTickets : index < 0 ? [] : [options.gateArgs[index + 1]];
  for (const ticket of tickets) await editPhase(repo, options, ticket, phase, signal);
}

async function editPhase(repo, options, ticket, phase, signal) {
  await child(
    repo,
    process.env.HOTSHEET_MAIN_WINDOW_CLI ?? 'hotsheet-cli',
    [
      'edit',
      ticket,
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

/**
 * Whether `hooks` (the shared `core.hooksPath`) names this repository's own tracked `.githooks`
 * (HS2-Z333GJ). Linked worktrees share one config, and creating one can leave an absolute path to
 * the main checkout's `.githooks`; that is still the repository's hooks, so it is accepted from the
 * main checkout and from any of its worktrees. Any other location is refused.
 */
export function acceptedHooksPath(repo, hooks) {
  const real = (path) => {
    try {
      return realpathSync(path);
    } catch {
      return undefined;
    }
  };
  const configured = real(resolve(repo, hooks));
  if (configured === undefined) return false;
  const commonDir = real(resolve(repo, git(repo, ['rev-parse', '--git-common-dir'])));
  const owners = [resolve(repo, '.githooks')];
  if (commonDir && basename(commonDir) === '.git') owners.push(resolve(dirname(commonDir), '.githooks'));
  return owners.some((owner) => real(owner) === configured);
}

export async function pushMain(repo, args) {
  const options = parseMainPushArgs(args);
  options.sourceStore = resolve(options.sourceStore ?? repo);
  if (!acceptedHooksPath(repo, git(repo, ['config', '--get', 'core.hooksPath'])))
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
    const before = await preflight(repo, window.value, control.signal);
    check();
    // HS2-GT57BQ: a promotion's gate is the green CI run on the exact SHA, not a local receipt.
    if (options.mode === 'promote' && before === window.value.head) return window.value.head;
    if (options.mode !== 'promote') {
      await child(repo, 'bash', ['scripts/check.sh', ...options.gateArgs], { signal: control.signal });
      check();
      await child(repo, 'bash', ['scripts/check.sh', '--verify-receipt', ...options.gateArgs], {
        signal: control.signal,
      });
      check();
    }
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
    // HS2-GT57BQ: a rebuilt integration tip replaces exactly the old one it was checked against.
    const lease =
      options.mode === 'staged' && oldTip && !isAncestor(repo, oldTip, window.value.head)
        ? [`--force-with-lease=${options.target}:${oldTip}`]
        : [];
    await child(
      repo,
      'git',
      ['push', ...options.gitFlags, ...lease, options.remote, `${window.value.head}:${options.target}`],
      {
        signal: control.signal,
        timeoutMs: 10 * 60_000,
        env: { ...withoutGitRepositoryEnv(process.env), HOTSHEET_MAIN_WINDOW_TOKEN: token },
      },
    );
    check();
    const confirmed = await remoteTip(repo, options.remote, control.signal, options.target);
    if (options.mode === 'staged' && confirmed !== window.value.head)
      throw new Error(`Push returned, but remote ${options.target} is not the reserved commit`);
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
  } catch (error) {
    // HS2-RXYJ6G: a heartbeat failure aborts the gate, whose own exit (often a null code) would
    // otherwise hide the reason the reservation stopped.
    if (heartbeatError && error !== heartbeatError)
      throw new Error(`Main integration heartbeat stopped the push: ${heartbeatError.message}`, { cause: error });
    throw error;
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
    // HS2-GT57BQ: a promotion was gated by its green CI run, which authority() re-read.
    if (value.mode !== 'promote')
      await child(project, 'bash', ['scripts/check.sh', '--verify-receipt', ...value.gateArgs]);
    verifyMainPush(project, process.env.HOTSHEET_MAIN_WINDOW_TOKEN, remote, target, localSha, remoteSha);
  } else if (command === 'push') {
    const sha = await pushMain(project, args);
    const options = parseMainPushArgs(args);
    console.log(`Confirmed remote ${options.target.slice(11)} contains ${sha}`);
  } else throw new Error('Expected push or verify');
}

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url)))
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
