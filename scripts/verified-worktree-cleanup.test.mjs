import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, realpathSync, unlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { cleanup, command } from './verified-worktree-cleanup.mjs';
import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';

const slug = 'HS2-HR3ZX4';
const native = '01M00000000000000000000000';
const connection = 'synthetic-store';
const projectId = 'synthetic-checkout';

function git(cwd, ...args) {
  const result = spawnSync('git', args, { cwd, encoding: 'utf8', env: withoutGitRepositoryEnv(process.env) });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}

function fixture({ cherryPick = false } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'hs2-hr3zx4-'));
  const project = join(root, 'project'),
    store = join(root, 'store'),
    remote = join(root, 'remote.git');
  mkdirSync(project);
  mkdirSync(store);
  git(project, 'init', '-b', 'main');
  git(project, 'config', 'user.name', 'Synthetic Test');
  git(project, 'config', 'user.email', 'synthetic@example.invalid');
  writeFileSync(join(project, 'base.txt'), 'synthetic\n');
  writeFileSync(join(project, '.gitignore'), 'target/\n');
  git(project, 'add', 'base.txt', '.gitignore');
  git(project, 'commit', '-m', 'Synthetic base');
  git(root, 'init', '--bare', remote);
  git(project, 'remote', 'add', 'origin', remote);
  git(project, 'push', '-u', 'origin', 'main');
  const branch = slug.toLowerCase();
  const worktree = join(project, '.claude', 'worktrees', branch);
  mkdirSync(join(project, '.claude', 'worktrees'), { recursive: true });
  git(project, 'worktree', 'add', '-b', branch, worktree);
  writeFileSync(join(worktree, 'ticket.txt'), 'synthetic ticket\n');
  git(worktree, 'add', 'ticket.txt');
  git(worktree, 'commit', '-m', `${slug} Add synthetic change`);
  const source = git(worktree, 'rev-parse', 'HEAD');
  if (cherryPick) {
    git(project, 'cherry-pick', '-n', branch);
    git(project, 'commit', '-m', `${slug} Integrate synthetic change`);
  } else git(project, 'merge', '--ff-only', branch);
  git(project, 'push', 'origin', 'main');
  const shipping = git(project, 'rev-parse', 'HEAD');
  const tree = git(project, 'rev-parse', 'HEAD^{tree}');
  const common = git(project, 'rev-parse', '--path-format=absolute', '--git-common-dir');
  mkdirSync(join(common, 'hotsheet-root-gate'));
  writeFileSync(join(common, 'hotsheet-root-gate', tree), `${tree}\n`);
  let claim = false,
    phase = 'done',
    handles = false,
    failBranchOnce = false;
  let workerLive = false,
    ciConclusion = 'success',
    ticketStatus = 'completed';
  const run = (binary, args, cwd) => {
    if (binary === 'hotsheet-cli') {
      if (args.includes('show'))
        return { status: 0, stdout: `---\nid: ${native}\nslug: ${slug}\nstatus: ${ticketStatus}\n---\n`, stderr: '' };
      if (args.includes('resolve'))
        return {
          status: 0,
          stdout: JSON.stringify({
            id: projectId,
            sources: [{ provider: 'git', locator: store, connection_id: connection }],
          }),
          stderr: '',
        };
      if (args.includes('status'))
        return {
          status: 0,
          stdout: JSON.stringify({
            state: {
              assignments:
                phase === null
                  ? {}
                  : {
                      operation: {
                        id: 'operation',
                        phase,
                        handoff: { kind: 'code', commit: source },
                        ticket: { connection_id: connection, native_id: native },
                        worker: 'worker',
                      },
                    },
              workers: workerLive ? { worker: { expires_at: Date.now() / 1000 + 3600 } } : {},
            },
            live_claims: claim ? [{ ticket: { connection_id: connection, native_id: native } }] : [],
          }),
          stderr: '',
        };
    }
    if (binary === 'gh')
      return {
        status: 0,
        stdout: JSON.stringify([
          {
            databaseId: 1,
            headSha: shipping,
            headBranch: 'main',
            event: 'push',
            workflowName: 'CI',
            status: 'completed',
            conclusion: ciConclusion,
            createdAt: '2026-11-09T00:00:00Z',
            url: 'https://example.invalid/1',
          },
        ]),
        stderr: '',
      };
    if (binary === 'lsof') return { status: handles ? 0 : 1, stdout: handles ? 'synthetic process\n' : '', stderr: '' };
    if (binary === 'git' && args[0] === 'branch' && failBranchOnce) {
      failBranchOnce = false;
      return { status: 1, stdout: '', stderr: 'synthetic interruption after worktree removal' };
    }
    return command(binary, args, cwd);
  };
  const options = { project, store, worktree, ticket: slug, shipping };
  return {
    root,
    project,
    store,
    worktree,
    branch,
    source,
    shipping,
    common,
    options,
    run,
    claim: (value) => {
      claim = value;
    },
    phase: (value) => {
      phase = value;
    },
    handles: (value) => {
      handles = value;
    },
    interrupt: () => {
      failBranchOnce = true;
    },
    worker: (value) => {
      workerLive = value;
    },
    ci: (value) => {
      ciConclusion = value;
    },
    status: (value) => {
      ticketStatus = value;
    },
    gate: join(common, 'hotsheet-root-gate', tree),
  };
}

test('HS2-HR3ZX4 removes only a clean verified direct worktree and branch', () => {
  const f = fixture();
  f.phase(null);
  mkdirSync(join(f.worktree, 'target'));
  writeFileSync(join(f.worktree, 'target', 'build-output'), 'synthetic build output\n');
  const result = cleanup(f.options, { run: f.run });
  assert.equal(result.state, 'removed');
  assert.ok(result.reclaimedBytes >= 0);
  assert.equal(existsSync(f.worktree), false);
  assert.equal(command('git', ['show-ref', '--verify', '--quiet', `refs/heads/${f.branch}`], f.project).status, 1);
  assert.equal(cleanup(f.options, { run: f.run }).state, 'already_removed');
});

