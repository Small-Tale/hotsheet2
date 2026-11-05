// HS2-HAHX20: ranking, proposals, idempotent filing, and the CLI end to end against an isolated
// synthetic store. Every manifest, log and ticket here is synthetic; no customer content.
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  areaOf,
  attribute,
  changedFiles,
  fetchRun,
  fileAll,
  MARKER,
  nextestFiles,
  parseArgs,
  rankFailure,
  resolvePath,
  SIGNALS,
  storeConnection,
  TAG,
  validateManifest,
} from './ci-failure-attribution.mjs';
import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';
import { parseLog } from './ci-failure-parse.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const fixtures = join(here, 'fixtures/ci-failure-attribution');
const SHA = (digit) => digit.repeat(40);

const SOURCES = {
  'crates/demo-core/Cargo.toml': '[package]\nname = "demo-core"\n',
  'crates/demo-core/src/lib.rs': 'mod lexer;\nmod parser;\n',
  'crates/demo-core/src/parser.rs': 'pub fn parse() {}\n',
  'crates/demo-core/src/lexer.rs': 'pub fn lex() {}\n',
  'crates/demo-cli/Cargo.toml':
    '[package]\nname = "demo-cli"\n\n[dependencies]\ndemo-core = { path = "../demo-core" }\n',
  'crates/demo-cli/src/main.rs': 'fn main() {}\n',
  'crates/demo-cli/tests/cli.rs': '#[test]\nfn retries_lock() {}\n',
  'clients/web/src/label.ts': "export const label = 'a';\n",
  'clients/web/src/widget.ts': "import { label } from './label';\nexport const widget = () => label;\n",
  'clients/web/src/widget.test.ts': "import { widget } from './widget';\n",
  'clients/web/src/board-card.tsx': 'export const BoardCard = () => \'<div class="board-card">card</div>\';\n',
  'clients/web/tests/board.spec.ts': [
    "import { test } from '@playwright/test';",
    '',
    "test.describe('board', () => {",
    "  test('opens a card', async ({ page }) => {",
    "    await page.locator('.board-card').click();",
    '  });',
    '});',
    '',
  ].join('\n'),
  'scripts/tool.mjs': 'export const format = (x) => x;\n',
  'scripts/tool.test.mjs': "import { format } from './tool.mjs';\n",
  'migrator/src/index.ts': 'export {};\n',
  'docs/readme.md': '# Demo\n',
};

const PROJECT = 'demo-1a2b3c4d5e6f';
const CONNECTION = 'c0ffee0123456789';
function manifestFor(tickets) {
  return {
    version: 1,
    batch_id: 'batch-0001',
    project_id: PROJECT,
    tickets: tickets.map(([slug, changes], index) => ({
      slug,
      ticket: { project_id: PROJECT, connection_id: CONNECTION, native_id: `01NATIVE${index}` },
      source_commits: [SHA(String(index + 1))],
      shipping_commits: [SHA(String(index + 4))],
      changed_files: changes,
    })),
  };
}
const A = ['TS-AAAAAA', ['clients/web/src/label.ts', 'crates/demo-core/src/parser.rs']];
const B = ['TS-BBBBBB', ['crates/demo-core/src/lexer.rs', 'clients/web/src/board-card.tsx', 'scripts/tool.test.mjs']];
const C = ['TS-CCCCCC', ['docs/readme.md']];

const memoryRepo = (sources = SOURCES) => ({
  files: new Set(Object.keys(sources)),
  read: (path) => sources[path] ?? null,
  exercise: (changes) => ({
    files: changes.includes('clients/web/src/board-card.tsx') ? ['tests/board.spec.ts'] : [],
    providerTitles: [],
    providerWhole: false,
    demoTitles: [],
    reasons: [],
  }),
});
const tickets = (...entries) =>
  validateManifest(manifestFor(entries)).tickets.map((entry) => ({
    ...entry,
    qualified: entry.ticket,
    changes: entry.changed_files,
  }));
const failure = (overrides) => ({
  kind: 'test',
  job: 'job',
  step: 'step',
  locations: [],
  projects: [],
  file: null,
  excerpt: 'x',
  ...overrides,
});
const summary = (ranking) => ranking.suspects.map(({ slug, signal, confidence }) => `${slug} ${signal} ${confidence}`);

