import assert from 'node:assert/strict';
import test from 'node:test';
import { areaFor, citationProblem, citations, isMapped, isSource, requiredEntries } from './architecture-coverage.mjs';

const config = {
  source_extensions: ['.rs', '.ts'],
  test_patterns: ['/tests/', '_tests.rs', '.test.ts'],
  areas: [
    { key: 'orch', roots: ['crates/t/src/orchestration', 'crates/t/src/claim_'] },
    { key: 'core', roots: ['crates/t/src'], list_every_entry: ['crates/t/src/routes'] },
  ],
};

test('HS2-H0T69J source filter excludes tests and other extensions', () => {
  assert.equal(isSource('crates/t/src/a.rs', config), true);
  assert.equal(isSource('crates/t/src/a_tests.rs', config), false);
  assert.equal(isSource('crates/t/tests/a.rs', config), false);
  assert.equal(isSource('clients/web/src/a.test.ts', config), false);
  assert.equal(isSource('crates/t/src/a.md', config), false);
});

test('HS2-H0T69J first matching area wins, including file-prefix roots', () => {
  assert.equal(areaFor('crates/t/src/orchestration/run.rs', config).key, 'orch');
  assert.equal(areaFor('crates/t/src/claim_gate.rs', config).key, 'orch');
  assert.equal(areaFor('crates/t/src/store.rs', config).key, 'core');
  assert.equal(areaFor('crates/x/src/store.rs', config), null);
});

test('HS2-H0T69J required entries: large files, large directories and listed entries', () => {
  const files = {
    'crates/t/src/big.rs': 600,
    'crates/t/src/small.rs': 10,
    'crates/t/src/sync/a.rs': 300,
    'crates/t/src/sync/b.rs': 300,
    'crates/t/src/tiny/a.rs': 20,
    'crates/t/src/routes/x.rs': 5,
  };
  const required = requiredEntries(files, config.areas[1], 500);
  assert.deepEqual(
    [...required.keys()].sort(),
    ['crates/t/src/big.rs', 'crates/t/src/routes/x.rs', 'crates/t/src/sync'].sort(),
  );
  assert.equal(required.get('crates/t/src/sync').lines, 600);
  assert.equal(required.has('crates/t/src'), false, 'the area root itself is never a required entry');
});

test('HS2-H0T69J mapping requires a backticked path, with or without extension or line', () => {
  assert.equal(isMapped('crates/t/src/big.rs', 'see `crates/t/src/big.rs:12`'), true);
  assert.equal(isMapped('crates/t/src/big.rs', 'see `crates/t/src/big`'), true);
  assert.equal(isMapped('crates/t/src/sync', '`crates/t/src/sync/`'), true);
  assert.equal(isMapped('crates/t/src/big.rs', 'crates/t/src/big.rs without backticks'), false);
  assert.equal(isMapped('crates/t/src/big.rs', '`crates/t/src/big.rs.bak`'), false);
});

test('HS2-H0T69J citations parse single lines and ranges and report problems', () => {
  const found = citations('a `crates/t/src/a.rs:10` b `clients/web/src/x.ts:3-9` c `notapath` d `x.rs`');
  assert.deepEqual(found, [
    { path: 'crates/t/src/a.rs', start: 10, end: 10 },
    { path: 'clients/web/src/x.ts', start: 3, end: 9 },
  ]);
  assert.equal(citationProblem(found[0], null), 'file not found');
  assert.equal(citationProblem(found[0], 9), 'line 10 beyond end (9 lines)');
  assert.equal(citationProblem(found[0], 10), null);
  assert.equal(citationProblem({ path: 'a.rs', start: 5, end: 2 }, 10), 'invalid range');
});
