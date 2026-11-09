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
const nextNative = '01M00000000000000000000001';
const connection = 'synthetic-store';
const projectId = 'synthetic-checkout';

function git(cwd, ...args) {
  const result = spawnSync('git', args, { cwd, encoding: 'utf8', env: withoutGitRepositoryEnv(process.env) });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}

function fixture({ cherryPick = false, batch = false, superseded = false, differentReplayPatch = false, retiredFindings = false } = {}) {
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
  const smokeStages = [
    { cwd: '.', command: 'cargo fmt --all --check' },
    { cwd: 'clients/web', command: 'npm run format:sources:check' },
    { cwd: 'clients/web', command: 'npm run lint:web' },
    { cwd: 'clients/web', command: 'npm run typecheck' },
  ];
  mkdirSync(join(project, 'scripts'));
  writeFileSync(join(project, 'scripts/gate-smoke.json'), JSON.stringify({ stages: smokeStages }));
  git(project, 'add', 'base.txt', '.gitignore', 'scripts/gate-smoke.json');
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
  let replacement = null;
  if (superseded) {
    const replay = join(project, '.claude', 'worktrees', `${branch}-replay`);
    git(project, 'worktree', 'add', '-b', `${branch}-replay`, replay, `${source}^`);
    if (differentReplayPatch) {
      writeFileSync(join(replay, 'ticket.txt'), 'different synthetic change\n');
      git(replay, 'add', 'ticket.txt');
    } else git(replay, 'cherry-pick', '-n', source);
    git(replay, 'commit', '-m', `${slug} Replay synthetic change`);
    replacement = git(replay, 'rev-parse', 'HEAD');
  }
  if (cherryPick) {
    git(project, 'cherry-pick', '-n', branch);
    git(project, 'commit', '-m', `${slug} Integrate synthetic change`);
  } else git(project, 'merge', '--ff-only', branch);
  git(project, 'push', 'origin', 'main');
  const shipping = git(project, 'rev-parse', 'HEAD');
  const tree = git(project, 'rev-parse', 'HEAD^{tree}');
  const common = git(project, 'rev-parse', '--path-format=absolute', '--git-common-dir');
  const gateDir = git(worktree, 'rev-parse', '--path-format=absolute', '--git-path', 'hotsheet-root-gate');
  const gate = join(gateDir, tree);
  mkdirSync(gateDir);
  writeFileSync(gate, `${tree}\n`);
  const integratorGate = join(common, 'hotsheet-root-gate', tree);
  mkdirSync(join(common, 'hotsheet-root-gate'));
  writeFileSync(integratorGate, `${tree}\n`);
  const smokeChecks = [
    'cargo fmt --all --check',
    'clients/web npm run format:sources:check',
    'npm run lint:web',
    'npm run typecheck',
  ];
  const receiptKey = 'a'.repeat(64);
  const receiptPath = join(common, 'hotsheet-local-gate', `${receiptKey}.json`);
  const smokeReceipt = {
    key: receiptKey,
    tree,
    tier: 'smoke',
    profile: { smoke: true },
    selected: smokeStages.map((stage) => stage.command),
    results: smokeStages.map((stage) => ({ ...stage, status: 0 })),
  };
  if (batch) {
    const gateCheckout = join(project, '.claude', 'worktrees', 'synthetic-integrator');
    git(project, 'worktree', 'add', '-b', 'synthetic-integrator', gateCheckout, shipping);
    assert.equal(git(gateCheckout, 'rev-parse', '--git-common-dir'), common);
    mkdirSync(join(common, 'hotsheet-local-gate'));
    writeFileSync(receiptPath, JSON.stringify(smokeReceipt));
    // The primary checkout is deliberately dirty; the clean integration worktree ran the gate.
    writeFileSync(join(project, 'unrelated-local-edit.txt'), 'preserve\n');
  }
  const batchEntry = {
    state: 'green',
    main_commit: shipping,
    smoke_checks: smokeChecks,
    tickets: [{ assignment: superseded ? 'replacement' : 'operation', ticket: { connection_id: connection, native_id: native }, source_commit: replacement ?? source, shipping_commits: [shipping] }],
  };
  const extraBatches = {};
  let claim = false,
    phase = 'done',
    handles = false,
    failBranchOnce = false;
  let workerRefilled = false,
    ciConclusion = 'success',
    ticketStatus = 'completed',
    handoffCommit = source;
  const note = (heading, text, id = 'synthetic') => `<!-- hotsheet:note:begin ${id} created_at: 2026-11-09T00:00:00Z -->\n## ${heading}\n${text}\n<!-- hotsheet:note:end -->`;
  let notes = superseded ? [
    note('Integration handoff', `Assignment operation; branch ${branch}; commit ${source}; worktree ${realpathSync(worktree)}`),
    note('Replacement worker assignment', `Assignment: replacement; original ${source}; old assignment blocked and marked done`),
    note('Integration handoff', `Assignment replacement; commit ${replacement}; prior ${source}`),
  ] : retiredFindings ? [
    note('Findings handoff', 'Assignment old-findings; original findings', 'findings-note'),
    note('Review return', 'Reviewed findings handoff old-findings at base; original findings note findings-note', 'review-note'),
    note('Worker assignment: reviewed correction', 'Operation operation supersedes old-findings, review-returned with note review-note'),
    note('Coordination retirement', 'assignment: old-findings; handoff evidence: findings note findings-note; recovery or review-return note: review-note'),
  ] : [];
  let oldTransitions = superseded ? [
    { kind: 'blocked', at: '2026-11-09T00:00:00Z', actor: { role: 'worker' } },
    { kind: 'done', at: '2026-11-09T00:01:00Z', actor: { role: 'integrator' } },
  ] : [];
  let extraReplacement = false;
  let retiredPhase = 'done', retiredRecovery = 'review-note';
  const run = (binary, args, cwd, input) => {
    if (binary === 'hotsheet-cli') {
      if (args.includes('show'))
        return { status: 0, stdout: `---\nid: ${native}\nslug: ${slug}\nstatus: ${ticketStatus}\n---\n${notes.join('\n')}`, stderr: '' };
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
              assignments: {
                ...(retiredFindings ? {
                  'old-findings': {
                    id: 'old-findings', phase: retiredPhase, recovery_note: retiredRecovery,
                    handoff: { kind: 'findings', note_id: 'findings-note' },
                    ticket: { connection_id: connection, native_id: native }, worker: 'worker',
                    transitions: [
                      { kind: 'handoff', at: '2026-11-09T00:00:00Z', actor: { role: 'worker' } },
                      { kind: 'review_return', at: '2026-11-09T00:01:00Z', actor: { role: 'integrator' } },
                      { kind: 'done', at: '2026-11-09T00:04:00Z', actor: { role: 'integrator' } },
                    ],
                  },
                } : {}),
                ...(phase === null
                  ? {}
                  : {
                      operation: {
                        id: 'operation',
                        phase,
                        handoff: superseded ? null : { kind: 'code', commit: handoffCommit },
                        ticket: { connection_id: connection, native_id: native },
                        worker: 'worker',
                        transitions: retiredFindings ? [{ kind: 'offered', at: '2026-11-09T00:02:00Z' }] : oldTransitions,
                      },
                    }),
                ...(retiredFindings && extraReplacement ? { another: {
                  id: 'another', phase: 'done', handoff: { kind: 'code', commit: source },
                  ticket: { connection_id: connection, native_id: native }, worker: 'worker',
                  transitions: [{ kind: 'offered', at: '2026-11-09T00:03:00Z' }],
                } } : {}),
                ...(superseded ? {
                  replacement: {
                    id: 'replacement', phase: 'done', handoff: { kind: 'code', commit: replacement },
                    ticket: { connection_id: connection, native_id: native }, worker: 'worker',
                    transitions: [{ kind: 'offered', at: '2026-11-09T00:02:00Z' }],
                  },
                  ...(extraReplacement ? { another: {
                    id: 'another', phase: 'done', handoff: { kind: 'code', commit: replacement },
                    ticket: { connection_id: connection, native_id: native }, worker: 'worker',
                    transitions: [{ kind: 'offered', at: '2026-11-09T00:03:00Z' }],
                  } } : {}),
                } : {}),
                ...(workerRefilled
                  ? {
                      later: {
                        id: 'later',
                        phase: 'accepted',
                        ticket: { connection_id: connection, native_id: nextNative },
                        worker: 'worker',
                      },
                    }
                  : {}),
              },
              workers: workerRefilled ? { worker: { expires_at: Date.now() / 1000 + 3600 } } : {},
            },
            live_claims: [
              ...(claim ? [{ ticket: { connection_id: connection, native_id: native } }] : []),
              ...(workerRefilled ? [{ ticket: { connection_id: connection, native_id: nextNative } }] : []),
            ],
          }),
          stderr: '',
        };
      if (args.includes('batch-status'))
        return { status: 0, stdout: JSON.stringify({ batches: batch ? { synthetic: batchEntry, ...extraBatches } : {} }), stderr: '' };
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
    return command(binary, args, cwd, input);
  };
  const options = { project, store, worktree, ticket: slug, shipping };
  return {
    root,
    project,
    store,
    worktree,
    branch,
    source,
    replacement,
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
    refilled: (value) => {
      workerRefilled = value;
    },
    ci: (value) => {
      ciConclusion = value;
    },
    status: (value) => {
      ticketStatus = value;
    },
    handoff: (value) => {
      handoffCommit = value;
    },
    notes: (value) => { notes = value; },
    oldTransitions: (value) => { oldTransitions = value; },
    extraReplacement: (value) => { extraReplacement = value; },
    retiredPhase: (value) => { retiredPhase = value; },
    retiredRecovery: (value) => { retiredRecovery = value; },
    gate,
    integratorGate,
    receiptPath,
    smokeReceipt,
    batchEntry,
    addBatch: (id, entry) => { extraBatches[id] = entry; },
  };
}