test('HS2-HAHX20 a single obvious culprit: the ticket that changed the failing test file ranks first', () => {
  const ranking = rankFailure(
    failure({ framework: 'nextest', test: 'demo-core parser::tests::parses_header', binary: 'demo-core' }),
    tickets(A, B, C),
    memoryRepo(),
  );
  assert.deepEqual(ranking.files, ['crates/demo-core/src/parser.rs']);
  assert.deepEqual(summary(ranking), ['TS-AAAAAA failure_file_changed 67', 'TS-BBBBBB same_crate 33']);
  assert.equal(ranking.suspects[0].level, 'high');
  assert.equal(ranking.suspects[1].level, 'medium');
  assert.match(ranking.reasoning, /^TS-AAAAAA changed crates\/demo-core\/src\/parser\.rs, which the failure names\.$/);
});

test('HS2-HAHX20 shared files: tickets tied on the strongest signal split confidence and keep batch order', () => {
  const both = ['TS-DDDDDD', ['clients/web/src/widget.test.ts']];
  const also = ['TS-EEEEEE', ['clients/web/src/widget.test.ts', 'docs/readme.md']];
  const ranking = rankFailure(
    failure({ framework: 'vitest', test: 'src/widget.test.ts > widget', file: 'src/widget.test.ts' }),
    tickets(both, also),
    memoryRepo(),
  );
  assert.deepEqual(summary(ranking), ['TS-DDDDDD failure_file_changed 50', 'TS-EEEEEE failure_file_changed 50']);
  assert.match(ranking.reasoning, /tied on failure_file_changed with TS-EEEEEE/);
});

test('HS2-HAHX20 no signal: every batch ticket is equally suspect at low confidence', () => {
  const ranking = rankFailure(
    failure({ framework: 'step', kind: 'step', job: 'Migrator tests', step: 'Run' }),
    tickets(A, B, C),
    memoryRepo(),
  );
  assert.deepEqual(summary(ranking), ['TS-AAAAAA no_signal 10', 'TS-BBBBBB no_signal 10', 'TS-CCCCCC no_signal 10']);
  assert.ok(ranking.suspects.every((suspect) => suspect.level === 'low'));
  assert.match(ranking.reasoning, /all 3 tickets are equally suspect/);
});

test('HS2-HAHX20 each weaker signal: imports, browser exercise, crate dependency and area ownership', () => {
  const repo = memoryRepo();
  const imports = rankFailure(
    failure({ framework: 'vitest', test: 'src/widget.test.ts > widget', file: 'src/widget.test.ts' }),
    tickets(A, C),
    repo,
  );
  assert.deepEqual(summary(imports), ['TS-AAAAAA imports_changed 60']);
  assert.match(imports.suspects[0].reason, /label\.ts, which clients\/web\/src\/widget\.test\.ts imports/);

  const exercised = rankFailure(
    failure({
      framework: 'playwright',
      test: 'tests/board.spec.ts › board › opens a card',
      file: 'tests/board.spec.ts',
    }),
    tickets(A, B),
    repo,
  );
  assert.deepEqual(summary(exercised), ['TS-BBBBBB exercised_by_spec 60', 'TS-AAAAAA area_owner 20']);

  const dependency = rankFailure(
    failure({ framework: 'nextest', test: 'demo-cli::cli retries_lock', binary: 'demo-cli::cli' }),
    tickets(A, C),
    repo,
  );
  assert.deepEqual(dependency.files, ['crates/demo-cli/tests/cli.rs']);
  assert.deepEqual(summary(dependency), ['TS-AAAAAA crate_dependency 30']);

  const area = rankFailure(
    failure({
      framework: 'step',
      kind: 'step',
      job: 'fmt · clippy · test',
      step: 'Web client lint, typecheck, unit tests, and production build',
    }),
    tickets(C, A),
    repo,
  );
  assert.deepEqual(summary(area), ['TS-AAAAAA area_owner 20']);
});

