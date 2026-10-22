import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {
  checkBundled,
  destinations,
  main,
  readSource,
  repository,
  ruleBody,
  validateLock,
} from './sync-ai-playbook.mjs';

test('rule packaging is removed without changing the managed body', () => {
  assert.equal(ruleBody('---\nroot: true\n---\n\nbody\n'), 'body\n');
  assert.throws(() => ruleBody('body'), /frontmatter/);
});

test('snapshot rejects unknown, duplicate, and incomplete rule identities', () => {
  const root = path.resolve(import.meta.dirname, '..');
  const lock = JSON.parse(fs.readFileSync(path.join(root, 'guidance/ai-playbook.lock.json')));
  checkBundled(root, lock);
  assert.throws(() => validateLock({ ...lock, commit: 'main' }), /provenance/);
  assert.throws(() => validateLock({ ...lock, rules: lock.rules.slice(1) }), /Incomplete/);
  assert.throws(() => validateLock({ ...lock, rules: lock.rules.map(() => lock.rules[0]) }), /Invalid snapshot/);
});

test('real git source produces an offline bundle, detects drift, and restores it', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'hotsheet-playbook-test-'));
  try {
    const source = path.join(root, 'source'),
      bundle = path.join(root, 'bundle');
    fs.mkdirSync(path.join(source, 'rules'), { recursive: true });
    const rules = Object.keys(destinations).map((name) => {
      const section = name.slice('hotsheet-'.length);
      const marker =
        section === 'workflow'
          ? '<!-- hotsheet-instructions-version: 1 -->'
          : `<!-- hotsheet:begin section=${section} v=1 -->`;
      const body = `${marker}\n\nA verified rule.\n`;
      fs.writeFileSync(path.join(source, `rules/${name}.md`), `---\nroot: true\n---\n\n${body}`);
      for (const destination of destinations[name])
        fs.mkdirSync(path.dirname(path.join(bundle, destination)), { recursive: true });
      return { name, path: `rules/${name}.md`, version: 1, sha256: createHash('sha256').update(body).digest('hex') };
    });
    const extraBody = '<!-- hotsheet:begin section=project-bootstrap v=1 -->\n\nAn optional rule.\n';
    fs.writeFileSync(path.join(source, 'rules/project-bootstrap.md'), `---\nroot: true\n---\n\n${extraBody}`);
    rules.push({
      name: 'project-bootstrap',
      path: 'rules/project-bootstrap.md',
      version: 1,
      sha256: createHash('sha256').update(extraBody).digest('hex'),
    });
    fs.writeFileSync(path.join(source, 'rules-manifest.json'), JSON.stringify({ schema_version: 1, rules }));
    const git = (...args) =>
      execFileSync('git', ['-C', source, ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
    git('init', '--quiet');
    git('add', 'rules', 'rules-manifest.json');
    git('-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '--quiet', '-m', 'fixture');
    const commit = git('rev-parse', 'HEAD');
    const args = ['--write', '--source', source, '--ref', commit, '--release', 'v1.0.0'];
    main(args, bundle);
    const lock = JSON.parse(fs.readFileSync(path.join(bundle, 'guidance/ai-playbook.lock.json')));
    assert.equal(lock.repository, repository);
    assert.deepEqual(lock.rules.map(({ name }) => name), Object.keys(destinations));
    main(['--check'], bundle);
    main(['--check', '--source', source], bundle);
    // An uncommitted source edit cannot change what the pinned revision supplies.
    fs.appendFileSync(path.join(source, rules[0].path), 'uncommitted edit');
    main(['--check', '--source', source], bundle);
    const changed = path.join(bundle, destinations[rules[0].name][0]);
    fs.appendFileSync(changed, 'drift');
    assert.throws(() => main(['--check'], bundle), /Guidance drift/);
    main(args, bundle);
    checkBundled(bundle, lock);
    assert.throws(() => readSource(source, 'main', 'v1'), /full immutable/);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
