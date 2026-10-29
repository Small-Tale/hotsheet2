import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { integrationRecordPath, recordIntegration } from './integration-record.mjs';

const run = (program, args, cwd) =>
  execFileSync(program, args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
const git = (cwd, ...args) => run('git', args, cwd);

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'hs2-39d5ys-'));
  const repo = join(root, 'repo');
  const remote = join(root, 'remote.git');
  git(root, 'init', '-b', 'main', repo);
  git(repo, 'config', 'user.name', 'Synthetic Test');
  git(repo, 'config', 'user.email', 'test@invalid.example');
  writeFileSync(join(repo, 'seed'), 'seed\n');
  git(repo, 'add', 'seed');
  git(repo, 'commit', '-m', 'Seed');
  git(root, 'init', '--bare', '-b', 'main', remote);
  git(repo, 'remote', 'add', 'origin', remote);
  git(repo, 'push', 'origin', 'main');
  const base = git(repo, 'rev-parse', 'HEAD');
  return { root, repo, remote, base, dispose: () => rmSync(root, { recursive: true, force: true }) };
}

function commit(repo, name, content) {
  writeFileSync(join(repo, name), content);
  git(repo, 'add', name);
  git(repo, 'commit', '-m', `HS2-39D5YS Add ${name}`);
  return git(repo, 'rev-parse', 'HEAD');
}

const ticket = '@project-a/connection-a:ticket-123';

test('HS2-39D5YS records a reviewed rewritten source after remote main contains it', () => {
  const f = fixture();
  try {
    git(f.repo, 'switch', '-c', 'worker');
    const source = commit(f.repo, 'work', 'original\n');
    git(f.repo, 'switch', 'main');
    const shipping = commit(f.repo, 'work', 'corrected during integration\n');
    assert.notEqual(source, shipping);
    assert.throws(
      () =>
        recordIntegration({ project: f.repo, ticket, operation: 'handoff-1', source: [source], shipping: [shipping] }),
      /does not contain shipping/,
    );
    assert.equal(existsSync(integrationRecordPath(f.repo, ticket, 'handoff-1')), false);
    git(f.repo, 'push', 'origin', 'main');
    const first = recordIntegration({
      project: f.repo,
      ticket,
      operation: 'handoff-1',
      source: [source],
      shipping: [shipping],
    });
    assert.equal(first.replayed, false);
    assert.deepEqual(first.record.source_commits, [source]);
    assert.deepEqual(first.record.shipping_commits, [shipping]);
    assert.equal(first.record.remote_tip, shipping);
    const bytes = readFileSync(first.path, 'utf8');
    git(f.repo, 'branch', '-D', 'worker');
    git(f.repo, 'reflog', 'expire', '--expire=now', '--all');
    git(f.repo, 'gc', '--prune=now');
    assert.equal(git(f.repo, 'cat-file', '-t', source), 'commit');
    assert.equal(git(f.repo, 'rev-parse', `refs/hotsheet/integration-source/${source}`), source);
    assert.equal(
      recordIntegration({ project: f.repo, ticket, operation: 'handoff-1', source: [source], shipping: [shipping] })
        .replayed,
      true,
    );
    const other = join(f.root, 'other-worktree');
    git(f.repo, 'worktree', 'add', '-b', 'other', other);
    assert.equal(integrationRecordPath(other, ticket, 'handoff-1'), first.path);
    assert.equal(
      recordIntegration({ project: other, ticket, operation: 'handoff-1', source: [source], shipping: [shipping] })
        .replayed,
      true,
    );
    assert.equal(readFileSync(first.path, 'utf8'), bytes);
    assert.throws(
      () =>
        recordIntegration({ project: f.repo, ticket, operation: 'handoff-1', source: [source], shipping: [f.base] }),
      /different mapping/,
    );
    assert.throws(
      () =>
        recordIntegration({ project: f.repo, ticket, operation: 'handoff-1', source: [f.base], shipping: [shipping] }),
      /different mapping/,
    );
    assert.throws(() => git(f.repo, 'rev-parse', '--verify', `refs/hotsheet/integration-source/${f.base}`));
    assert.notEqual(
      integrationRecordPath(f.repo, ticket, 'handoff-1'),
      integrationRecordPath(f.repo, '@project-b/connection-a:ticket-123', 'handoff-1'),
    );
    assert.notEqual(
      integrationRecordPath(f.repo, ticket, 'handoff-1'),
      integrationRecordPath(f.repo, '@project-a/connection-a:group/repo#123', 'handoff-1'),
    );
  } finally {
    f.dispose();
  }
});

test('HS2-39D5YS records multiple source and shipping commits without duplicate operations', () => {
  const f = fixture();
  try {
    const first = commit(f.repo, 'one', 'one\n');
    const second = commit(f.repo, 'two', 'two\n');
    git(f.repo, 'push', 'origin', 'main');
    const recorded = recordIntegration({
      project: f.repo,
      ticket,
      operation: 'merge-2',
      source: [first, second],
      shipping: [first, second],
    });
    assert.deepEqual(recorded.record.source_commits, [first, second]);
    assert.deepEqual(recorded.record.shipping_commits, [first, second]);
    assert.throws(
      () =>
        recordIntegration({ project: f.repo, ticket, operation: 'bad', source: [first, first], shipping: [second] }),
      /duplicate SHA/,
    );
    assert.throws(
      () =>
        recordIntegration({
          project: f.repo,
          ticket: '@unqualified',
          operation: 'bad',
          source: [first],
          shipping: [second],
        }),
      /qualified/,
    );
  } finally {
    f.dispose();
  }
});