test('HS2-HAHX20 unknown files are reported and fall back to the job area instead of failing', () => {
  const ranking = rankFailure(
    failure({
      framework: 'vitest',
      test: 'src/missing.test.ts',
      file: 'src/missing.test.ts',
      job: 'web',
      step: 'unit tests',
    }),
    tickets(A, C),
    memoryRepo(),
  );
  assert.deepEqual(ranking.files, []);
  assert.deepEqual(ranking.unknownFiles, ['src/missing.test.ts']);
  assert.deepEqual(summary(ranking), ['TS-AAAAAA area_owner 20']);
});

test('HS2-HAHX20 path resolution: package-relative, runner-absolute, nextest module and integration paths', () => {
  const files = new Set(Object.keys(SOURCES));
  assert.equal(resolvePath('src/widget.ts', files), 'clients/web/src/widget.ts');
  assert.equal(
    resolvePath('/home/runner/work/repo/repo/crates/demo-core/src/lexer.rs:4:9', files),
    'crates/demo-core/src/lexer.rs',
  );
  assert.equal(resolvePath('src/nowhere.ts', files), null);
  assert.deepEqual(nextestFiles('demo-core parser::tests::parses_header', files), ['crates/demo-core/src/parser.rs']);
  assert.deepEqual(nextestFiles('demo-core top_level_test', files), ['crates/demo-core/src/lib.rs']);
  assert.deepEqual(nextestFiles('demo-cli::cli retries_lock', files), ['crates/demo-cli/tests/cli.rs']);
  assert.deepEqual(nextestFiles('demo-cli::bin/demo-cli args::parses', files), ['crates/demo-cli/src/main.rs']);
  assert.equal(areaOf('crates/demo-core/src/lib.rs'), 'crates/demo-core');
  assert.equal(areaOf('clients/web/src/a.ts'), 'clients/web');
  assert.equal(areaOf('README.md'), '.');
});

test('HS2-HAHX20 attribute: one proposal per distinct failure with the marker, suspects and identifiers only', () => {
  const parsed = parseLog(readFileSync(join(fixtures, 'attempt-2.log'), 'utf8'));
  const result = attribute({
    manifest: manifestFor([A, B, C]),
    parsed,
    run: { id: '9001', url: 'https://ci.invalid/runs/9001', attempt: 2, headSha: SHA('f') },
    repo: memoryRepo(),
  });
  const failures = result.proposals.filter((proposal) => proposal.kind === 'failure');
  const flaky = result.proposals.filter((proposal) => proposal.kind === 'flaky');
  assert.equal(failures.length, 6);
  assert.equal(flaky.length, 2);
  assert.equal(new Set(result.proposals.map((proposal) => proposal.key)).size, result.proposals.length);
  const top = Object.fromEntries(
    failures.map((proposal) => [
      proposal.identity,
      proposal.suspects[0] && `${proposal.suspects[0].slug} ${proposal.suspects[0].signal}`,
    ]),
  );
  assert.deepEqual(top, {
    'rustc:fmt · clippy · test / Clippy': 'TS-BBBBBB failure_file_changed',
    'node-test:scripts/tool.test.mjs > tool formats output': 'TS-BBBBBB failure_file_changed',
    'vitest:src/widget.test.ts > widget > renders the label': 'TS-AAAAAA imports_changed',
    'nextest:demo-core parser::tests::parses_header': 'TS-AAAAAA failure_file_changed',
    'playwright:tests/board.spec.ts › board › opens a card': 'TS-BBBBBB exercised_by_spec',
    'step:Migrator tests + coverage / Run migrator tests': 'TS-AAAAAA no_signal',
  });
  for (const proposal of failures) {
    assert.ok(proposal.details.endsWith(`${MARKER} ${proposal.key}`));
    assert.match(proposal.details, /batch-0001/);
    assert.match(proposal.details, /\[9001\]\(https:\/\/ci\.invalid\/runs\/9001\), attempt 2/);
    assert.match(
      proposal.details,
      /\| 1 \| TS-[A-Z]{6} \(`@demo-1a2b3c4d5e6f\/c0ffee0123456789:01NATIVE\d`\) \| \d+% (high|medium|low) \|/,
    );
    assert.ok(proposal.title.length <= 140);
    // Only the identifier excerpt reaches the ticket, never the surrounding log lines.
    assert.doesNotMatch(proposal.details, /AssertionError|Segmentation fault|toBeVisible/);
  }
  assert.ok(flaky.every((proposal) => proposal.suspects.length === 0 && proposal.tags.includes('flaky-test')));
});

