import test from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, mkdtempSync, mkdirSync, readFileSync, realpathSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { classifyRuns, evaluate, parseTicket, refresh, register } from './ci-completion-proposal.mjs';

const source = 'a'.repeat(40);
const integrated = 'b'.repeat(40);
const later = 'c'.repeat(40);
const tree = 'd'.repeat(40);
const ref = { connection_id: 'git-one', native_id: '01M00000000000000000000000' };
const run = (id, sha, status, conclusion, createdAt, overrides = {}) => ({
  databaseId: id, headSha: sha, headBranch: 'main', event: 'push', workflowName: 'CI',
  status, conclusion, createdAt, url: `https://example.test/runs/${id}`, ...overrides,
});

function markdown(extra = '') {
  return `---\nid: ${ref.native_id}\nslug: HS2-1C1565\nstatus: started\nupdated_at: 2026-10-28T00:00:00Z\nfollow_ups:\n- project_id: project\n  connection_id: git-two\n  native_id: 01M11111111111111111111111\n---\n<!-- hotsheet:body:begin -->\nSynthetic scope\n<!-- hotsheet:body:end -->\n<!-- hotsheet:notes:begin -->\n${extra}<!-- hotsheet:notes:end -->\n`;
}

const connectionFor = (path) => createHash('sha256').update(realpathSync(path)).digest('hex').slice(0, 16);

function note(id, body) {
  return `<!-- hotsheet:note:begin ${id} created_at: 2026-10-28T00:00:00Z -->\n${body}\n<!-- hotsheet:note:end -->\n`;
}

test('HS2-1C1565 parses scope and qualified follow-ups without copying ticket prose', () => {
  const parsed = parseTicket(markdown(note('review-1', '## Review return\nSynthetic finding')));
  assert.equal(parsed.slug, 'HS2-1C1565');
  assert.deepEqual(parsed.followUps, ['@project/git-two:01M11111111111111111111111']);
  assert.deepEqual(parsed.reviewFindings, ['review-1']);
  assert.ok(!JSON.stringify(parsed).includes('Synthetic'));
});

test('HS2-1C1565 only a successful covering main push CI run is green', () => {
  const runs = [
    run(5, source, 'completed', 'success', '2026-10-28T00:00:00Z', { headBranch: 'worker' }),
    run(6, integrated, 'completed', 'failure', '2026-10-28T01:00:00Z'),
    run(7, later, 'in_progress', '', '2026-10-28T02:00:00Z'),
  ];
  const covers = (_, head) => head === source ? 'uncovered' : 'covered';
  const pending = classifyRuns(runs, integrated, covers);
  assert.equal(pending.state, 'pending');
  assert.deepEqual(pending.runs.map((entry) => entry.id), [6, 7]);
  assert.equal(classifyRuns([runs[0], runs[1]], integrated, covers).state, 'failure');
  assert.equal(classifyRuns([runs[1], run(8, later, 'completed', 'cancelled', '2026-10-28T03:00:00Z'), run(9, source, 'in_progress', '', '2026-10-28T04:00:00Z')], integrated, covers).state, 'superseded');
  assert.equal(classifyRuns([run(9, later, 'completed', 'cancelled', '2026-10-28T03:00:00Z')], integrated, covers).state, 'cancelled');
  const recovered = classifyRuns([runs[1], run(10, later, 'completed', 'success', '2026-10-28T04:00:00Z')], integrated, covers);
  assert.equal(recovered.state, 'success');
  assert.deepEqual(classifyRuns([run(10, later, 'completed', 'success', '2026-10-28T04:00:00Z'), runs[1]], integrated, covers).runs.map((entry) => entry.id), [6, 10]);
  assert.equal(classifyRuns([run(10, later, 'completed', 'success', '2026-10-28T04:00:00Z')], source, () => 'covered').runs[0].id, 10);
  assert.equal(classifyRuns([runs[1], run(11, later, 'completed', 'success', '2026-10-28T05:00:00Z')], integrated, () => 'unknown').state, 'unavailable');
  assert.equal(classifyRuns([run(12, later, 'completed', 'success', '2026-10-28T06:00:00Z')], integrated, () => { throw new Error('git unavailable'); }).state, 'unavailable');
  assert.equal(classifyRuns(null, integrated, covers).state, 'unavailable');
});

