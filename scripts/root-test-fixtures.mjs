#!/usr/bin/env node
// HS2-WC74CR: regenerate scripts/root-test-fixtures.json from what the root tooling tests read.
//
//   node scripts/root-test-fixtures.mjs --write   # run the tooling tests under the recorder and write it
//
// It runs every root tooling test once with the read recorder (scripts/root-test-reads.mjs) in record
// mode and writes each tracked repository file outside scripts/ that a test read, with its readers,
// except files a reviewed `directories` entry already declares for that reader.
// The tooling stage enforces the result on every run, so a stale manifest fails there first.
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { ROOT_TEST_READS_PRELOAD, ROOT_TOOLING_TESTS } from './gate-plan.mjs';
import {
  buildManifest,
  formatManifest,
  LOG_ENV,
  MANIFEST_PATH,
  readManifest,
  RECORD_ENV,
  TEST_ENV,
} from './root-test-reads.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

export function main(argv = process.argv.slice(2), { log = console.log } = {}) {
  if (!argv.includes('--write')) {
    log('Usage: node scripts/root-test-fixtures.mjs --write');
    return 2;
  }
  const record = mkdtempSync(join(tmpdir(), 'hs2-root-test-fixtures-'));
  try {
    const env = { ...process.env, [RECORD_ENV]: record };
    delete env[TEST_ENV];
    delete env[LOG_ENV];
    const files = ROOT_TOOLING_TESTS.map((name) => `scripts/${name}.test.mjs`);
    const run = spawnSync(process.execPath, ['--test', '--import', ROOT_TEST_READS_PRELOAD, ...files], {
      cwd: root,
      env,
      stdio: 'inherit',
    });
    const reads = {};
    for (const file of readdirSync(record))
      reads[file.replace(/\.json$/, '')] = JSON.parse(readFileSync(join(record, file), 'utf8'));
    writeFileSync(
      join(root, MANIFEST_PATH),
      formatManifest(buildManifest(reads, readManifest(root).directories ?? {})),
    );
    log(`Wrote ${MANIFEST_PATH}.`);
    if (run.status === 0) return 0;
    // A test that asserts on the manifest fails until it exists; a failed case may also have stopped
    // before a read. Fix the failure and regenerate; the tooling stage flags anything still missing.
    log('Some root tooling tests failed; fix them and regenerate before relying on the manifest.');
    return run.status ?? 1;
  } finally {
    rmSync(record, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = main();
