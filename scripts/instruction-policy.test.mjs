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