function fixture({ kind = 'code', phase = 'ci', review = '', dirty = false, head = source, sourceWorktree = true, projectId } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'hs2-1c1565-'));
  const project = join(root, 'project'), store = join(root, 'store'), gitDir = join(root, 'git'), worktree = join(root, 'worker');
  for (const path of [project, store, gitDir, worktree, join(gitDir, 'hotsheet-root-gate')]) mkdirSync(path, { recursive: true });
  const ticketRef = { ...ref, connection_id: connectionFor(store) };
  writeFileSync(join(gitDir, 'hotsheet-root-gate', tree), `${tree}\n`);
  let ticketText = markdown(kind === 'findings' ? note('findings-1', '## Findings handoff\nSynthetic conclusion') : review);
  const assignment = {
    id: 'operation-1', ticket: ticketRef, slug: 'HS2-1C1565', generation: 'worker-generation', phase,
    handoff: kind === 'code' ? { kind, commit: source } : { kind, note_id: 'findings-1' },
    transitions: [{ kind: 'handoff', at: '2026-10-28T00:00:00Z' }, { kind: 'ci', at: '2026-10-28T00:10:00Z' }],
  };
  let remoteContains = true;
  let coverage = 'covered';
  let mapping = true;
  let providerAvailable = true;
  let activeClaims = [];
  let integrator = { id: 'integrator', generation: 'integrator-generation' };
  const calls = [];
  const fake = (binary, args, cwd, input) => {
    calls.push({ binary, args, cwd });
    const ok = (stdout = '') => ({ status: 0, stdout, stderr: '' });
    const fail = () => ({ status: 1, stdout: '', stderr: 'synthetic failure' });
    if (binary === 'hotsheet-cli' && args.at(-2) === 'show') return providerAvailable ? ok(ticketText) : fail();
    if (binary === 'hotsheet-cli' && args.at(-1) === 'status') return ok(JSON.stringify({ state: { project_id: projectId, assignments: { 'operation-1': assignment }, integrator }, live_integrator: true, live_claims: activeClaims, cursor: 'unrelated-cursor' }));
    if (binary !== 'git') throw new Error(`unexpected ${binary}`);
    if (args[0] === 'rev-parse' && args[1] === '--git-common-dir') return ok(gitDir);
    if (args[0] === 'rev-parse' && args[1] === '--git-path') return ok(join(gitDir, 'hotsheet-root-gate'));
    if (args[0] === 'rev-parse' && args[1] === `${integrated}^{commit}`) return ok(integrated);
    if (args[0] === 'rev-parse' && args[1] === `${integrated}^{tree}`) return ok(tree);
    if (args[0] === 'rev-parse' && args[1] === 'HEAD') return ok(head);
    if (args[0] === 'symbolic-ref') return ok('main');
    if (args[0] === 'status') return ok(dirty ? ' M source.rs' : '');
    if (args[0] === 'worktree') return ok(sourceWorktree ? `worktree ${worktree}\nHEAD ${head}\nbranch refs/heads/worker\n` : '');
    if (args[0] === 'show') return ok(`diff --git a/file b/file\n${args[2]}\n`);
    if (args[0] === 'patch-id') return ok(`${mapping || input?.includes(source) ? 'f'.repeat(40) : 'e'.repeat(40)} ${source}`);
    if (args[0] === 'ls-remote') return ok(`${remoteContains ? integrated : source}\trefs/heads/main\n`);
    if (args[0] === 'merge-base' && args[1] === '--is-ancestor' && args[3] === source) return fail();
    if (args[0] === 'merge-base') return coverage === 'covered' ? ok() : fail();
    if (args[0] === 'cat-file') return ok();
    throw new Error(`unexpected git ${args.join(' ')}`);
  };
  return { project, store, gitDir, worktree, assignment, ticketRef, fake, calls,
    setTicket: (value) => { ticketText = value; },
    setRemote: (value) => { remoteContains = value; },
    setCoverage: (value) => { coverage = value; },
    setMapping: (value) => { mapping = value; },
    setProvider: (value) => { providerAvailable = value; },
    setClaims: (value) => { activeClaims = value; },
    setIntegrator: (value) => { integrator = value; },
  };
}