test('HS2-GYSR6T requires the exact linked worktree gate receipt', () => {
  const f = fixture();
  f.phase(null);
  unlinkSync(f.gate);
  assert.throws(() => cleanup(f.options, { run: f.run }), /gate in this worktree/);
  assert.equal(existsSync(f.worktree), true);
});

test('HS2-GYSR6T requires the integrator receipt for a reviewed handoff', () => {
  const f = fixture();
  unlinkSync(f.integratorGate);
  assert.throws(() => cleanup({ ...f.options, operation: 'operation' }, { run: f.run }), /gate in the integrator/);
  assert.equal(existsSync(f.worktree), true);
});

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
  unlinkSync(f.gate);
  const qualified = `@${projectId}/${connection}:${native}`;
  const recordDir = join(f.common, 'hotsheet-integrations', createHash('sha256').update(qualified).digest('hex'));
  mkdirSync(recordDir, { recursive: true });
  writeFileSync(join(recordDir, 'operation.json'), JSON.stringify({
    version: 1,
    operation: 'operation',
    branch: 'main',
    ticket: { project_id: projectId, connection_id: connection, native_id: native },
    source_commits: [f.source],
    shipping_commits: [f.shipping],
  }));
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

function recordReviewedHandoff(f, { proposal = true } = {}) {
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
  if (proposal) {
    const proposalDir = join(f.common, 'hotsheet-ci-proposals');
    mkdirSync(proposalDir, { recursive: true });
    writeFileSync(join(proposalDir, 'operation.json'), JSON.stringify({ record: {
      operation: 'operation', slug, sourceWorktree: f.worktree, sourceBranch: f.branch,
      sourceCommit: f.source, integrated: f.shipping,
      ticket: { connection_id: connection, native_id: native },
    } }));
  }
  git(f.project, 'update-ref', `refs/hotsheet/integration-source/${f.source}`, f.source);
}

