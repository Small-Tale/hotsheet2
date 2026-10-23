import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = (name) => readFileSync(new URL(`../${name}`, import.meta.url), 'utf8');

test('HS2-A47SCQ keeps one generated instruction owner and the local rule', () => {
  const agents = read('AGENTS.md');
  const local = read('.rulesync/rules/project-local.md').replace(/^---\n[\s\S]*?\n---\n/, '').trim();

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