test('HS2-1C1565 refreshes a durable proposal without duplicate writes and rechecks evidence', () => {
  const f = fixture();
  const path = register({ project: f.project, store: f.store, assignment: 'operation-1', integrated, worktree: f.worktree }, f.fake);
  const green = [run(1, integrated, 'completed', 'success', '2026-10-28T01:00:00Z')];
  const proposal = refresh(path, { run: f.fake, runs: green });
  assert.equal(proposal.ready, true);
  assert.equal(proposal.integrated.mapping, 'stable_patch_id');
  assert.equal(proposal.integrated.gateReceipt, true);
  assert.equal(proposal.sourceWorktree.state, 'clean');
  assert.equal(proposal.measurements.handoffToCiMs, 600_000);
  assert.equal(proposal.measurements.handoffToCompletionMs, null);
  const mtime = statSync(path).mtimeMs;
  refresh(path, { run: f.fake, runs: green });
  assert.equal(statSync(path).mtimeMs, mtime);
  f.setRemote(false);
  assert.match(refresh(path, { run: f.fake, runs: green }).reasons.join(' '), /remote main ancestry/);
  assert.ok(f.calls.some(({ binary, args }) => binary === 'git' && args[0] === 'ls-remote'));
  f.setRemote(true);
  f.setTicket(markdown(note('review-2', '## Review return\nNew finding')));
  assert.match(refresh(path, { run: f.fake, runs: green }).reasons.join(' '), /ticket changed/);
  assert.ok(readFileSync(path, 'utf8').includes('review-2'));
});

test('HS2-39D5YS requires a qualified durable mapping and accepts a reviewed unequal patch', () => {
  const f = fixture({ projectId: 'project' });
  const path = register({ project: f.project, store: f.store, assignment: 'operation-1', integrated, worktree: f.worktree }, f.fake);
  const green = [run(1, integrated, 'completed', 'success', '2026-10-28T01:00:00Z')];
  assert.match(refresh(path, { run: f.fake, runs: green }).reasons.join(' '), /durable integration mapping/);
  f.setMapping(false);
  const qualified = `@project/${f.ticketRef.connection_id}:${f.ticketRef.native_id}`;
  const directory = join(f.gitDir, 'hotsheet-integrations', createHash('sha256').update(qualified).digest('hex'));
  mkdirSync(directory, { recursive: true });
  const ledgerPath = join(directory, 'operation-1.json');
  const evidence = { version: 1, operation: 'operation-1', ticket: { project_id: 'wrong-project', ...f.ticketRef }, remote: 'origin', branch: 'main', source_commits: [source], shipping_commits: [integrated], remote_tip: integrated, recorded_at: '2026-10-28T00:00:00Z' };
  writeFileSync(ledgerPath, JSON.stringify(evidence));
  assert.match(refresh(path, { run: f.fake, runs: green }).reasons.join(' '), /mapping/);
  evidence.ticket.project_id = 'project';
  writeFileSync(ledgerPath, JSON.stringify(evidence));
  const ready = refresh(path, { run: f.fake, runs: green });
  assert.equal(ready.ready, true);
  assert.equal(ready.integrated.mapping, 'reviewed_integration_record');
  f.setRemote(false);
  assert.match(refresh(path, { run: f.fake, runs: green }).reasons.join(' '), /remote main ancestry/);
});

test('HS2-1C1565 blocks dirty, missing, unmapped, returned and stale-generation cleanup', () => {
  const f = fixture({ dirty: true });
  const path = register({ project: f.project, store: f.store, assignment: 'operation-1', integrated, worktree: f.worktree }, f.fake);
  const green = [run(1, integrated, 'completed', 'success', '2026-10-28T01:00:00Z')];
  assert.match(refresh(path, { run: f.fake, runs: green }).reasons.join(' '), /worktree/);
  f.assignment.phase = 'blocked';
  assert.match(refresh(path, { run: f.fake, runs: green }).reasons.join(' '), /blocked operation/);
  f.assignment.phase = 'ci';
  f.assignment.generation = 'replacement';
  assert.match(refresh(path, { run: f.fake, runs: green }).reasons.join(' '), /generation changed/);
  f.assignment.generation = 'worker-generation';
  f.setMapping(false);
  assert.match(refresh(path, { run: f.fake, runs: green }).reasons.join(' '), /mapping/);
  f.setMapping(true);
  f.setClaims([{ ticket: f.ticketRef, worker: 'another-worker', expires_at: 1800000000 }]);
  assert.match(refresh(path, { run: f.fake, runs: green }).reasons.join(' '), /active claim/);
  f.setClaims([]);
  f.setIntegrator({ id: 'integrator', generation: 'replacement' });
  assert.match(refresh(path, { run: f.fake, runs: green }).reasons.join(' '), /integrator session changed/);
});