test('HS2-HAHX20 the manifest is validated and changed files come from the shipping commits when absent', () => {
  assert.throws(() => validateManifest({ version: 2 }), /version must be 1/);
  assert.throws(() => validateManifest({ version: 1, batch_id: 'b', tickets: [] }), /at least one ticket/);
  assert.throws(
    () => validateManifest({ version: 1, batch_id: 'b', tickets: [{ slug: 'X', ticket: { connection_id: 'c' } }] }),
    /qualified ticket/,
  );
  assert.throws(
    () =>
      validateManifest({
        ...manifestFor([A]),
        tickets: [{ ...manifestFor([A]).tickets[0], shipping_commits: ['abc'] }],
      }),
    /full shipping_commits/,
  );
  const entry = { ...manifestFor([A]).tickets[0], changed_files: undefined };
  const calls = [];
  const files = changedFiles(entry, {
    project: '/repo',
    run: (binary, args) => {
      calls.push([binary, ...args]);
      return { status: 0, stdout: 'b.ts\na.ts\n' };
    },
  });
  assert.deepEqual(files, ['a.ts', 'b.ts']);
  assert.deepEqual(calls, [['git', 'diff-tree', '--no-commit-id', '--name-only', '-r', '-m', '--root', SHA('4')]]);
});

test('HS2-HAHX20 arguments: run or saved logs, a run id for idempotency, and a store unless dry-run', () => {
  assert.throws(() => parseArgs([]), /--manifest is required/);
  assert.throws(() => parseArgs(['--manifest', 'm']), /give --run/);
  assert.throws(() => parseArgs(['--manifest', 'm', '--log', 'l']), /--run-id is required/);
  assert.throws(() => parseArgs(['--manifest', 'm', '--run', '1', '--log', 'l']), /do not combine/);
  assert.throws(() => parseArgs(['--manifest', 'm', '--run', '1']), /--store is required/);
  const options = parseArgs([
    '--manifest',
    'm',
    '--log',
    'l',
    '--earlier-log',
    'e1',
    '--earlier-log',
    'e2',
    '--run-id',
    '7',
    '--dry-run',
  ]);
  assert.deepEqual(options['earlier-log'], ['e1', 'e2']);
  assert.equal(options['dry-run'], true);
});

test('HS2-HAHX20 fetchRun reads every attempt through gh and refuses a run still in progress', () => {
  const logs = {
    1: readFileSync(join(fixtures, 'attempt-1.log'), 'utf8'),
    2: readFileSync(join(fixtures, 'attempt-2.log'), 'utf8'),
  };
  const gh = (info) => (binary, args) => {
    assert.equal(binary, 'gh');
    if (args.includes('--json')) return { status: 0, stdout: JSON.stringify(info) };
    return { status: 0, stdout: logs[args[args.indexOf('--attempt') + 1]] };
  };
  const red = fetchRun('9001', {
    run: gh({ databaseId: 9001, attempt: 2, conclusion: 'failure', status: 'completed', headSha: SHA('f'), url: 'u' }),
  });
  assert.equal(red.run.attempt, 2);
  assert.ok(red.parsed.flaky.some((entry) => entry.test === 'src/other.test.ts > other > settles in time'));
  const green = fetchRun('9002', {
    run: gh({ databaseId: 9002, attempt: 2, conclusion: 'success', status: 'completed' }),
  });
  assert.deepEqual(green.parsed.failures, []);
  assert.ok(green.parsed.flaky.length > 0);
  assert.throws(
    () => fetchRun('9003', { run: gh({ attempt: 1, status: 'in_progress' }) }),
    /attribute it after CI completes/,
  );
});

