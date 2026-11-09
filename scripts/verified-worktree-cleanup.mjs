#!/usr/bin/env node
// HS2-HR3ZX4: remove only a closed ticket's verified, idle, preserved checkout.
import { spawnSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, realpathSync, renameSync, statfsSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { classifyRuns, parseTicket } from './ci-completion-proposal.mjs';
import { isCovered } from './ci-gate.mjs';
import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';

const sha = /^[0-9a-f]{40}$/i;
const operationName = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;
const hash = (value) => createHash('sha256').update(value).digest('hex');

export function command(binary, args, cwd) {
  const result = spawnSync(binary, args, {
    cwd,
    encoding: 'utf8',
    env: withoutGitRepositoryEnv(process.env),
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  return result;
}

function checked(run, binary, args, cwd) {
  const result = run(binary, args, cwd);
  if (result.status !== 0) throw new Error(result.stderr?.trim() || `${binary} ${args[0]} failed`);
  return result.stdout.trim();
}

function git(run, cwd, ...args) {
  return checked(run, 'git', args, cwd);
}
function requireProof(condition, reason) {
  if (!condition) throw new Error(reason);
}
function freeBytes(path) {
  const stat = statfsSync(path, { bigint: true });
  return stat.bavail * stat.bsize;
}

function worktreeEntry(project, path, run) {
  const entries = git(run, project, 'worktree', 'list', '--porcelain').split('\n\n');
  const match = entries.find((entry) => {
    const listed = /^worktree (.+)$/m.exec(entry)?.[1];
    return listed && realpathSync(listed) === path;
  });
  requireProof(match, 'worktree is not registered; unknown ownership');
  requireProof(!/^locked(?: |$)/m.test(match), 'worktree is locked by a worker or agent session');
  requireProof(!/^prunable(?: |$)/m.test(match), 'worktree registration is interrupted or stale');
  const branch = /^branch refs\/heads\/(.+)$/m.exec(match)?.[1];
  requireProof(branch && branch !== 'main', 'worktree has no removable ticket branch');
  const head = /^HEAD ([0-9a-f]{40})$/im.exec(match)?.[1];
  requireProof(head, 'worktree has no exact HEAD commit');
  return { branch, head: head.toLowerCase() };
}

function ticketIdentity(project, store, slug, run) {
  const ticket = parseTicket(checked(run, 'hotsheet-cli', ['-C', store, 'show', slug], project));
  requireProof(ticket.slug === slug && ticket.id, 'ticket identity does not match the requested slug');
  requireProof(['completed', 'verified', 'archive'].includes(ticket.status), 'ticket is not completed or verified');
  const checkout = JSON.parse(checked(run, 'hotsheet-cli', ['checkout', 'resolve', project], project));
  const source = checkout.sources?.find((item) => item.provider === 'git' && realpathSync(item.locator) === store);
  requireProof(checkout.id && source?.connection_id, 'checkout does not own this exact ticket store');
  const qualified = `@${checkout.id}/${source.connection_id}:${ticket.id}`;
  return { ticket, qualified, source };
}

function coordination(project, store, ticket, operation, run) {
  const snapshot = JSON.parse(
    checked(run, 'hotsheet-cli', ['-C', store, 'coordination', '--project', project, 'status'], project),
  );
  const liveClaim = snapshot.live_claims?.some(
    (claim) =>
      claim.ticket?.connection_id === ticket.source.connection_id && claim.ticket?.native_id === ticket.ticket.id,
  );
  requireProof(!liveClaim, 'ticket has an active claim');
  const assignments = Object.values(snapshot.state?.assignments ?? {}).filter(
    (assignment) =>
      assignment.ticket?.connection_id === ticket.source.connection_id &&
      assignment.ticket?.native_id === ticket.ticket.id,
  );
  requireProof(
    assignments.every((assignment) => assignment.phase === 'done' && !assignment.recovery_note),
    'handoff, review, build, or CI wait is still active',
  );
  const relevant = operation ? assignments.find((assignment) => assignment.id === operation) : null;
  if (operation)
    requireProof(relevant?.phase === 'done' && relevant.handoff?.kind === 'code', 'reviewed code handoff is not done');
  // HS2-AYPN79: a worker may remain registered while working on a later ticket.
  // The exact ticket claim and handoff checks above, plus liveHandles(path) below,
  // establish whether this completed worktree is still in use.
  return relevant;
}

function handoffMapping(project, common, ticket, operation, path, branch, head, shipping, assignment, batchMember, run) {
  requireProof(operationName.test(operation), 'operation is not a safe identifier');
  const recordPath = join(common, 'hotsheet-integrations', hash(ticket.qualified), `${operation}.json`);
  requireProof(existsSync(recordPath), 'durable integration record is missing');
  const record = JSON.parse(readFileSync(recordPath, 'utf8'));
  requireProof(
    record.version === 1 &&
      record.operation === operation &&
      record.branch === 'main' &&
      record.ticket?.project_id === ticket.qualified.split('/')[0].slice(1) &&
      record.ticket?.connection_id === ticket.source.connection_id &&
      record.ticket?.native_id === ticket.ticket.id &&
      record.source_commits?.includes(head) &&
      record.shipping_commits?.includes(shipping),
    'integration record does not map this exact source and shipping commit',
  );
  const proposalPath = join(common, 'hotsheet-ci-proposals', `${operation}.json`);
  if (existsSync(proposalPath)) {
    const proposal = JSON.parse(readFileSync(proposalPath, 'utf8')).record;
    requireProof(
      proposal?.operation === operation &&
        proposal.slug === ticket.ticket.slug &&
        proposal.sourceWorktree &&
        realpathSync(proposal.sourceWorktree) === path &&
        proposal.sourceBranch === branch &&
        proposal.sourceCommit === head &&
        proposal.integrated === shipping &&
        proposal.ticket?.native_id === ticket.ticket.id &&
        proposal.ticket?.connection_id === ticket.source.connection_id,
      'reviewed handoff does not map this exact ticket, worktree, branch, and commits',
    );
  } else {
    // Dispatcher batches have no legacy proposal. Their frozen source commit is in both
    // the coordinator handoff and batch member; require a canonical ticket worktree too.
    requireProof(
      batchMember?.source_commit === head && assignment?.handoff?.commit === head,
      'reviewed batch has no exact source commit mapping',
    );
    sourceWorktreeIdentity(project, path, branch, ticket.ticket.slug, run);
  }
  const archive = `refs/hotsheet/integration-source/${head}`;
  requireProof(
    git(run, project, 'rev-parse', '--verify', archive) === head,
    'source commit is not preserved by its integration archive ref',
  );
}

// HS2-VKGV08: a batch smoke gate writes its exact receipt in the shared Git cache, not a
// root-gate marker in the primary checkout. Bind it to the green batch and its exact member.
function batchGate(project, store, common, ticket, operation, shipping, remote, run) {
  const ledger = JSON.parse(
    checked(run, 'hotsheet-cli', ['-C', store, 'coordination', '--project', project, 'batch-status'], project),
  );
  const matches = Object.values(ledger.batches ?? {}).filter((batch) =>
    batch.tickets?.some(
      (member) =>
        member.assignment === operation &&
        member.ticket?.connection_id === ticket.source.connection_id &&
        member.ticket?.native_id === ticket.ticket.id,
    ),
  );
  requireProof(matches.length <= 1, 'reviewed handoff maps to multiple batches');
  if (!matches.length) return false;
  const batch = matches[0];
  requireProof(
    batch.tickets.some((member) => member.assignment === operation && member.shipping_commits?.includes(shipping)),
    'reviewed batch does not map the exact shipping commit',
  );
  requireProof(batch.state === 'green' && sha.test(batch.main_commit ?? ''), 'reviewed batch is not green');
  requireProof(
    run('git', ['merge-base', '--is-ancestor', shipping, batch.main_commit], project).status === 0 &&
      run('git', ['merge-base', '--is-ancestor', batch.main_commit, remote], project).status === 0,
    'reviewed batch does not contain the shipping commit on origin/main',
  );
  const tree = git(run, project, 'rev-parse', `${batch.main_commit}^{tree}`);
  const cache = join(common, 'hotsheet-local-gate');
  const expected = batch.smoke_checks;
  requireProof(Array.isArray(expected) && expected.length > 0, 'reviewed batch has no declared smoke checks');
  const receipts = existsSync(cache) ? readdirSync(cache).filter((name) => /^[0-9a-f]{64}\.json$/.test(name)) : [];
  const passed = receipts.some((name) => {
    const receipt = JSON.parse(readFileSync(join(cache, name), 'utf8'));
    return (
      receipt.key === name.slice(0, -5) &&
      receipt.tree === tree &&
      receipt.tier === 'smoke' &&
      receipt.profile?.smoke === true &&
      JSON.stringify(receipt.selected) === JSON.stringify(expected) &&
      receipt.results?.length === expected.length &&
      receipt.results.every(
        (result, index) =>
          result.command === expected[index] &&
          (result.status === 0 || (result.reused === true && result.status == null)),
      )
    );
  });
  requireProof(passed, 'reviewed batch has no successful exact-tree smoke receipt in the shared Git gate cache');
  return batch.tickets.find((member) => member.assignment === operation && member.shipping_commits?.includes(shipping));
}

function sourceWorktreeIdentity(project, path, branch, slug, run) {
  const token = slug.toLowerCase();
  requireProof(branch === token || branch.startsWith(`${token}-`), 'branch does not identify this ticket');
  requireProof(basename(path) === branch, 'worktree path does not identify this branch');
  const expectedRoot = join(project, '.claude', 'worktrees');
  const subpath = relative(expectedRoot, path);
  requireProof(
    subpath && !subpath.startsWith('..') && !subpath.includes('/'),
    'worktree is outside the ticket worktree root',
  );
  requireProof(
    git(run, path, 'log', '-1', '--format=%s').startsWith(`${slug} `),
    'source HEAD subject does not name this ticket',
  );
}

function directMapping(project, path, branch, head, slug, run) {
  sourceWorktreeIdentity(project, path, branch, slug, run);
  requireProof(
    run('git', ['merge-base', '--is-ancestor', head, 'refs/remotes/origin/main'], project).status === 0,
    'source commit is not preserved on origin/main',
  );
}

function liveHandles(path, project, run) {
  const result = run('lsof', ['+D', path], project);
  requireProof(
    result.status === 1 && !result.stdout.trim() && !result.stderr.trim(),
    result.status === 0
      ? 'worktree has live process handles (agent or build)'
      : 'cannot prove the worktree has no live process handles',
  );
}

function greenCi(project, shipping, run) {
  const listed = JSON.parse(
    checked(
      run,
      'gh',
      [
        'run',
        'list',
        '--branch',
        'main',
        '--workflow',
        'CI',
        '--event',
        'push',
        '--limit',
        '100',
        '--json',
        'databaseId,headSha,headBranch,event,workflowName,status,conclusion,createdAt,url',
      ],
      project,
    ),
  );
  const result = classifyRuns(listed, shipping, (base, head) =>
    isCovered(base, head, (binary, args) => run(binary, args, project)),
  );
  requireProof(result.state === 'success', `covering main CI is ${result.state}`);
  return result.runs.find(
    (item) => item.status === 'completed' && item.conclusion === 'success' && item.coverage === 'covered',
  );
}

function deleteBranch(project, branch, head, operation, run) {
  if (operation) {
    // A reviewed cherry-pick may retain the source only under its archive ref.
    // Compare-and-delete the exact local branch tip; no force push or ref rewrite.
    git(run, project, 'update-ref', '-d', `refs/heads/${branch}`, head);
  } else git(run, project, 'branch', '-d', '--', branch);
}

export function cleanup(options, { run = command } = {}) {
  const project = realpathSync(options.project),
    store = realpathSync(options.store);
  const requestedPath = resolve(options.worktree);
  const path = existsSync(requestedPath)
    ? realpathSync(requestedPath)
    : join(realpathSync(dirname(requestedPath)), basename(requestedPath));
  const slug = options.ticket,
    shipping = options.shipping?.toLowerCase();
  requireProof(
    /^HS2-[A-Z0-9]+$/.test(slug ?? '') && sha.test(shipping ?? ''),
    'ticket slug and full shipping SHA are required',
  );
  requireProof(path !== project, 'main checkout cannot be cleaned');
  const common = resolve(project, git(run, project, 'rev-parse', '--git-common-dir'));
  const intentPath = join(common, 'hotsheet-worktree-cleanup', `${hash(`${slug}\n${path}`)}.json`);
  const previous = existsSync(intentPath) ? JSON.parse(readFileSync(intentPath, 'utf8')) : null;
  if (previous) {
    requireProof(
      previous.version === 1 &&
        previous.slug === slug &&
        previous.path === path &&
        previous.shipping === shipping &&
        previous.operation === (options.operation ?? null),
      'cleanup intent maps different evidence',
    );
  }
  const identity = ticketIdentity(project, store, slug, run);
  const assignment = coordination(project, store, identity, options.operation, run);
  requireProof(options.operation ? assignment : true, 'operation has no matching assignment');
  const remoteLine = git(run, project, 'ls-remote', '--exit-code', 'origin', 'refs/heads/main');
  const remote = /^([0-9a-f]{40})\s+refs\/heads\/main$/i.exec(remoteLine)?.[1]?.toLowerCase();
  requireProof(
    remote && git(run, project, 'cat-file', '-e', `${remote}^{commit}`) === '',
    'origin/main commit is not available locally; fetch it first',
  );
  requireProof(
    run('git', ['merge-base', '--is-ancestor', shipping, remote], project).status === 0,
    'shipping commit is not on origin/main',
  );
  const tree = git(run, project, 'rev-parse', `${shipping}^{tree}`);
  const ci = greenCi(project, shipping, run);
  if (previous && !existsSync(path)) {
    // A direct ticket's receipt lived in this worktree's Git directory,
    // which Git removed with the worktree. The durable intent records that
    // the exact receipt passed earlier.
    const ref = run('git', ['show-ref', '--verify', '--quiet', `refs/heads/${previous.branch}`], project);
    if (ref.status === 0) {
      requireProof(
        git(run, project, 'rev-parse', `refs/heads/${previous.branch}`) === previous.head,
        'interrupted branch moved after cleanup intent',
      );
      if (options.operation) {
        requireProof(
          git(run, project, 'rev-parse', '--verify', `refs/hotsheet/integration-source/${previous.head}`) ===
            previous.head,
          'interrupted source commit lost its archive ref',
        );
      } else
        requireProof(
          run('git', ['merge-base', '--is-ancestor', previous.head, remote], project).status === 0,
          'interrupted source commit is not preserved',
        );
      deleteBranch(project, previous.branch, previous.head, options.operation, run);
    }
    return { state: 'already_removed', branch: previous.branch, reclaimedBytes: 0, ciRun: ci.id };
  }
  requireProof(existsSync(path), 'worktree is missing without a cleanup intent');
  const resolved = realpathSync(path);
  requireProof(resolved === path, 'worktree path is a symlink or changed');
  const { branch, head } = worktreeEntry(project, path, run);
  requireProof(
    !git(run, path, 'status', '--porcelain=v1', '--untracked-files=all'),
    'worktree has tracked or non-ignored untracked edits',
  );
  requireProof(git(run, path, 'rev-parse', 'HEAD') === head, 'worktree HEAD changed');
  const batchPassed = options.operation
    ? batchGate(project, store, common, identity, options.operation, shipping, remote, run)
    : false;
  if (!batchPassed) {
    // Direct and nonbatch handoffs still require their existing exact-tree root-gate marker.
    const gateCheckout = options.operation ? project : path;
    const gateDir = resolve(gateCheckout, git(run, gateCheckout, 'rev-parse', '--git-path', 'hotsheet-root-gate'));
    const gate = join(gateDir, tree);
    requireProof(
      existsSync(gate) && readFileSync(gate, 'utf8').trim() === tree,
      `shipping tree has no successful exact-tree local gate in ${options.operation ? 'the integrator' : 'this worktree'}`,
    );
  }
  if (options.operation)
    handoffMapping(project, common, identity, options.operation, path, branch, head, shipping, assignment, batchPassed, run);
  else directMapping(project, path, branch, head, slug, run);
  requireProof(
    run('git', ['merge-base', '--is-ancestor', head, remote], project).status === 0 || options.operation,
    'source commit is not preserved on origin/main or an integration archive ref',
  );
  liveHandles(path, project, run);
  const intent = { version: 1, slug, path, branch, head, shipping, operation: options.operation ?? null, ciRun: ci.id };
  if (previous)
    requireProof(
      JSON.stringify(previous) === JSON.stringify(intent),
      'cleanup intent no longer matches current evidence',
    );
  else {
    mkdirSync(dirname(intentPath), { recursive: true });
    const temporary = `${intentPath}.${randomUUID()}.tmp`;
    writeFileSync(temporary, `${JSON.stringify(intent)}\n`, { flag: 'wx', mode: 0o600 });
    renameSync(temporary, intentPath);
  }
  const before = freeBytes(project);
  git(run, project, 'worktree', 'remove', path);
  deleteBranch(project, branch, head, options.operation, run);
  const reclaimedBytes = Number(freeBytes(project) - before);
  return { state: 'removed', branch, reclaimedBytes: Math.max(0, reclaimedBytes), ciRun: ci.id };
}

function args(values) {
  const parsed = {};
  for (let index = 0; index < values.length; index += 2) {
    const key = values[index],
      value = values[index + 1];
    if (
      !['--project', '--store', '--ticket', '--worktree', '--shipping', '--operation'].includes(key) ||
      !value ||
      parsed[key.slice(2)]
    )
      throw new Error(`invalid cleanup option ${key ?? ''}`);
    parsed[key.slice(2)] = value;
  }
  for (const key of ['project', 'store', 'ticket', 'worktree', 'shipping'])
    requireProof(parsed[key], `--${key} is required`);
  return parsed;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    console.log(JSON.stringify(cleanup(args(process.argv.slice(2)))));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
