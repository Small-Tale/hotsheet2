import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { test } from 'node:test';

import { verifyPlaywrightShards } from './verify-playwright-shards.mjs';

function fixture(shards) {
  const root = mkdtempSync(resolve(tmpdir(), 'hs2-shard-'));
  const artifacts = resolve(root, 'artifacts');
  const destination = resolve(root, 'merged');
  mkdirSync(artifacts);
  for (const shard of shards) {
    const directory = resolve(artifacts, `playwright-blob-${shard}`);
    mkdirSync(directory);
    writeFileSync(resolve(directory, `report-${shard}.zip`), `synthetic-${shard}`);
  }
  return { root, artifacts, destination };
}

test('HS2-6CK09C accepts exactly four separate browser shard artifacts', (context) => {
  const { root, artifacts, destination } = fixture([1, 2, 3, 4]);
  context.after(() => rmSync(root, { recursive: true, force: true }));
  assert.deepEqual(verifyPlaywrightShards(artifacts, destination, 4), [
    'report-1.zip',
    'report-2.zip',
    'report-3.zip',
    'report-4.zip',
  ]);
  assert.deepEqual(readdirSync(destination).sort(), ['report-1.zip', 'report-2.zip', 'report-3.zip', 'report-4.zip']);
});

test('HS2-6CK09C refuses the observed odd-shard-only CI run before merging', (context) => {
  const { root, artifacts, destination } = fixture([1, 3]);
  context.after(() => rmSync(root, { recursive: true, force: true }));
  assert.throws(() => verifyPlaywrightShards(artifacts, destination, 4), /expected.*playwright-blob-2/);
  assert.equal(existsSync(destination), false);
});

test('HS2-6CK09C refuses a wrong or extra report inside a named shard', (context) => {
  const { root, artifacts, destination } = fixture([1, 2, 3, 4]);
  context.after(() => rmSync(root, { recursive: true, force: true }));
  writeFileSync(resolve(artifacts, 'playwright-blob-2', 'unexpected.zip'), 'synthetic');
  assert.throws(() => verifyPlaywrightShards(artifacts, destination, 4), /Invalid playwright-blob-2 artifact/);
  assert.equal(existsSync(destination), false);
});

test('HS2-6CK09C pins the workflow preflight before the merge to the four-shard matrix', () => {
  const workflow = readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8');
  const localGate = readFileSync(new URL('./check-full.sh', import.meta.url), 'utf8');
  const gatePlan = readFileSync(new URL('./gate-plan.mjs', import.meta.url), 'utf8');
  assert.match(workflow, /shard: \[1, 2, 3, 4\]/);
  assert.ok(
    workflow.indexOf('name: Require every browser shard report') >
      workflow.indexOf('name: Download the shard blob reports'),
  );
  assert.ok(
    workflow.indexOf('name: Merge the shard reports') > workflow.indexOf('name: Require every browser shard report'),
  );
  assert.match(
    workflow,
    /verify-playwright-shards\.mjs clients\/web\/all-blob-reports clients\/web\/verified-blob-reports 4/,
  );
  assert.doesNotMatch(workflow, /merge-multiple: true/);
  assert.match(workflow, /run: node --test scripts\/verify-playwright-shards\.test\.mjs/);
  assert.match(localGate, /node --test scripts\/verify-playwright-shards\.test\.mjs/);
  assert.match(gatePlan, /\.::node --test scripts\/verify-playwright-shards\.test\.mjs/);
});