test('HS2-HR3ZX4 refuses dirty, locked, claimed, active, and unmerged worktrees', () => {
  for (const condition of [
    'dirty',
    'locked',
    'claim',
    'handoff',
    'worker',
    'handles',
    'unmerged',
    'gate',
    'ci',
    'open',
  ]) {
    const f = fixture();
    f.phase(null);
    if (condition === 'dirty') writeFileSync(join(f.worktree, 'untracked.txt'), 'preserve\n');
    if (condition === 'locked') git(f.project, 'worktree', 'lock', f.worktree);
    if (condition === 'claim') f.claim(true);
    if (condition === 'handoff') f.phase('ci');
    if (condition === 'worker') {
      f.phase('done');
      f.worker(true);
    }
    if (condition === 'handles') f.handles(true);
    if (condition === 'gate') unlinkSync(f.gate);
    if (condition === 'ci') f.ci('failure');
    if (condition === 'open') f.status('started');
    if (condition === 'unmerged') {
      writeFileSync(join(f.worktree, 'later.txt'), 'not shipped\n');
      git(f.worktree, 'add', 'later.txt');
      git(f.worktree, 'commit', '-m', `${slug} Add unshipped change`);
    }
    assert.throws(
      () => cleanup(f.options, { run: f.run }),
      /worktree|claim|handoff|process|source commit|gate|CI|ticket|worker/i,
      condition,
    );
    assert.equal(existsSync(f.worktree), true, condition);
    assert.equal(command('git', ['show-ref', '--verify', '--quiet', `refs/heads/${f.branch}`], f.project).status, 0);
  }
});

test('HS2-HR3ZX4 requires a durable record for a reviewed operation', () => {
  const f = fixture();
  assert.throws(
    () => cleanup({ ...f.options, operation: 'operation' }, { run: f.run }),
    /integration record is missing/,
  );
  assert.equal(existsSync(f.worktree), true);
});

test('HS2-HR3ZX4 verifies a cherry-picked handoff record and exact reviewed branch', () => {
  const f = fixture({ cherryPick: true });
  const qualified = `@${projectId}/${connection}:${native}`;
  const recordDir = join(f.common, 'hotsheet-integrations', createHash('sha256').update(qualified).digest('hex'));
  mkdirSync(recordDir, { recursive: true });
  writeFileSync(
    join(recordDir, 'operation.json'),
    JSON.stringify({
      version: 1,
      operation: 'operation',
      branch: 'main',
      ticket: { project_id: projectId, connection_id: connection, native_id: native },
      source_commits: [f.source],
      shipping_commits: [f.shipping],
    }),
  );
  const proposalDir = join(f.common, 'hotsheet-ci-proposals');
  mkdirSync(proposalDir);
  const proposal = {
    operation: 'operation',
    slug,
    sourceWorktree: f.worktree,
    sourceBranch: f.branch,
    sourceCommit: f.source,
    integrated: f.shipping,
    ticket: { connection_id: connection, native_id: native },
  };
  writeFileSync(
    join(proposalDir, 'operation.json'),
    JSON.stringify({ record: { ...proposal, sourceBranch: 'wrong-branch' } }),
  );
  git(f.project, 'update-ref', `refs/hotsheet/integration-source/${f.source}`, f.source);
  assert.throws(
    () => cleanup({ ...f.options, operation: 'operation' }, { run: f.run }),
    /reviewed handoff does not map/,
  );
  assert.equal(existsSync(f.worktree), true);
  writeFileSync(join(proposalDir, 'operation.json'), JSON.stringify({ record: proposal }));
  assert.equal(cleanup({ ...f.options, operation: 'operation' }, { run: f.run }).state, 'removed');
});

test('HS2-HR3ZX4 resumes a branch deletion interrupted after worktree removal', () => {
  const f = fixture();
  f.phase(null);
  f.interrupt();
  assert.throws(() => cleanup(f.options, { run: f.run }), /synthetic interruption/);
  assert.equal(existsSync(f.worktree), false);
  assert.equal(command('git', ['show-ref', '--verify', '--quiet', `refs/heads/${f.branch}`], f.project).status, 0);
  const canonical = join(realpathSync(join(f.project, '.claude', 'worktrees')), f.branch);
  const intent = join(
    f.common,
    'hotsheet-worktree-cleanup',
    `${createHash('sha256').update(`${slug}\n${canonical}`).digest('hex')}.json`,
  );
  assert.equal(JSON.parse(readFileSync(intent, 'utf8')).branch, f.branch);
  assert.equal(cleanup(f.options, { run: f.run }).state, 'already_removed');
  assert.equal(command('git', ['show-ref', '--verify', '--quiet', `refs/heads/${f.branch}`], f.project).status, 1);
});

test('HS2-HR3ZX4 refuses an interrupted retry when its branch moved', () => {
  const f = fixture();
  f.phase(null);
  f.interrupt();
  assert.throws(() => cleanup(f.options, { run: f.run }), /synthetic interruption/);
  const base = git(f.project, 'rev-parse', `${f.source}^`);
  git(f.project, 'update-ref', `refs/heads/${f.branch}`, base);
  assert.throws(() => cleanup(f.options, { run: f.run }), /branch moved/);
  assert.equal(command('git', ['show-ref', '--verify', '--quiet', `refs/heads/${f.branch}`], f.project).status, 0);
});
