import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';
import { waitingCommit } from './ci-gate.mjs';
import {
  MARKER,
  attributionManifest,
  batchRun,
  ci,
  manifestNote,
  parseArgs,
  record,
  recover,
} from './integration-batch.mjs';

const MAIN = 'b'.repeat(40);
const SOURCE_1 = '1'.repeat(40);
const SOURCE_2 = '2'.repeat(40);
const SHIP_1 = 'c'.repeat(40);

function batch(state = 'ci', extra = {}) {
  return {
    id: 'batch-1',
    integrator: 'integrator',
    generation: 'session',
    state,
    opened_at: 1,
    main_commit: MAIN,
    smoke_checks: ['cargo lint', 'npm run typecheck'],
    tickets: [
      {
        assignment: 'op-1',
        ticket: { connection_id: 'git-local', native_id: 'native-1' },
        slug: 'HS2-AAA111',
        source_commit: SOURCE_1,
        paths: ['src/a.rs', 'docs/a.md#heading:x'],
        shipping_commits: [SHIP_1],
      },
      {
        assignment: 'op-2',
        ticket: { connection_id: 'git-local', native_id: 'native-2' },
        slug: 'HS2-BBB222',
        source_commit: SOURCE_2,
        paths: ['src/b.rs'],
        shipping_commits: [MAIN],
      },
    ],
    ...extra,
  };
}

const options = {
  cli: 'hotsheet-cli',
  store: '/synthetic/store',
  project: '/synthetic/project',
  batch: 'batch-1',
  integratorId: 'integrator',
  integratorSession: 'session',
  attribution: '/synthetic/ci-failure-attribution.mjs',
};

/** A fake process runner: routes by program and subcommand and records every call. */
function fake({ ledger = { version: 1, batches: { 'batch-1': batch() } }, runs = [], green = [], shown = {}, attribution } = {}) {
  const calls = [];
  const reply = (stdout, status = 0) => ({ status, stdout: typeof stdout === 'string' ? stdout : JSON.stringify(stdout), stderr: '' });
  const run = (binary, args, extra = {}) => {
    calls.push({ binary, args, input: extra.input });
    if (binary === 'gh') return reply(args.includes('--status') ? green : runs);
    if (binary === process.execPath) {
      const manifest = JSON.parse(readFileSync(args[args.indexOf('--manifest') + 1], 'utf8'));
      calls.at(-1).manifest = manifest;
      return reply(attribution);
    }
    const sub = args[args.indexOf('--project') + 2] ?? args[2];
    if (args.includes('coordination')) {
      if (sub === 'batch-status') return reply(ledger);
      if (sub === 'status') return reply({ state: { project_id: 'hotsheet2-0123456789ab' } });
      if (sub === 'batch-result') {
        const conclusion = args[args.indexOf('--conclusion') + 1];
        const current = ledger.batches['batch-1'];
        current.state = conclusion === 'success' ? 'green' : 'red';
        current.run = args[args.indexOf('--run') + 1];
        return reply({ batch: current, breaker: { consecutive_red: current.state === 'red' ? 1 : 0 }, notify: null });
      }
      if (sub === 'batch-follow-ups') {
        const filed = JSON.parse(readFileSync(args[args.indexOf('--attribution-file') + 1], 'utf8')).filed;
        return reply({
          ...ledger.batches['batch-1'],
          attributed: true,
          failures: filed.filter((f) => f.kind !== 'flaky').map((f) => ({ key: f.key, follow_up: f.slug })),
        });
      }
      if (sub === 'batch-recover') return reply({ consecutive_red: 0 });
    }
    if (args[2] === 'show') return reply(shown[args[3]] ?? '---\nstatus: started\n---\n');
    if (args[2] === 'edit') return reply('Updated');
    throw new Error(`unexpected call ${binary} ${args.join(' ')}`);
  };
  return { run, calls };
}