test('HS2-1C1565 findings-only handoff has no fabricated CI or worktree gate', () => {
  const f = fixture({ kind: 'findings', phase: 'verifying' });
  const path = register({ project: f.project, store: f.store, assignment: 'operation-1' }, f.fake);
  const proposal = refresh(path, { run: f.fake, runs: null });
  assert.equal(proposal.ready, true);
  assert.equal(proposal.ci.state, 'not_required');
  assert.equal(proposal.integrated, null);
  assert.equal(proposal.sourceWorktree.state, 'none');
  assert.equal(f.calls.some(({ binary }) => binary === 'gh'), false);
});

test('HS2-1C1565 unavailable provider evidence remains explicit and cannot approve completion', () => {
  const f = fixture();
  const path = register({ project: f.project, store: f.store, assignment: 'operation-1', integrated, worktree: f.worktree }, f.fake);
  f.setProvider(false);
  const proposal = refresh(path, { run: f.fake, runs: [run(1, integrated, 'completed', 'success', '2026-10-28T01:00:00Z')] });
  assert.equal(proposal.ready, false);
  assert.equal(proposal.ticket.status, 'unavailable');
  assert.match(proposal.reasons.join(' '), /provider evidence is unavailable/);
});

test('HS2-1C1565 rejects main checkout as cleanup target and same-slug wrong-store tickets', () => {
  const main = fixture();
  const green = [run(1, integrated, 'completed', 'success', '2026-10-28T01:00:00Z')];
  assert.throws(() => register({ project: main.project, store: main.store, assignment: 'operation-1', integrated, worktree: main.project }, main.fake), /must not be the main checkout/);
  const mainPath = register({ project: main.project, store: main.store, assignment: 'operation-1', integrated, worktree: main.worktree }, main.fake);
  const corrupted = JSON.parse(readFileSync(mainPath, 'utf8'));
  corrupted.record.sourceWorktree = main.project;
  writeFileSync(mainPath, JSON.stringify(corrupted));
  const mainProposal = refresh(mainPath, { run: main.fake, runs: green });
  assert.equal(mainProposal.ready, false);
  assert.equal(mainProposal.sourceWorktree.state, 'main_checkout');
  assert.equal(mainProposal.cleanup, null);

  const wrongConnection = fixture();
  wrongConnection.assignment.ticket.connection_id = 'another-store';
  const wrongPath = register({ project: wrongConnection.project, store: wrongConnection.store, assignment: 'operation-1', integrated, worktree: wrongConnection.worktree }, wrongConnection.fake);
  const wrongProposal = refresh(wrongPath, { run: wrongConnection.fake, runs: green });
  assert.equal(wrongProposal.ticket.identityMatches, false);
  assert.match(wrongProposal.reasons.join(' '), /selected store does not own/);

  const wrongId = fixture();
  wrongId.setTicket(markdown().replace(ref.native_id, '01M22222222222222222222222'));
  const wrongIdPath = register({ project: wrongId.project, store: wrongId.store, assignment: 'operation-1', integrated, worktree: wrongId.worktree }, wrongId.fake);
  const wrongIdProposal = refresh(wrongIdPath, { run: wrongId.fake, runs: green });
  assert.equal(wrongIdProposal.ticket.identityMatches, false);
  assert.equal(wrongIdProposal.ready, false);
});

test('HS2-1C1565 evaluate rejects stale ticket and independent gate evidence', () => {
  const record = { operation: 'op', ticket: ref, workerGeneration: 'worker', integrator: { id: 'integrator', generation: 'one' }, ticketRevision: 'one', kind: 'code', sourceCommit: source, integrated };
  const evidence = {
    assignment: { id: 'op', ticket: ref, generation: 'worker', phase: 'ci', handoff: { kind: 'code', commit: source } },
    integrator: { id: 'integrator', generation: 'one' }, integratorLive: true,
    ticket: { revision: 'two', status: 'started' }, ticketIdentityMatches: true, outstandingReview: ['review-1'], integrated, tree, mapping: 'identity',
    mainBranch: 'main', mainContains: true, remoteAncestry: 'covered', gateReceipt: false, ci: { state: 'success' }, sourceWorktree: { state: 'clean' },
  };
  assert.deepEqual(evaluate(record, evidence).reasons, [
    'ticket changed since registration; review current scope, notes and follow-ups',
    'unresolved review notes: review-1',
    'integrated main commit or tree changed',
    'exact integrated tree has no successful local root gate receipt',
  ]);
});