function recordSupersededReplay(f) {
  const qualified = `@${projectId}/${connection}:${native}`;
  const recordDir = join(f.common, 'hotsheet-integrations', createHash('sha256').update(qualified).digest('hex'));
  mkdirSync(recordDir, { recursive: true });
  writeFileSync(join(recordDir, 'replacement.json'), JSON.stringify({
    version: 1, operation: 'replacement', branch: 'main',
    ticket: { project_id: projectId, connection_id: connection, native_id: native },
    source_commits: [f.replacement], shipping_commits: [f.shipping],
  }));
  git(f.project, 'update-ref', `refs/hotsheet/integration-source/${f.replacement}`, f.replacement);
}

test('HS2-HQD4QZ removes a reviewed superseded source only after its exact replay shipped', () => {
  const f = fixture({ cherryPick: true, batch: true, superseded: true });
  recordSupersededReplay(f);
  unlinkSync(f.integratorGate);
  assert.equal(cleanup({ ...f.options, operation: 'operation' }, { run: f.run }).state, 'removed');
  assert.equal(existsSync(f.worktree), false);
  assert.equal(git(f.project, 'rev-parse', `refs/hotsheet/integration-source/${f.source}`), f.source);
});

test('HS2-HQD4QZ refuses ambiguous or unlinked superseded replays', () => {
  for (const tamper of ['notes', 'transition', 'ambiguous', 'record', 'patch', 'batch', 'receipt', 'claim', 'handles', 'dirty']) {
    const f = fixture({ cherryPick: true, batch: true, superseded: true, differentReplayPatch: tamper === 'patch' });
    recordSupersededReplay(f);
    if (tamper === 'notes') f.notes([]);
    if (tamper === 'transition') f.oldTransitions([]);
    if (tamper === 'ambiguous') f.extraReplacement(true);
    if (tamper === 'record') unlinkSync(join(f.common, 'hotsheet-integrations',
      createHash('sha256').update(`@${projectId}/${connection}:${native}`).digest('hex'), 'replacement.json'));
    if (tamper === 'batch') f.batchEntry.state = 'red';
    if (tamper === 'receipt') unlinkSync(f.receiptPath);
    if (tamper === 'claim') f.claim(true);
    if (tamper === 'handles') f.handles(true);
    if (tamper === 'dirty') writeFileSync(join(f.worktree, 'untracked.txt'), 'synthetic\n');
    assert.throws(() => cleanup({ ...f.options, operation: 'operation' }, { run: f.run }), undefined, tamper);
    assert.equal(existsSync(f.worktree), true, tamper);
    assert.equal(spawnSync('git', ['show-ref', '--verify', '--quiet', `refs/hotsheet/integration-source/${f.source}`],
      { cwd: f.project, env: withoutGitRepositoryEnv(process.env) }).status, 1);
  }
});