function scratch(t) {
  const directory = mkdtempSync(join(tmpdir(), 'hs2-9g0m3v-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  return directory;
}

test('HS2-9G0M3V the manifest note carries the CI gate commit ci-gate reads and every batch fact', () => {
  const note = manifestNote(batch(), batch().tickets[0], 'hotsheet2-0123456789ab');
  assert.ok(note.startsWith('## Batch manifest\n'));
  assert.ok(note.endsWith(`${MARKER} batch-1`));
  const manifest = JSON.parse(/```json\n([\s\S]*?)\n```/.exec(note)[1]);
  assert.deepEqual(manifest, {
    batch_id: 'batch-1',
    ticket: '@hotsheet2-0123456789ab/git-local:native-1',
    slug: 'HS2-AAA111',
    operation: 'op-1',
    source_commits: [SOURCE_1],
    shipping_commits: [SHIP_1],
    main_commit: MAIN,
    members: ['HS2-AAA111', 'HS2-BBB222'],
    smoke_checks: ['cargo lint', 'npm run typecheck'],
  });
  const ticket = [
    '---',
    'status: started',
    'started_phase: final_testing',
    '---',
    '<!-- hotsheet:notes:begin -->',
    note,
    '<!-- hotsheet:notes:end -->',
  ].join('\n');
  assert.equal(waitingCommit(ticket), MAIN);
});

test('HS2-9G0M3V record writes one manifest note and integration record per ticket, idempotently', () => {
  const recorded = [];
  const { run, calls } = fake({ shown: { 'HS2-BBB222': `---\n---\n${MARKER} batch-1\n` } });
  const result = record(options, { run, recordIntegrationFn: (input) => recorded.push(input) });
  assert.deepEqual(
    result.tickets.map((ticket) => [ticket.slug, ticket.note]),
    [
      ['HS2-AAA111', 'added'],
      ['HS2-BBB222', 'existing'],
    ],
  );
  const edits = calls.filter((call) => call.args[2] === 'edit');
  assert.equal(edits.length, 1);
  assert.deepEqual(edits[0].args.slice(3), ['HS2-AAA111', '--note-file', '-', '--actor-role', 'ai', '--actor-id', 'integrator']);
  assert.ok(edits[0].input.includes(`CI gate commit: ${MAIN}`));
  assert.deepEqual(recorded, [
    {
      project: '/synthetic/project',
      ticket: '@hotsheet2-0123456789ab/git-local:native-1',
      operation: 'op-1',
      source: [SOURCE_1],
      shipping: [SHIP_1],
    },
    {
      project: '/synthetic/project',
      ticket: '@hotsheet2-0123456789ab/git-local:native-2',
      operation: 'op-2',
      source: [SOURCE_2],
      shipping: [MAIN],
    },
  ]);
  for (const state of ['integrating', 'aborted']) {
    const early = fake({ ledger: { version: 1, batches: { 'batch-1': batch(state) } } });
    assert.throws(() => record(options, { run: early.run, recordIntegrationFn: () => {} }), /after coordination batch-pushed/);
  }
});

test('HS2-9G0M3V ci records nothing while the batch run is missing, running or cancelled', () => {
  const completed = (conclusion) => ({
    databaseId: 9,
    status: 'completed',
    conclusion,
    headSha: MAIN,
    event: 'push',
    workflowName: 'CI',
    createdAt: '2026-11-05T00:00:00Z',
  });
  for (const [runs, status] of [
    [[], 'pending'],
    [[{ ...completed(null), status: 'in_progress' }], 'pending'],
    [[{ ...completed('success'), headSha: 'd'.repeat(40) }], 'pending'],
    [[completed('cancelled')], 'inconclusive'],
  ]) {
    const { run, calls } = fake({ runs });
    assert.equal(ci(options, { run }).status, status);
    assert.ok(!calls.some((call) => call.args.includes('batch-result')));
  }
});

test('HS2-9G0M3V a green batch run records success once and makes every ticket completable', () => {
  const { run, calls } = fake({
    runs: [
      { databaseId: 7, status: 'completed', conclusion: 'failure', headSha: MAIN, event: 'push', workflowName: 'CI', createdAt: '2026-11-05T00:00:00Z' },
      { databaseId: 8, status: 'completed', conclusion: 'success', headSha: MAIN, event: 'push', workflowName: 'CI', createdAt: '2026-11-05T01:00:00Z' },
    ],
  });
  const result = ci(options, { run });
  assert.equal(result.status, 'green');
  assert.equal(result.run, 8);
  assert.equal(result.completable, true);
  const recorded = calls.find((call) => call.args.includes('batch-result')).args;
  assert.deepEqual(recorded.slice(recorded.indexOf('batch-result')), [
    'batch-result',
    '--id',
    'integrator',
    '--session',
    'session',
    '--batch',
    'batch-1',
    '--run',
    '8',
    '--conclusion',
    'success',
  ]);
  // A concluded batch is reported without another gh or result call.
  const again = ci(options, { run });
  assert.equal(again.status, 'green');
});

test('HS2-9G0M3V a red batch run is attributed and its non-flaky follow-ups recorded', (t) => {
  const red = { databaseId: 11, status: 'completed', conclusion: 'failure', headSha: MAIN, event: 'push', workflowName: 'CI', createdAt: '2026-11-05T00:00:00Z' };
  const { run, calls } = fake({
    runs: [red],
    attribution: {
      filed: [
        { key: 'k1', kind: 'failure', slug: 'HS2-CCC333', links: ['HS2-AAA111'] },
        { key: 'k2', kind: 'flaky', slug: 'HS2-DDD444', links: [] },
      ],
    },
  });
  const result = ci(options, { run, scratch: scratch(t) });
  assert.equal(result.status, 'red');
  assert.equal(result.completable, true);
  assert.deepEqual(result.follow_ups, ['HS2-CCC333']);
  const attribution = calls.find((call) => call.binary === process.execPath);
  assert.equal(attribution.args[0], '/synthetic/ci-failure-attribution.mjs');
  assert.equal(attribution.args[attribution.args.indexOf('--run') + 1], '11');
  assert.equal(attribution.manifest.version, 1);
  assert.equal(attribution.manifest.shipping_sha, MAIN);
  assert.deepEqual(attribution.manifest.tickets[0].ticket, {
    project_id: 'hotsheet2-0123456789ab',
    connection_id: 'git-local',
    native_id: 'native-1',
  });
  // Section scopes are not files.
  assert.deepEqual(attribution.manifest.tickets[0].changed_files, ['src/a.rs']);
  assert.ok(calls.some((call) => call.args.includes('batch-follow-ups')));
});

test('HS2-9G0M3V a red run without filed failures stays blocked, and an attributed replay skips filing', (t) => {
  const red = { databaseId: 12, status: 'completed', conclusion: 'timed_out', headSha: MAIN, event: 'push', workflowName: 'CI', createdAt: '2026-11-05T00:00:00Z' };
  const empty = fake({ runs: [red], attribution: { filed: [{ key: 'k2', kind: 'flaky', slug: 'HS2-DDD444' }] } });
  const blocked = ci(options, { run: empty.run, scratch: scratch(t) });
  assert.equal(blocked.completable, false);
  assert.match(blocked.reason, /file one by hand/);
  assert.ok(!empty.calls.some((call) => call.args.includes('batch-follow-ups')));
  const attributed = fake({ runs: [red], ledger: { version: 1, batches: { 'batch-1': batch('red', { attributed: true }) } } });
  const replay = ci(options, { run: (binary, args, extra) => {
    const result = attributed.run(binary, args, extra);
    if (args.includes('batch-result')) {
      const parsed = JSON.parse(result.stdout);
      parsed.batch.attributed = true;
      result.stdout = JSON.stringify(parsed);
    }
    return result;
  } });
  assert.equal(replay.completable, true);
  assert.ok(!attributed.calls.some((call) => call.binary === process.execPath));
});

test('HS2-9G0M3V recover clears the breaker only from the newest green main push run', () => {
  const none = fake();
  assert.equal(recover(options, { run: none.run }).status, 'red');
  assert.ok(!none.calls.some((call) => call.args.includes('batch-recover')));
  const { run, calls } = fake({
    green: [
      { conclusion: 'success', status: 'completed', headBranch: 'main', event: 'push', workflowName: 'CI', headSha: MAIN, databaseId: 21, createdAt: '2026-11-05T02:00:00Z' },
    ],
  });
  assert.equal(recover(options, { run }).status, 'recovered');
  const args = calls.find((call) => call.args.includes('batch-recover')).args;
  assert.deepEqual(args.slice(args.indexOf('--run')), ['--run', '21', '--main-commit', MAIN]);
});

test('HS2-9G0M3V helpers pick the exact batch run and build the attribution manifest', () => {
  assert.equal(batchRun([{ headSha: MAIN, workflowName: 'Platforms', event: 'push', databaseId: 1 }], MAIN), null);
  assert.throws(() => batchRun({}, MAIN), /non-array/);
  assert.equal(attributionManifest(batch(), 'p').tickets.length, 2);
  assert.throws(() => parseArgs(['ci', '--project', '.']), /--store/);
  assert.throws(() => parseArgs(['ci', '--project', '.', '--store', '.', '--integrator-id', 'i', '--integrator-session', 's']), /--batch/);
  assert.throws(() => parseArgs(['merge']), /Expected/);
  assert.throws(() => parseArgs(['ci', '--bogus', 'x']), /Unknown/);
  assert.equal(
    parseArgs(['recover', '--project', '.', '--store', '.', '--integrator-id', 'i', '--integrator-session', 's']).action,
    'recover',
  );
});

// End to end: the real hotsheet-cli process against an isolated home, a synthetic store, a synthetic
// checkout and a bare remote. Only `gh` and the attribution tool are faked.
const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const cliBinary = process.env.HS2_HOTSHEET_CLI ?? join(repoRoot, 'target/debug/hotsheet-cli');
test(
  'HS2-9G0M3V end to end: open, push once, record, red CI follow-ups gate completion, breaker recovery',
  { skip: existsSync(cliBinary) ? false : `no hotsheet-cli at ${cliBinary}; build it or set HS2_HOTSHEET_CLI` },
  (t) => {
    const base = realpathSync(mkdtempSync(join(tmpdir(), 'hs2-9g0m3v-e2e-')));
    t.after(() => rmSync(base, { recursive: true, force: true }));
    const home = join(base, 'home');
    const store = join(base, 'store');
    const project = join(base, 'project');
    const remote = join(base, 'remote.git');
    for (const dir of [home, store, project]) mkdirSync(dir, { recursive: true });
    // HS2-KKDZF4: an explicit environment, scrubbed of repository-locating git variables.
    const env = withoutGitRepositoryEnv({
      PATH: process.env.PATH,
      HOME: home,
      HOTSHEET_HOME: home,
      HOTSHEET_ACTOR_ROLE: 'system',
      GIT_AUTHOR_NAME: 'Hot Sheet Test',
      GIT_AUTHOR_EMAIL: 'test@invalid.example',
      GIT_COMMITTER_NAME: 'Hot Sheet Test',
      GIT_COMMITTER_EMAIL: 'test@invalid.example',
      GIT_CONFIG_GLOBAL: join(home, 'gitconfig'),
      GIT_CONFIG_NOSYSTEM: '1',
    });
    const run = (binary, args, cwd = base) =>
      execFileSync(binary, args, { cwd, env, encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] }).trim();
    const gitIn = (cwd, ...args) => run('git', args, cwd);
    const hs = (...args) => run(cliBinary, ['-C', store, ...args]);
    const coord = (...args) => JSON.parse(hs('coordination', '--project', project, ...args));
    hs('init', '--prefix', 'TS');
    gitIn(project, 'init', '-q', '-b', 'main');
    writeFileSync(join(project, 'base.txt'), 'base\n');
    gitIn(project, 'add', 'base.txt');
    gitIn(project, 'commit', '-qm', 'base');
    gitIn(project, 'init', '-q', '--bare', remote);
    gitIn(project, 'remote', 'add', 'origin', remote);
    gitIn(project, 'push', '-q', '-u', 'origin', 'main');
    const connection = createHash('sha256').update(realpathSync(store)).digest('hex').slice(0, 16);
    coord('strategy-allow', '--strategy', 'batch', '--reason', 'synthetic throughput', '--max-red-batches', '1');
    const id = ['--id', 'integrator', '--session', 'session'];
    coord('register', '--role', 'integrator', ...id, '--strategy', 'batch', '--all-participants-upgraded', '--lease-seconds', '3600');
    const handoffs = ['one', 'two'].map((name) => {
      const slug = /Created (\S+)/.exec(hs('new', '--title', `Synthetic ${name}`, '--category', 'feature', '--up-next'))[1];
      const native = /^id: (\S+)$/m.exec(hs('show', slug))[1];
      const worktree = join(base, `worktree-${name}`);
      const baseSha = gitIn(project, 'rev-parse', 'main');
      gitIn(project, 'worktree', 'add', '-qb', `work-${name}`, worktree, baseSha);
      writeFileSync(join(worktree, `${name}.txt`), `${name}\n`);
      gitIn(worktree, 'add', `${name}.txt`);
      gitIn(worktree, 'commit', '-qm', `${slug} synthetic`);
      const commit = gitIn(worktree, 'rev-parse', 'HEAD');
      const worker = ['--id', `worker-${name}`, '--session', 'worker-session'];
      coord('register', '--role', 'worker', ...worker);
      coord('assign', ...id, '--worker', `worker-${name}`, '--worker-session', 'worker-session', '--operation', `op-${name}`,
        '--connection', connection, '--native-id', native, '--slug', slug, '--base', baseSha);
      hs('claim', slug, '--worker', `worker-${name}`, '--eta', '10m');
      coord('worker', ...worker, '--assignment', `op-${name}`, '--phase', 'accepted');
      hs('release', slug, '--worker', `worker-${name}`);
      hs('edit', slug, '--note', `## Integration handoff\nVerified synthetic commit ${commit}`);
      coord('worker', ...worker, '--assignment', `op-${name}`, '--phase', 'handoff', '--commit', commit);
      return { slug, commit, operation: `op-${name}` };
    });
    coord('batch-open', ...id, '--batch', 'batch-1', '--assignment', 'op-one', '--assignment', 'op-two');
    for (const handoff of handoffs) gitIn(project, 'cherry-pick', handoff.commit);
    const main = gitIn(project, 'rev-parse', 'HEAD');
    const first = gitIn(project, 'rev-parse', 'HEAD~1');
    gitIn(project, 'push', '-q', 'origin', 'main');
    coord('batch-pushed', ...id, '--batch', 'batch-1', '--main-commit', main, '--shipping', `op-one=${first}`,
      '--shipping', `op-two=${main}`, '--smoke-check', 'cargo lint');
    const options = {
      cli: cliBinary,
      store,
      project,
      batch: 'batch-1',
      integratorId: 'integrator',
      integratorSession: 'session',
      attribution: join(base, 'attribution.mjs'),
    };
    const real = (binary, args, extra = {}) =>
      spawnSync(binary, args, { cwd: base, env, encoding: 'utf8', input: extra.input });
    // The real integration recorder checks the bare remote; run it from this process's env.
    const recorded = record(options, {
      run: real,
      recordIntegrationFn: (input) => {
        const result = spawnSync(process.execPath, [
          join(repoRoot, 'scripts/integration-record.mjs'),
          '--project', input.project, '--ticket', input.ticket, '--operation', input.operation,
          ...input.source.flatMap((sha) => ['--source', sha]),
          ...input.shipping.flatMap((sha) => ['--shipping', sha]),
        ], { cwd: base, env, encoding: 'utf8' });
        assert.equal(result.status, 0, result.stderr);
      },
    });
    assert.deepEqual(recorded.tickets.map((ticket) => ticket.note), ['added', 'added']);
    // Replaying the record step adds nothing.
    assert.deepEqual(
      record(options, { run: real, recordIntegrationFn: () => {} }).tickets.map((ticket) => ticket.note),
      ['existing', 'existing'],
    );
    for (const handoff of handoffs) {
      const shown = hs('show', handoff.slug);
      assert.ok(shown.includes(`CI gate commit: ${main}`));
      assert.equal(shown.split(`${MARKER} batch-1`).length, 2);
    }
    const bug = /Created (\S+)/.exec(hs('new', '--title', 'Synthetic CI failure', '--category', 'bug'))[1];
    const faked = (binary, args, extra) => {
      if (binary === 'gh')
        return {
          status: 0,
          stderr: '',
          stdout: JSON.stringify(
            args.includes('--status')
              ? [{ conclusion: 'success', status: 'completed', headBranch: 'main', event: 'push', workflowName: 'CI', headSha: main, databaseId: 31, createdAt: '2026-11-05T01:00:00Z' }]
              : [{ databaseId: 30, status: 'completed', conclusion: 'failure', headSha: main, event: 'push', workflowName: 'CI', createdAt: '2026-11-05T00:00:00Z' }],
          ),
        };
      if (binary === process.execPath)
        return { status: 0, stderr: '', stdout: JSON.stringify({ filed: [{ key: 'synthetic-key', kind: 'failure', slug: bug, links: [handoffs[0].slug] }] }) };
      return real(binary, args, extra);
    };
    assert.throws(() => coord('mark', ...id, '--assignment', 'op-one', '--phase', 'done'));
    const red = ci(options, { run: faked, scratch: base });
    assert.equal(red.status, 'red');
    assert.match(red.notify, /fell back to serial/);
    assert.deepEqual(red.follow_ups, [bug]);
    for (const handoff of handoffs) coord('mark', ...id, '--assignment', handoff.operation, '--phase', 'done');
    assert.equal(coord('status').batches.effective_strategy, 'serial');
    assert.equal(recover(options, { run: faked }).status, 'recovered');
    assert.equal(coord('status').batches.effective_strategy, 'batch');
  },
);