test('HS2-1C1565 real CLI process replays a registered proposal and refuses changed scope', () => {
  const root = mkdtempSync(join(tmpdir(), 'hs2-1c1565-process-'));
  const project = join(root, 'project'), bare = join(root, 'origin.git'), worker = join(root, 'worker'), bin = join(root, 'bin'), store = join(root, 'store');
  for (const path of [project, bin, store]) mkdirSync(path, { recursive: true });
  const ticketRef = { ...ref, connection_id: connectionFor(store) };
  const git = (cwd, ...args) => {
    const result = spawnSync('git', args, { cwd, encoding: 'utf8' });
    assert.equal(result.status, 0, `${args.join(' ')}: ${result.stderr}`);
    return result.stdout.trim();
  };
  git(root, 'init', '--bare', bare);
  git(root, 'init', '-b', 'main', project);
  git(project, 'config', 'user.name', 'Synthetic Test');
  git(project, 'config', 'user.email', 'synthetic@example.test');
  writeFileSync(join(project, 'base.txt'), 'base\n');
  git(project, 'add', 'base.txt');
  git(project, 'commit', '-m', 'synthetic base');
  const oldSha = git(project, 'rev-parse', 'HEAD');
  writeFileSync(join(project, 'source.txt'), 'synthetic\n');
  git(project, 'add', 'source.txt');
  git(project, 'commit', '-m', 'synthetic source');
  const sha = git(project, 'rev-parse', 'HEAD');
  const testedTree = git(project, 'rev-parse', 'HEAD^{tree}');
  git(project, 'remote', 'add', 'origin', bare);
  git(project, 'push', '-u', 'origin', 'main');
  git(project, 'update-ref', 'refs/remotes/origin/main', oldSha);
  assert.equal(git(project, 'rev-parse', 'origin/main'), oldSha);
  git(project, 'worktree', 'add', '-b', 'worker', worker, sha);
  const receiptDir = join(project, '.git', 'hotsheet-root-gate');
  mkdirSync(receiptDir);
  writeFileSync(join(receiptDir, testedTree), `${testedTree}\n`);
  writeFileSync(join(root, 'ticket'), markdown());
  writeFileSync(join(root, 'status'), JSON.stringify({ state: { integrator: { id: 'integrator', generation: 'gen-1' }, assignments: {
    'process-op': { id: 'process-op', ticket: ticketRef, slug: 'HS2-1C1565', generation: 'worker-gen', phase: 'ci', handoff: { kind: 'code', commit: sha } },
  } }, live_integrator: true, live_claims: [], cursor: 'cursor-1' }));
  writeFileSync(join(root, 'runs'), JSON.stringify([run(42, sha, 'completed', 'success', '2026-10-28T01:00:00Z')]));
  const cli = join(bin, 'hotsheet-cli'), gh = join(bin, 'gh');
  writeFileSync(cli, '#!/bin/sh\nfor arg do last="$arg"; done\ncase "$last" in show) exit 1;; status) cat "$HS2_FIXTURE_DIR/status";; HS2-1C1565) cat "$HS2_FIXTURE_DIR/ticket";; *) exit 1;; esac\n');
  writeFileSync(gh, '#!/bin/sh\ncat "$HS2_FIXTURE_DIR/runs"\n');
  chmodSync(cli, 0o755); chmodSync(gh, 0o755);
  const script = new URL('./ci-completion-proposal.mjs', import.meta.url).pathname;
  const env = { ...process.env, PATH: `${bin}:${process.env.PATH}`, HS2_FIXTURE_DIR: root };
  const invoke = (...args) => spawnSync(process.execPath, [script, ...args], { cwd: project, env, encoding: 'utf8' });
  const first = invoke('register', '--project', project, '--store', store, '--assignment', 'process-op', '--integrated', sha, '--worktree', worker);
  assert.equal(first.status, 0, `${first.stderr}\n${first.stdout}`);
  const { path, proposal } = JSON.parse(first.stdout);
  assert.equal(proposal.ready, true);
  assert.equal(proposal.ci.runs[0].id, 42);
  const before = statSync(path).mtimeMs;
  const replay = invoke('refresh', '--project', project, '--assignment', 'process-op');
  assert.equal(replay.status, 0, replay.stderr);
  assert.equal(statSync(path).mtimeMs, before);
  writeFileSync(join(root, 'ticket'), markdown(note('scope-change', '## Review return\nSynthetic issue')));
  const stale = invoke('refresh', '--project', project, '--assignment', 'process-op');
  assert.equal(stale.status, 2, stale.stderr);
  assert.match(stale.stdout, /ticket changed since registration/);
});
