// HS2-VA97HW: the committed AGENTS.md must equal the pinned Rulesync regeneration. These cases run the
// real pinned generator over synthetic rules, offline; the repository check itself runs in CI with
// `node scripts/check-rulesync-generation.mjs --fetch`.
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';
import { firstDifference, generate, materialize, readSelection } from './check-rulesync-generation.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const json = (path) => JSON.parse(readFileSync(join(root, path), 'utf8'));
const rule = (description, body) =>
  `---\nroot: true\ntargets: ["*"]\ndescription: ${JSON.stringify(description)}\n---\n\n${body}\n`;
const synthetic = {
  'alpha-rule': rule('Alpha synthetic rule', '## Alpha\n\nAlpha body line.'),
  'beta-rule': rule('Beta synthetic rule', '## Beta\n\nBeta body line.'),
};
const local = '---\nroot: true\ntargets: ["*"]\n---\n\n# Synthetic local rule\n\nPush policy: `push-per-ticket`.\n';

function regenerate(rules, localRule = local) {
  const project = mkdtempSync(join(tmpdir(), 'hotsheet-rulesync-test-'));
  try {
    materialize(project, { rules, readRule: (name) => synthetic[name], localRule });
    return generate(project);
  } finally {
    rmSync(project, { recursive: true, force: true });
  }
}

test('HS2-VA97HW the real pinned generator is deterministic and includes every selected rule and the local rule', () => {
  const first = regenerate(['alpha-rule', 'beta-rule']);
  assert.equal(regenerate(['alpha-rule', 'beta-rule']), first);
  for (const text of ['Synthetic local rule', 'Alpha body line.', 'Beta body line.'])
    assert.ok(first.includes(text), text);
  assert.equal(firstDifference(first, first), undefined);
});

test('HS2-VA97HW unrelated hand edits, a dropped rule, and a changed local rule all fail the comparison', () => {
  const committed = regenerate(['alpha-rule', 'beta-rule']);
  const handEdited = committed.replace('Alpha body line.', 'Alpha body line, edited by hand.');
  assert.equal(firstDifference(handEdited, committed)?.committed, 'Alpha body line, edited by hand.');
  assert.ok(firstDifference(committed, regenerate(['alpha-rule'])), 'a deselected rule must change the output');
  assert.ok(
    firstDifference(committed, regenerate(['alpha-rule', 'beta-rule'], local.replace('per-ticket', 'per-batch'))),
    'a changed local rule must change the output',
  );
  assert.ok(firstDifference(`${committed}\n`, committed), 'a trailing-byte change must fail');
});

test('HS2-VA97HW the committed selection pins the same ai-playbook commit as the bundled guidance', () => {
  const selection = readSelection(readFileSync(join(root, 'guidance/rulesync/project-selection.json'), 'utf8'));
  assert.equal(selection.repository, json('guidance/ai-playbook.lock.json').repository);
  assert.equal(selection.commit, json('guidance/ai-playbook.lock.json').commit);
  assert.equal(selection.commit, json('guidance/rulesync/rulesync.lock').sources['small-tale/ai-playbook'].resolvedRef);
  // Every bundled section and every selected rule is represented in the generated file's markers.
  const agents = readFileSync(join(root, 'AGENTS.md'), 'utf8');
  for (const name of selection.rules.filter((item) => item.startsWith('hotsheet-') && item !== 'hotsheet-workflow'))
    assert.ok(agents.includes(`<!-- hotsheet:begin section=${name.slice('hotsheet-'.length)} `), name);
  assert.throws(() => readSelection('{"commit":"main","rules":["a"]}'), /full source commit/);
  assert.throws(() => readSelection(`{"commit":"${'a'.repeat(40)}","rules":["a","a"]}`), /repeats/);
});

test('HS2-VA97HW isolates fixture and generator children from a hook repository', () => {
  const poison = mkdtempSync(join(tmpdir(), 'hotsheet-va97hw-hook-'));
  const clean = withoutGitRepositoryEnv(process.env);
  const git = (...args) => execFileSync('git', args, { cwd: poison, env: clean, encoding: 'utf8' });
  try {
    git('init', '--quiet', '-b', 'main');
    git('config', 'user.name', 'Isolation Guard');
    git('config', 'user.email', 'guard@example.invalid');
    writeFileSync(join(poison, 'README.md'), 'Synthetic hook repository\n');
    git('add', 'README.md');
    git('commit', '--quiet', '-m', 'Seed isolated hook fixture');
    const snapshot = () => ({
      index: readFileSync(join(poison, '.git/index')).toString('hex'),
      config: readFileSync(join(poison, '.git/config'), 'utf8'),
      head: git('rev-parse', 'HEAD'),
    });
    const before = snapshot();
    const childEnv = {
      ...clean,
      GIT_DIR: join(poison, '.git'),
      GIT_WORK_TREE: poison,
      GIT_INDEX_FILE: join(poison, '.git/index'),
      GIT_COMMON_DIR: join(poison, '.git'),
      GIT_OBJECT_DIRECTORY: join(poison, '.git/objects'),
    };
    // This is an independent CLI test runner, not an IPC child of this test (HS2-VA97HW).
    Reflect.deleteProperty(childEnv, 'NODE_TEST_CONTEXT');
    const child = spawnSync(
      process.execPath,
      [
        '--test',
        '--test-name-pattern=^HS2-VA97HW (drafts offline|drafts with|the real pinned)',
        'scripts/gitgist-tooling.test.mjs',
        'scripts/check-rulesync-generation.test.mjs',
      ],
      {
        cwd: root,
        env: childEnv,
        encoding: 'utf8',
      },
    );
    assert.deepEqual(snapshot(), before, 'the hook repository index, config and HEAD must remain unchanged');
    assert.equal(child.status, 0, `${child.stdout}\n${child.stderr}`);
    assert.match(child.stdout, /# pass 3\b/);
  } finally {
    rmSync(poison, { recursive: true, force: true });
  }
});
