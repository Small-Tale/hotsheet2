#!/usr/bin/env node
// HS2-A4HD9X: synthetic, credential-free local ticketing performance baseline.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';

const root = fileURLToPath(new URL('..', import.meta.url));
const target = process.env.CARGO_TARGET_DIR ? resolve(root, process.env.CARGO_TARGET_DIR) : join(root, 'target');
const binary = join(target, 'debug/hotsheet-cli');
const work = mkdtempSync(join(tmpdir(), 'hs2-a4hd9x-ticketing-'));
const store = join(work, 'store');
const env = {
  ...process.env,
  HOTSHEET_HOME: join(work, 'home'),
  GIT_AUTHOR_NAME: 'Synthetic Benchmark',
  GIT_AUTHOR_EMAIL: 'synthetic@example.invalid',
  GIT_COMMITTER_NAME: 'Synthetic Benchmark',
  GIT_COMMITTER_EMAIL: 'synthetic@example.invalid',
};

function run(command, args, cwd = root) {
  const start = performance.now();
  const result = spawnSync(command, args, { cwd, env, encoding: 'utf8', maxBuffer: 4 * 1024 * 1024 });
  if (result.status !== 0)
    throw new Error(`${command} ${args[0] ?? ''} failed: ${result.stderr || result.stdout || result.error?.message}`);
  return { ms: performance.now() - start, output: result.stdout };
}

function p95(values) {
  return [...values].sort((a, b) => a - b)[Math.ceil(values.length * 0.95) - 1];
}

try {
  run('cargo', ['build', '--quiet', '-p', 'hotsheet-cli', '--bin', 'hotsheet-cli']);
  run(binary, ['-C', store, 'init', '--prefix', 'HS']);
  let slug;
  for (let index = 0; index < 100; index += 1) {
    const { output } = run(binary, ['-C', store, 'new', '--title', `Synthetic ticket ${index}`, '--up-next']);
    if (index === 0) slug = /\bHS-[A-Z0-9]{6,}\b/.exec(output)?.[0];
  }
  assert.ok(slug, 'synthetic ticket slug is present');
  const listMs = [];
  const claimMs = [];
  for (let index = 0; index < 20; index += 1) {
    const list = run(binary, ['-C', store, 'ls', '--up-next']);
    assert.match(list.output, /Synthetic ticket/);
    listMs.push(list.ms);
    const claimed = run(binary, ['-C', store, 'claim', slug, '--worker', 'synthetic-benchmark']);
    assert.match(claimed.output, /Claimed/);
    const released = run(binary, ['-C', store, 'release', slug, '--worker', 'synthetic-benchmark']);
    assert.match(released.output, /Released/);
    claimMs.push(claimed.ms + released.ms);
  }
  const result = {
    ticket_count: 100,
    samples: 20,
    list_p95_ms: Math.round(p95(listMs)),
    claim_release_p95_ms: Math.round(p95(claimMs)),
    list_limit_ms: 1000,
    claim_release_limit_ms: 2000,
  };
  console.log(`HS2-A4HD9X local ticketing performance: ${JSON.stringify(result)}`);
  assert.ok(result.list_p95_ms <= result.list_limit_ms, 'Up Next list exceeded the local performance baseline');
  assert.ok(
    result.claim_release_p95_ms <= result.claim_release_limit_ms,
    'Claim and release exceeded the local performance baseline',
  );
} finally {
  rmSync(work, { recursive: true, force: true });
}