test('HS2-X4MM1P cleans a retired findings handoff only through its unique shipped replacement', () => {
  const f = fixture({ cherryPick: true, batch: true, retiredFindings: true });
  recordReviewedHandoff(f, { proposal: false });
  unlinkSync(f.integratorGate);
  assert.equal(cleanup({ ...f.options, operation: 'operation' }, { run: f.run }).state, 'removed');
});

test('HS2-X4MM1P refuses active, ambiguous, unmatched, or unshipped retired findings', () => {
  for (const tamper of ['active', 'ambiguous', 'review', 'replacement', 'recovery', 'batch', 'receipt']) {
    const f = fixture({ cherryPick: true, batch: true, retiredFindings: true });
    recordReviewedHandoff(f, { proposal: false });
    unlinkSync(f.integratorGate);
    if (tamper === 'active') f.retiredPhase('blocked');
    if (tamper === 'ambiguous') f.extraReplacement(true);
    if (tamper === 'review') f.notes([]);
    if (tamper === 'replacement') f.phase('ci');
    if (tamper === 'recovery') f.retiredRecovery('wrong-note');
    if (tamper === 'batch') f.batchEntry.state = 'red';
    if (tamper === 'receipt') unlinkSync(f.receiptPath);
    assert.throws(() => cleanup({ ...f.options, operation: 'operation' }, { run: f.run }), undefined, tamper);
    assert.equal(existsSync(f.worktree), true, tamper);
  }
});

test('HS2-VKGV08 and HS2-54M43C accept mixed ledger spellings backed by exact cwd results', () => {
  const f = fixture({ cherryPick: true, batch: true });
  recordReviewedHandoff(f, { proposal: false });
  unlinkSync(f.integratorGate);
  assert.equal(existsSync(join(f.project, 'unrelated-local-edit.txt')), true);
  assert.equal(cleanup({ ...f.options, operation: 'operation' }, { run: f.run }).state, 'removed');
  assert.equal(existsSync(f.worktree), false);
  assert.equal(existsSync(join(f.project, 'unrelated-local-edit.txt')), true);
});