/** An in-memory stand-in for `hotsheet-cli` with the same command shapes the filer uses. */
function memoryStore(slugs) {
  const tickets = new Map(
    slugs.map((slug, index) => [slug, { id: `01NATIVE${index}`, slug, tags: [], body: '', notes: [], followUps: [] }]),
  );
  const calls = [];
  let next = 0;
  const find = (ref) => [...tickets.values()].find((ticket) => ticket.slug === ref || ticket.id === ref);
  const links = (ticket) =>
    ticket.followUps.length
      ? `\nfollow_ups:\n${ticket.followUps.map((ref) => `- native_id: ${ref.split(':').at(-1)}`).join('\n')}`
      : '';
  const show = (ticket) =>
    `---\nid: ${ticket.id}\nslug: ${ticket.slug}${links(ticket)}\n---\n\n<!-- hotsheet:body:begin -->\n${ticket.body}\n<!-- hotsheet:body:end -->\n\n<!-- hotsheet:notes:begin -->\n${ticket.notes.join('\n')}\n<!-- hotsheet:notes:end -->\n`;
  const run = (binary, args, _cwd, input) => {
    assert.equal(binary, 'hotsheet-cli');
    const [, , action, ...rest] = args;
    calls.push(action);
    const value = (flag) => rest[rest.indexOf(flag) + 1];
    if (action === 'show') return { status: 0, stdout: show(find(rest[0])) };
    if (action === 'ls') {
      const text = value('--text');
      const rows = [...tickets.values()].filter(
        (ticket) =>
          ticket.tags.includes(value('--tag')) &&
          (ticket.body.includes(text) || ticket.notes.some((note) => note.includes(text))),
      );
      return { status: 0, stdout: rows.map((ticket) => ` ● ${ticket.slug}    not_started  title`).join('\n') };
    }
    if (action === 'new') {
      next += 1;
      const ticket = {
        id: `01FOLLOWUP${next}`,
        slug: `TS-F${String(next).padStart(5, '0')}`,
        tags: [],
        body: value('--details'),
        notes: [],
        followUps: [],
      };
      rest.forEach((arg, index) => {
        if (arg === '--tag') ticket.tags.push(rest[index + 1]);
      });
      tickets.set(ticket.slug, ticket);
      return { status: 0, stdout: `Created ${ticket.slug} (tickets/${ticket.id}.md)\n` };
    }
    if (action === 'link-follow-up') {
      const source = find(rest[0]);
      if (!source.followUps.includes(rest[1])) source.followUps.push(rest[1]);
      return { status: 0, stdout: '' };
    }
    if (action === 'edit') {
      find(rest[0]).notes.push(input);
      return { status: 0, stdout: `Updated ${rest[0]}\n` };
    }
    return { status: 2, stderr: `unexpected ${action}` };
  };
  return { tickets, calls, run };
}

