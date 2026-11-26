import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';

const read = (name) => readFileSync(new URL(`../${name}`, import.meta.url), 'utf8');

test('HS2-A47SCQ keeps one generated instruction owner and the local rule', () => {
  const agents = read('AGENTS.md');
  const local = read('.rulesync/rules/project-local.md')
    .replace(/^---\n[\s\S]*?\n---\n/, '')
    .trim();

  assert.equal(read('CLAUDE.md'), '@AGENTS.md\n');
  assert.ok(agents.includes(local), 'generated AGENTS.md must contain the reviewed local rule');

  const sections = [...agents.matchAll(/<!-- hotsheet:begin section=([^ ]+) v=(\d+) -->/g)];
  assert.equal(new Set(sections.map((match) => match[1])).size, sections.length);
  for (const [, name] of sections) {
    assert.equal(agents.split(`<!-- hotsheet:end section=${name} -->`).length - 1, 1);
  }
  // HS2-JQAWX6: the selected shared workflow rule carries exactly one instructions-version marker.
  assert.equal((agents.match(/<!-- hotsheet-instructions-version: \d+ -->/g) ?? []).length, 1);
  assert.match(agents, /Push policy: `push-per-ticket`/);
  assert.match(agents, /\.agents\/skills\/hotsheet\/SKILL\.md/);
});

// HS2-JQAWX6 restored the workflow and UI judgment rules HS2-S2FZFM had dropped. Codex reads them only
// when the project raises `project_doc_max_bytes` above this ceiling; HS2-S2FZFM's 28,000-byte target
// is retired. Raise the ceiling only with a measured, reviewed reason.
const ROOT_INSTRUCTION_CEILING = 40_000;

test('HS2-JQAWX6 selects the essential shared rules inside a measured instruction budget', () => {
  const agents = read('AGENTS.md');
  const selection = JSON.parse(read('guidance/rulesync/project-selection.json'));
  const bytes = Buffer.byteLength(agents, 'utf8');
  assert.ok(
    bytes < ROOT_INSTRUCTION_CEILING,
    `root AGENTS.md is ${bytes} bytes; ceiling is ${ROOT_INSTRUCTION_CEILING}`,
  );
  for (const rule of [
    'hotsheet-workflow',
    'git-commit-and-push',
    'ui-composition',
    'ui-evaluation',
    'kerf-ui-usage',
    'kerf-ui-interaction-defaults',
  ])
    assert.ok(selection.rules.includes(rule), `${rule} must stay selected`);
  assert.match(agents, /Create a ticket before features, bugs, refactors/);
  assert.match(agents, /docs\/29-affected-local-verification\.md/);
  assert.match(agents, /docs\/ui-design-philosophy\.md/);
  assert.match(agents, /docs\/34-development-strategies\.md/);
  assert.match(agents, /Push policy: `push-per-ticket`/);
});

test('HS2-DKSN1H installs the selected playbook skills in both tool mirrors', () => {
  const selection = JSON.parse(read('guidance/rulesync/project-selection.json'));
  assert.deepEqual(selection.skills, ['ui-ux-review', 'security-review', 'feature-health']);
  for (const skill of selection.skills) {
    const claude = read(`.claude/skills/${skill}/SKILL.md`);
    assert.equal(read(`.agents/skills/${skill}/SKILL.md`), claude, `${skill} mirrors must match`);
    assert.match(claude, new RegExp(`^---\\nname: ${skill}\\n`));
  }
});

test('HS2-XJRJ9Z keeps the reviewed local guidance once in generated instructions', () => {
  const agents = read('AGENTS.md');
  const local = read('.rulesync/rules/project-local.md')
    .replace(/^---\n[\s\S]*?\n---\n/, '')
    .trim();
  assert.equal(agents.split(local).length - 1, 1);
});

