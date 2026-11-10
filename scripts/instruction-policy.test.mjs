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
  assert.equal((agents.match(/<!-- hotsheet-instructions-version: \d+ -->/g) ?? []).length, 1);
  assert.match(agents, /Push policy: `push-per-ticket`/);
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
    assert.match(source, /Run `scripts\/check\.sh` without profile flags normally/);
    assert.match(source, /completion note names deferred-to-CI checks and the successful\s+covering main CI run/);
  }
  assert.match(worker, /set the ticket's tier\s+tag when declaring risk/);
  assert.match(worker, /normal gate without profile flags/);
  assert.match(integrator, /verify\s+the worker's analyzed selection for uncapped high-risk work/);
  assert.match(integrator, /Record deferred\s+checks and the covering main CI run in the completion note/);
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
  assert.ok(/run: \$\{\{ matrix\.test_wrapper \}\} cargo nextest run --all-features\n/.test(core));
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
    assert.match(text, /user instruction takes precedence over a ticket strategy, then a\s+session strategy/);
    assert.match(text, /mandatory affected gates/);
    assert.match(text, /unset\s+project setting preserves the\s+existing integration default and risk-selected/);
    assert.match(text, /next safe boundary/);
    assert.match(text, /user-selected\s+strategy as the highest-priority `--ticket` override/);
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
  for (const skill of ['.agents/skills/hotsheet/SKILL.md', '.claude/skills/hotsheet/SKILL.md', 'plugins/codex/SKILL.md', 'plugins/claude/SKILL.md']) {
    const text = read(skill);
    assert.match(text, /development-strategy-resolve/);
    assert.match(text, /ticket, session, then project/);
    assert.match(text, /unconfigured-project/);
  }
});
