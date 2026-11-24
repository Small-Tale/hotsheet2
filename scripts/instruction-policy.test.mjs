import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

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
  assert.equal((agents.match(/<!-- hotsheet-instructions-version: \d+ -->/g) ?? []).length, 0);
  assert.match(agents, /Push policy: `push-per-ticket`/);
  assert.match(agents, /\.agents\/skills\/hotsheet\/SKILL\.md/);
});

test('HS2-S2FZFM keeps all root guidance inside a measured instruction budget', () => {
  const agents = read('AGENTS.md');
  const selection = JSON.parse(read('guidance/rulesync/project-selection.json'));
  const bytes = Buffer.byteLength(agents, 'utf8');
  assert.ok(bytes < 28_000, `root AGENTS.md is ${bytes} bytes; target is below 28,000`);
  assert.ok(32_768 - bytes > 4_768, 'leave room for global and nested project instructions');
  assert.ok(!selection.rules.includes('hotsheet-workflow'), 'the installed skill owns ticket workflow detail');
  assert.match(agents, /docs\/29-affected-local-verification\.md/);
  assert.match(agents, /docs\/ui-design-philosophy\.md/);
  assert.match(agents, /docs\/34-development-strategies\.md/);
  assert.match(agents, /<!-- hotsheet:end section=visual-qa -->\s*$/);
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