test('HS2-EJ8JTG pins authoritative tier tags and analyzed T2 testing', () => {
  const local = read('.rulesync/rules/project-local.md');
  const agents = read('AGENTS.md');
  const worker = read('plugins/shared/role-skills/hotsheet-worker.md');
  const integrator = read('plugins/shared/role-skills/hotsheet-integrator.md');
  for (const source of [local, agents]) {
    assert.match(source, /Set exactly one\s+`tier:T0`, `tier:T1`, or `tier:T2` ticket tag/);
    assert.match(source, /T2 has no time cap:/);
    // HS2-6PZCD9: the configured development strategy makes the gate require a reviewed test plan.
    assert.match(source, /`scripts\/check\.sh` requires\s+`--test-plan <reviewed\.json>`/);
    // HS2-ZGS7JP: CI First is the one strategy whose best-effort run needs neither.
    assert.match(source, /T2 also\s+requires `--full` or an analyzed `--t2-stage` selection/);
    assert.match(source, /CI First's\s+budgeted run needs neither flag/);
    assert.match(source, /name deferred\s+checks and the green covering main CI run in the completion note/);
  }
  assert.match(worker, /set the ticket's tier\s+tag when declaring risk/);
  assert.match(worker, /normal gate without profile flags/);
  assert.match(integrator, /verify\s+the worker's analyzed selection for uncapped high-risk work/);
  assert.match(integrator, /Record deferred\s+checks and the covering main CI run in the completion note/);
});

test('HS2-ZGS7JP the CI First deviation keeps the push protections and attributes red CI', () => {
  for (const source of [read('.rulesync/rules/project-local.md'), read('AGENTS.md')]) {
    assert.match(
      source,
      /CI First \(HS2-ZGS7JP\) applies only when the resolver reports `ci-first` and\s+`push_without_receipt: true`/,
    );
    assert.match(
      source,
      /without a full-gate receipt but retains a clean\s+checkout, main-writer reservation, fast-forward, and remote confirmation/,
    );
    assert.match(source, /Attribute red CI to tickets since the last\s+green run/);
    assert.match(source, /Other strategies still require the receipt/);
  }
});

test('HS2-39D5YS requires an explicit integration record before worker cleanup', () => {
  const integrator = read('plugins/shared/role-skills/hotsheet-integrator.md');
  assert.match(integrator, /scripts\/integration-record\.mjs/);
  assert.match(integrator, /--ticket\s+@<project-id>\/<connection-id>:<native-id>/);
  assert.match(integrator, /Repeat `--source`\s+and `--shipping`/);
  assert.match(integrator, /before proposing\s+completion or deleting the worker branch/);
  assert.match(integrator, /integration record is durable/);
});

test('HS2-HR3ZX4 finalization invokes guarded worktree cleanup', () => {
  for (const file of ['plugins/codex/SKILL.md', 'plugins/claude/SKILL.md']) {
    const skill = read(file);
    assert.match(skill, /After green covering main CI, complete the ticket and release its claim/);
    assert.match(skill, /scripts\/verified-worktree-cleanup\.mjs/);
  }
  const integrator = read('plugins/shared/role-skills/hotsheet-integrator.md');
  assert.match(integrator, /Mark the mailbox `done`, then release the claim\. Invoke/);
  assert.match(integrator, /scripts\/verified-worktree-cleanup\.mjs/);
  assert.match(integrator, /--operation <assignment-id>/);
});

test('HS2-MFD44D runs notification process transitions on native Windows', () => {
  // HS2-WSE3FJ: the daily platform workflow's Windows targets run the whole nextest workspace,
  // which includes the hotsheet-notify hs2_mfd44d process regressions.
  const workflow = read('.github/workflows/platforms.yml');
  const start = workflow.indexOf('\n  core:');
  const end = workflow.indexOf('\n  tauri:', start);
  assert.ok(start >= 0 && end > start, 'the core platform job must exist');
  const core = workflow.slice(start, end);
  assert.match(core, /runs-on: \$\{\{ matrix\.runner \}\}/);
  assert.ok(/run: \$\{\{ matrix\.test_wrapper \}\} cargo nextest run --all-features --no-fail-fast\n/.test(core));
  const targets = read('scripts/platform-matrix.mjs');
  assert.match(targets, /runner: 'windows-latest'/);
  assert.match(targets, /runner: 'windows-11-arm'/);
});

test('HS2-F9062F makes effective strategy resolution and precedence executable guidance', () => {
  const local = read('.rulesync/rules/project-local.md');
  const agents = read('AGENTS.md');
  for (const text of [local, agents]) {
    assert.match(text, /development-strategies/);
    assert.match(text, /development-strategy-resolve/);
    assert.match(text, /User-selected strategy overrides ticket,\s+session, then project strategy/);
    assert.match(text, /never silently waive\s+ticket\/integration verification or green-main promotion/);
    assert.match(text, /An unset project retains the default\s+integration mode and risk-selected checks/);
    assert.match(text, /safe\s+boundary/);
    assert.match(text, /highest-priority `--ticket`\s+override/);
  }
  const roles = ['worker', 'dispatcher', 'integrator'];
  for (const role of roles) {
    const source = read(`plugins/shared/role-skills/hotsheet-${role}.md`);
    const codex = read(`.agents/skills/hotsheet-${role}/SKILL.md`);
    const claude = read(`.claude/skills/hotsheet-${role}/SKILL.md`);
    assert.equal(codex, source);
    assert.equal(claude.replace('allowed-tools: Read, Grep, Glob, Edit, Write, Bash\n', ''), source);
    assert.match(source, /development-strategy-resolve/);
    assert.match(source, /development-strategies/);
    assert.match(source, /--session\s+<strategy-name>/);
    assert.match(source, /--ticket\s+<strategy-name>/);
    assert.match(source, /unconfigured-project/);
    assert.match(source, /mandatory gate/);
    assert.match(source, /safe (ticket |integration )?boundary/);
    assert.match(source, /If (the selected CLI|this CLI|the CLI) lacks the\s+resolver/);
  }
  for (const skill of [
    '.agents/skills/hotsheet/SKILL.md',
    '.claude/skills/hotsheet/SKILL.md',
    'plugins/codex/SKILL.md',
    'plugins/claude/SKILL.md',
  ]) {
    const text = read(skill);
    assert.match(text, /development-strategy-resolve/);
    assert.match(text, /ticket, session, then project/);
    assert.match(text, /unconfigured-project/);
  }
});

test('HS2-Q390N2 pins the explicit standalone fast-push deviation', () => {
  for (const source of [read('.rulesync/rules/project-local.md'), read('AGENTS.md')]) {
    assert.match(source, /scripts\/push\.sh --standalone\s+--skip-tests/);
    assert.match(
      source,
      /still requires a clean checkout, main-writer reservation,\s+fast-forward, remote SHA confirmation/,
    );
    assert.match(source, /override is unavailable to integrators/);
  }
});

test('HS2-M0ANR4 role skills keep completed side-branch work on merge-pending/<SLUG>', () => {
  const worker = read('plugins/shared/role-skills/hotsheet-worker.md');
  const integrator = read('plugins/shared/role-skills/hotsheet-integrator.md');
  const dispatcher = read('plugins/shared/role-skills/hotsheet-dispatcher.md');
  assert.match(
    worker,
    /secrets, customer data and machine-local paths, then push the worker\s+branch to `merge-pending\/<SLUG>`/,
  );
  assert.match(worker, /Never rebase or force-push a pushed `merge-pending\/`\s+branch/);
  assert.match(integrator, /fast-forward, merge or\s+cherry-pick/);
  assert.match(integrator, /never rebase\s+or force-push it/);
  assert.match(integrator, /merge-pending-prune --apply/);
  assert.match(dispatcher, /Name the worker's remote branch in\s+the cue: `merge-pending\/<SLUG>`/);
  for (const skill of ['plugins/claude/SKILL.md', 'plugins/codex/SKILL.md']) {
    const text = read(skill);
    assert.match(text, /completes only from a pushed `merge-pending\/<SLUG>` branch/, skill);
    assert.match(text, /merge-pending-prune --apply/, skill);
  }
});

// HS2-FXF07C: "Never commit ... machine-local paths" and "a lint disable carries a reason and a
// ticket slug on the same line, and a policy test caps disables per file" (AGENTS.md Conventions as
// Code, Git Commit and Push). Both scan every tracked file from the repository root.
const repositoryRoot = fileURLToPath(new URL('..', import.meta.url));
const trackedFiles = (...patterns) =>
  execFileSync('git', ['ls-files', '-z', '--', ...patterns], {
    cwd: repositoryRoot,
    env: withoutGitRepositoryEnv(process.env),
    encoding: 'utf8',
    maxBuffer: 1 << 28,
  })
    .split('\0')
    .filter(Boolean);
const trackedText = (file) => {
  let bytes;
  try {
    bytes = readFileSync(join(repositoryRoot, file));
  } catch {
    return null; // A tracked file deleted in the working tree has no content to check.
  }
  return bytes.includes(0) ? null : bytes.toString('utf8');
};

// Home-directory segments that name no real person or machine. Every entry needs a reason; a name
// that is no longer used fails below, so the list shrinks with the fixtures.
const SYNTHETIC_HOME_NAMES = new Map([
  ['synthetic', 'the preferred synthetic home for new fixtures'],
  ['someone', 'redaction and journal fixtures'],
  ['me', 'short synthetic home in terminal, export and respelling fixtures'],
  ['x', 'Windows path-shape fixtures'],
  ['u', 'synthetic Claude install path'],
  ['user', 'Git path conversion fixtures'],
  ['dev', 'stable-dev home fixtures'],
  ['example', 'Vite /@fs URL fixture'],
  ['alice', 'feedback redaction fixture'],
  ['private', 'redaction fixtures that must never reach output'],
  ['runner', 'the GitHub-hosted Linux runner home in CI log fixtures'],
  ['runneradmin', 'the GitHub-hosted Windows runner home in path fixtures'],
  ['RUNNER~1', 'the GitHub-hosted Windows runner home as an 8.3 short name'],
  ['LONGNA~12.TXT', '8.3 short-name respelling fixture'],
  ['...', 'elided example path in prose and messages'],
  ['server-bootstrap.hs2', 'synthetic store path under /home'],
  ['.claude.json', 'synthetic Claude user config path under /home'],
]);
// Disk names under /Volumes that name no real machine's volumes: host-disk fixtures and the macOS
// system mounts the disk sampler filters.
const SYNTHETIC_VOLUME_NAMES = new Map([
  ['Synthetic', 'registered-project fixtures'],
  ['Backup', 'host-disk and registered-project fixtures'],
  ['Scratch', 'host-disk fixtures'],
  ['Share', 'host-disk fixtures'],
  ['Archive', 'host-disk fixtures'],
  ['Camera', 'host-disk fixtures'],
  ['Installer', 'disk sampler filter fixture for mounted installer images'],
  ['com.apple.TimeMachine.localsnapshots', 'disk sampler filter for the macOS system mount'],
]);
// `/Users/<name>`, `/home/<name>`, `C:\Users\<name>` (any separator spelling), `/Volumes/<disk>`,
// and the `-Users-<name>-` spelling AI tools use for per-project scratch directories.
const MACHINE_PATH =
  /(?:(?:\/Users\/|(?<![A-Za-z0-9_.-])\/home\/|[A-Za-z]:(?:\\\\|\\|\/)Users(?:\\\\|\\|\/))([A-Za-z0-9._~-]+)|-Users-([A-Za-z0-9._~]+)-|\/Volumes\/([A-Za-z0-9._~-]+))/g;
const machinePathName = (raw) => (raw === '...' ? raw : raw.replace(/[.]+$/, ''));
// The home (or encoded home) or volume name a match names.
const machinePath = ([, home, encoded, volume]) => ({
  kind: volume === undefined ? 'home' : 'volume',
  name: machinePathName(home ?? encoded ?? volume),
});

test('HS2-FXF07C tracked files carry no machine-local home or volume paths', () => {
  const violations = [];
  const used = new Set();
  for (const file of trackedFiles()) {
    const text = trackedText(file);
    if (text === null) continue;
    for (const match of text.matchAll(MACHINE_PATH)) {
      const { kind, name } = machinePath(match);
      if ((kind === 'home' ? SYNTHETIC_HOME_NAMES : SYNTHETIC_VOLUME_NAMES).has(name)) {
        used.add(`${kind}:${name}`);
        continue;
      }
      const line = text.slice(0, match.index).split('\n').length;
      violations.push(`${file}:${line}: ${match[0]}`);
    }
  }
  assert.deepEqual(
    violations,
    [],
    'use a synthetic path (/Users/synthetic/...) or a test output path, or add a reasoned synthetic name',
  );
  for (const [names, kind] of [
    [SYNTHETIC_HOME_NAMES, 'home'],
    [SYNTHETIC_VOLUME_NAMES, 'volume'],
  ]) {
    for (const [name, reason] of names) {
      assert.ok(reason.length > 0, `${kind} ${name} needs a reason`);
      assert.ok(used.has(`${kind}:${name}`), `remove the unused synthetic ${kind} name ${name}`);
    }
  }
});

test('HS2-FXF07C the machine-path scan rejects real-looking paths and accepts synthetic ones', () => {
  // The probes are assembled at run time so this file never holds the paths it rejects.
  const who = ['j', 'doe'].join('');
  const disk = ['Work', 'Disk'].join('');
  const paths = (text) => [...text.matchAll(MACHINE_PATH)].map(machinePath);
  const home = { kind: 'home', name: who };
  assert.deepEqual(
    paths(`/Users/${who}/src C:\\Users\\${who}\\x C:\\\\Users\\\\${who} D:/Users/${who} /home/${who}/x`),
    [home, home, home, home, home],
  );
  assert.deepEqual(paths(`scratch/-Users-${who}-Projects-app/x`), [home]);
  assert.deepEqual(paths(`file:///Volumes/${disk}/Projects and /Volumes/Scratch.`), [
    { kind: 'volume', name: disk },
    { kind: 'volume', name: 'Scratch' },
  ]);
  assert.deepEqual(paths('per-user/home/project'), []);
  assert.deepEqual(paths(`127.0.0.1:4175/@fs/Users/${who}/file.ts`), [home]);
  assert.equal(SYNTHETIC_HOME_NAMES.has(who), false);
});

// Rust lint suppressions: `#[allow(..)]`, `#[expect(..)]`, their inner `#![..]` forms and
// `#[cfg_attr(.., allow(..))]`. The reason and slug live in `reason = "..."` or a trailing
// same-line comment. The cap is the current per-file maximum; it only decreases.
const RUST_SUPPRESSIONS_PER_FILE = 10;
const SLUG = /\bHS2-[0-9A-Z]{6}\b/;

function rustSuppressions(text) {
  const found = [];
  for (const match of text.matchAll(/#!?\[/g)) {
    let depth = 0;
    let end = match.index + match[0].length - 1;
    for (; end < text.length; end += 1) {
      if (text[end] === '[') depth += 1;
      else if (text[end] === ']' && --depth === 0) break;
    }
    const body = text.slice(match.index + match[0].length, end);
    if (!/^\s*(?:allow|expect)\s*\(/.test(body) && !/^\s*cfg_attr\s*\([\s\S]*\b(?:allow|expect)\s*\(/.test(body)) {
      continue;
    }
    const lineEnd = text.indexOf('\n', end);
    const trailing = text.slice(end + 1, lineEnd === -1 ? text.length : lineEnd);
    const reason = /\breason\s*=\s*"([^"]*)"/.exec(body)?.[1] ?? /^\s*\/\/(.*)$/.exec(trailing)?.[1] ?? '';
    found.push({
      line: text.slice(0, match.index).split('\n').length,
      reasoned: SLUG.test(reason) && reason.replace(SLUG, '').replace(/[\s:()-]/g, '').length > 0,
    });
  }
  return found;
}

test('HS2-FXF07C Rust lint suppressions carry a reason with a ticket slug and stay capped per file', () => {
  const unreasoned = [];
  const overCap = [];
  for (const file of trackedFiles('*.rs')) {
    const text = trackedText(file);
    if (text === null) continue;
    const suppressions = rustSuppressions(text);
    for (const { line, reasoned } of suppressions) {
      if (!reasoned) unreasoned.push(`${file}:${line}`);
    }
    if (suppressions.length > RUST_SUPPRESSIONS_PER_FILE) overCap.push(`${file}: ${suppressions.length}`);
  }
  assert.deepEqual(unreasoned, [], 'add `reason = "HS2-XXXXXX: why"` or a same-line `// HS2-XXXXXX: why`');
  assert.deepEqual(overCap, [], `at most ${RUST_SUPPRESSIONS_PER_FILE} lint suppressions per Rust file`);
});

test('HS2-FXF07C the Rust suppression scan reads every accepted reason form', () => {
  const reasoned = (source) => rustSuppressions(source).map((entry) => entry.reasoned);
  assert.deepEqual(reasoned('#[allow(dead_code)]\nfn a() {}'), [false]);
  assert.deepEqual(reasoned('#[allow(dead_code)] // HS2-ABC123\nfn a() {}'), [false]);
  assert.deepEqual(reasoned('#[allow(dead_code)] // HS2-ABC123: fixture subset\nfn a() {}'), [true]);
  assert.deepEqual(reasoned('#![allow(\n    dead_code,\n    reason = "HS2-ABC123: subset"\n)]'), [true]);
  assert.deepEqual(reasoned('#[expect(unused, reason = "no slug here")]'), [false]);
  assert.deepEqual(reasoned('#[cfg_attr(unix, allow(dead_code))]\n// HS2-ABC123: next line\n'), [false]);
  assert.deepEqual(reasoned('#[cfg_attr(unix, allow(dead_code))] // HS2-ABC123: Unix only'), [true]);
  assert.deepEqual(reasoned('#[derive(Debug)]\n#[serde(rename_all = "camelCase")]'), []);
});