test('HS2-8W51AA accepts an aborted attempt followed by one exact green shipped batch', () => {
  const f = fixture({ cherryPick: true, batch: true });
  recordReviewedHandoff(f, { proposal: false });
  unlinkSync(f.integratorGate);
  f.addBatch('aborted-attempt', { ...structuredClone(f.batchEntry), state: 'aborted', main_commit: null });
  assert.equal(cleanup({ ...f.options, operation: 'operation' }, { run: f.run }).state, 'removed');
  assert.equal(existsSync(f.worktree), false);
});

test('HS2-8W51AA still refuses ambiguous and non-green effective batch histories', () => {
  for (const history of ['two-green', 'only-aborted', 'aborted-then-red']) {
    const f = fixture({ cherryPick: true, batch: true });
    recordReviewedHandoff(f, { proposal: false });
    unlinkSync(f.integratorGate);
    if (history === 'two-green') f.addBatch('second-green', structuredClone(f.batchEntry));
    else {
      f.addBatch('aborted-attempt', { ...structuredClone(f.batchEntry), state: 'aborted' });
      f.batchEntry.state = history === 'only-aborted' ? 'aborted' : 'red';
    }
    assert.throws(() => cleanup({ ...f.options, operation: 'operation' }, { run: f.run }),
      /multiple batches|not green/, history);
    assert.equal(existsSync(f.worktree), true, history);
  }
});

test('HS2-AYPN79 cleans a completed handoff after its worker refills on another ticket', () => {
  const f = fixture({ cherryPick: true, batch: true });
  recordReviewedHandoff(f, { proposal: false });
  unlinkSync(f.integratorGate);
  f.refilled(true);
  assert.equal(cleanup({ ...f.options, operation: 'operation' }, { run: f.run }).state, 'removed');
  assert.equal(existsSync(f.worktree), false);
});

test('HS2-VKGV08 refuses dispatcher batches with mismatched coordinator or ledger source commits', () => {
  for (const tamper of ['coordinator', 'ledger']) {
    const f = fixture({ cherryPick: true, batch: true });
    recordReviewedHandoff(f, { proposal: false });
    unlinkSync(f.integratorGate);
    if (tamper === 'coordinator') f.handoff('0'.repeat(40));
    else f.batchEntry.tickets[0].source_commit = '0'.repeat(40);
    assert.throws(
      () => cleanup({ ...f.options, operation: 'operation' }, { run: f.run }),
      /exact source commit mapping/,
      tamper,
    );
    assert.equal(existsSync(f.worktree), true, tamper);
  }
});

test('HS2-VKGV08 and HS2-54M43C reject red, mismatched, and unsuccessful batch evidence', () => {
  for (const tamper of [
    'red',
    'wrong-commit',
    'wrong-shipping',
    'wrong-tree',
    'wrong-checks',
    'wrong-cwd',
    'wrong-order',
    'wrong-ledger-order',
    'wrong-result',
    'failed-check',
    'missing-receipt',
  ]) {
    const f = fixture({ cherryPick: true, batch: true });
    recordReviewedHandoff(f);
    unlinkSync(f.integratorGate);
    if (tamper === 'red') f.batchEntry.state = 'red';
    if (tamper === 'wrong-commit') f.batchEntry.main_commit = git(f.project, 'rev-parse', `${f.shipping}^`);
    if (tamper === 'wrong-shipping') f.batchEntry.tickets[0].shipping_commits = ['0'.repeat(40)];
    if (tamper === 'wrong-tree') f.smokeReceipt.tree = '0'.repeat(40);
    if (tamper === 'wrong-checks') f.smokeReceipt.selected = ['cargo check'];
    if (tamper === 'wrong-cwd') f.smokeReceipt.results[1].cwd = 'migrator';
    if (tamper === 'wrong-order') f.smokeReceipt.selected.reverse();
    if (tamper === 'wrong-ledger-order') f.batchEntry.smoke_checks.reverse();
    if (tamper === 'wrong-result') f.smokeReceipt.results[1].command = 'npm run wrong';
    if (tamper === 'failed-check') f.smokeReceipt.results[0].status = 1;
    if (tamper === 'missing-receipt') unlinkSync(f.receiptPath);
    else writeFileSync(f.receiptPath, JSON.stringify(f.smokeReceipt));
    assert.throws(
      () => cleanup({ ...f.options, operation: 'operation' }, { run: f.run }),
      /batch|smoke receipt/i,
      tamper,
    );
    assert.equal(existsSync(f.worktree), true, tamper);
  }
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