test('HS2-HAHX20 filing is idempotent per run and failure: re-runs reuse tickets, links and notes', () => {
  const store = mkdtempSync(join(tmpdir(), 'hs2-hahx20-unit-'));
  try {
    const connection = storeConnection(store);
    const manifest = manifestFor([A, B, C]);
    for (const entry of manifest.tickets) entry.ticket.connection_id = connection;
    const parsed = parseLog(readFileSync(join(fixtures, 'attempt-2.log'), 'utf8'));
    const result = attribute({ manifest, parsed, run: { id: '9001' }, repo: memoryRepo() });
    const memory = memoryStore(['TS-AAAAAA', 'TS-BBBBBB', 'TS-CCCCCC']);

    const first = fileAll(result, { store, projectId: PROJECT, run: memory.run, actor: { role: 'ai', id: 'test' } });
    assert.ok(first.every((outcome) => outcome.action === 'created'));
    const created = [...memory.tickets.values()].filter((ticket) => ticket.tags.includes(TAG));
    assert.equal(created.length, result.proposals.length);
    const suspectA = memory.tickets.get('TS-AAAAAA');
    const failuresNamingA = result.proposals.filter((proposal) =>
      proposal.suspects.some((suspect) => suspect.slug === 'TS-AAAAAA'),
    );
    assert.equal(suspectA.followUps.length, failuresNamingA.length);
    assert.equal(suspectA.notes.length, failuresNamingA.length);
    assert.ok(suspectA.followUps.every((ref) => ref.startsWith(`@${PROJECT}/${connection}:01FOLLOWUP`)));
    assert.match(suspectA.notes[0], /^## CI failure suspect/);

    const second = fileAll(result, { store, projectId: PROJECT, run: memory.run });
    assert.ok(second.every((outcome) => outcome.action === 'existing' && outcome.notes.length === 0));
    assert.equal([...memory.tickets.values()].filter((ticket) => ticket.tags.includes(TAG)).length, created.length);
    assert.equal(suspectA.followUps.length, failuresNamingA.length);
    assert.equal(suspectA.notes.length, failuresNamingA.length);
    assert.equal(memory.calls.filter((call) => call === 'new').length, created.length);

    // A later run is a new failure identity; a flaky test reuses its ticket and records the occurrence.
    const later = attribute({ manifest, parsed, run: { id: '9002' }, repo: memoryRepo() });
    const third = fileAll(later, { store, projectId: PROJECT, run: memory.run });
    assert.ok(third.filter((outcome) => outcome.kind === 'failure').every((outcome) => outcome.action === 'created'));
    const flaky = third.filter((outcome) => outcome.kind === 'flaky');
    assert.ok(flaky.length && flaky.every((outcome) => outcome.action === 'existing' && outcome.notes.length === 1));
  } finally {
    rmSync(store, { recursive: true, force: true });
  }
});

test('HS2-HAHX20 a suspect on another connection is reported as an unsupported link, never guessed', () => {
  const store = mkdtempSync(join(tmpdir(), 'hs2-hahx20-unit-'));
  try {
    const manifest = manifestFor([A]);
    const result = attribute({
      manifest,
      parsed: {
        failures: [
          failure({ framework: 'nextest', test: 'demo-core parser::tests::parses_header', binary: 'demo-core' }),
        ],
        flaky: [],
      },
      run: { id: '1' },
      repo: memoryRepo(),
    });
    const memory = memoryStore(['TS-AAAAAA']);
    const [outcome] = fileAll(result, { store, projectId: PROJECT, run: memory.run });
    assert.equal(outcome.action, 'created');
    assert.deepEqual(outcome.links, []);
    assert.equal(outcome.unsupported[0].slug, 'TS-AAAAAA');
    assert.match(outcome.unsupported[0].reason, /own connection c0ffee0123456789/);
    assert.equal(memory.tickets.get('TS-AAAAAA').notes.length, 0);
  } finally {
    rmSync(store, { recursive: true, force: true });
  }
});

// End to end: the real CLI process, the real hotsheet-cli, an isolated home and synthetic store, a
// synthetic Git checkout, and the recorded synthetic log fixture.
const cliBinary = process.env.HS2_HOTSHEET_CLI ?? join(root, 'target/debug/hotsheet-cli');
test(
  'HS2-HAHX20 end to end: files follow-ups once, links suspects, and a re-run changes nothing',
  { skip: existsSync(cliBinary) ? false : `no hotsheet-cli at ${cliBinary}; build it or set HS2_HOTSHEET_CLI` },
  () => {
    const base = mkdtempSync(join(tmpdir(), 'hs2-hahx20-e2e-'));
    try {
      const home = join(base, 'home');
      const store = join(base, 'store');
      const project = join(base, 'project');
      for (const dir of [home, store, project]) mkdirSync(dir, { recursive: true });
      const env = withoutGitRepositoryEnv({
        PATH: process.env.PATH,
        HOME: home,
        HOTSHEET_HOME: home,
        HOTSHEET_ACTOR_ROLE: 'ai',
        HOTSHEET_ACTOR_ID: 'hs2-hahx20-test',
        GIT_AUTHOR_NAME: 'Hot Sheet Test',
        GIT_AUTHOR_EMAIL: 'test@invalid.example',
        GIT_COMMITTER_NAME: 'Hot Sheet Test',
        GIT_COMMITTER_EMAIL: 'test@invalid.example',
        GIT_CONFIG_GLOBAL: join(home, 'gitconfig'),
        GIT_CONFIG_NOSYSTEM: '1',
      });
      const hs = (...args) =>
        execFileSync(cliBinary, ['-C', store, ...args], {
          cwd: base,
          env,
          encoding: 'utf8',
          stdio: ['pipe', 'pipe', 'pipe'],
        });
      hs('init', '--prefix', 'TS');
      for (const [path, text] of Object.entries(SOURCES)) {
        mkdirSync(dirname(join(project, path)), { recursive: true });
        writeFileSync(join(project, path), text);
      }
      execFileSync('git', ['init', '-q', '-b', 'main'], { cwd: project, env });
      execFileSync('git', ['add', '.'], { cwd: project, env });
      const projectId = JSON.parse(hs('checkout', 'register', project, '--store', store)).id;
      const suspects = ['Synthetic suspect A', 'Synthetic suspect B', 'Synthetic suspect C'].map((title) => {
        const slug = /Created (\S+)/.exec(hs('new', '--title', title, '--category', 'feature'))[1];
        return { slug, id: /^id: (\S+)$/m.exec(hs('show', slug))[1] };
      });
      const connection = storeConnection(store);
      const manifest = manifestFor([A, B, C]);
      manifest.project_id = projectId;
      manifest.tickets.forEach((entry, index) => {
        entry.slug = suspects[index].slug;
        entry.ticket = { project_id: projectId, connection_id: connection, native_id: suspects[index].id };
      });
      writeFileSync(join(base, 'manifest.json'), JSON.stringify(manifest));
      const attribute = (...extra) => {
        const result = spawnSync(
          process.execPath,
          [
            join(here, 'ci-failure-attribution.mjs'),
            '--manifest',
            join(base, 'manifest.json'),
            '--project',
            project,
            '--store',
            store,
            '--cli',
            cliBinary,
            '--log',
            join(fixtures, 'e2e.log'),
            '--earlier-log',
            join(fixtures, 'attempt-1.log'),
            '--run-id',
            '9001',
            '--run-url',
            'https://ci.invalid/runs/9001',
            ...extra,
          ],
          { cwd: base, env, encoding: 'utf8' },
        );
        assert.equal(result.status, 0, result.stderr);
        return JSON.parse(result.stdout);
      };

      const dry = attribute('--dry-run');
      assert.equal(dry.dryRun, true);
      assert.equal(dry.filed, undefined);
      assert.doesNotMatch(hs('ls', '--tag', TAG), /TS-/);

      const first = attribute();
      const failures = first.proposals.filter((proposal) => proposal.kind === 'failure');
      assert.deepEqual(failures.map((proposal) => proposal.identity).sort(), [
        'nextest:demo-core parser::tests::parses_header',
        'playwright:tests/board.spec.ts › board › opens a card',
      ]);
      assert.equal(first.proposals.filter((proposal) => proposal.kind === 'flaky').length, 3);
      // The real exercise index ties the browser failure to the ticket that changed its component.
      const browser = failures.find((proposal) => proposal.identity.startsWith('playwright:'));
      assert.equal(browser.suspects[0].slug, suspects[1].slug);
      assert.equal(browser.suspects[0].signal, 'exercised_by_spec');
      assert.ok(first.filed.every((outcome) => outcome.action === 'created' && outcome.unsupported.length === 0));
      const listed = hs('ls', '--tag', TAG).match(/TS-[A-Z0-9]{6}/g);
      assert.equal(listed.length, first.proposals.length);

      const followUp = hs('show', first.filed[0].slug);
      assert.match(followUp, /^category: bug$/m);
      assert.match(followUp, /^priority: high$/m);
      assert.match(followUp, new RegExp(`${MARKER} ${first.filed[0].key}`));
      const suspectB = hs('show', suspects[1].slug);
      const namingB = first.filed.filter((outcome) => outcome.links.includes(suspects[1].slug));
      assert.equal(namingB.length, 2);
      for (const outcome of namingB) assert.match(suspectB, new RegExp(`${MARKER} ${outcome.key}`));
      assert.equal((suspectB.match(/^ {2}connection_id: /gm) ?? []).length, namingB.length);

      const second = attribute();
      assert.ok(second.filed.every((outcome) => outcome.action === 'existing' && outcome.notes.length === 0));
      assert.equal(hs('ls', '--tag', TAG).match(/TS-[A-Z0-9]{6}/g).length, listed.length);
      assert.equal(hs('show', suspects[1].slug), suspectB);
    } finally {
      rmSync(base, { recursive: true, force: true });
    }
  },
);
