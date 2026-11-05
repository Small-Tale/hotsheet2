#!/usr/bin/env node
// HS2-9G0M3V: ticket-side steps of batch integration. The coordination CLI owns the batch state
// (`coordination batch-*`); this script writes each ticket's manifest note and integration record
// after the one push, reads the covering CI run once, files red-run follow-ups through the
// HS2-HAHX20 attribution tool, and clears the circuit breaker after a green main run. It never
// polls: run `ci` when the CI gate status flow reports that the batch commit's run finished.
//
//   node scripts/integration-batch.mjs record  --project <main> --store <store> --batch <id> --integrator-id <id> --integrator-session <s>
//   node scripts/integration-batch.mjs ci      --project <main> --store <store> --batch <id> --integrator-id <id> --integrator-session <s>
//   node scripts/integration-batch.mjs recover --project <main> --store <store> --integrator-id <id> --integrator-session <s>
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { latestGreenRun } from './ci-gate.mjs';
import { recordIntegration } from './integration-record.mjs';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const SHA = /^[0-9a-f]{40}$/i;
export const MARKER = 'batch-manifest:';
const RED = new Set(['failure', 'timed_out', 'startup_failure']);

export function command(binary, args, { input, cwd } = {}) {
  const result = spawnSync(binary, args, { encoding: 'utf8', input, cwd, maxBuffer: 64 * 1024 * 1024 });
  if (result.error) throw result.error;
  return result;
}

function ok(result, what) {
  if (result.status !== 0) throw new Error(`${what} failed: ${(result.stderr || result.stdout || '').trim()}`);
  return result.stdout;
}

/** Run one `coordination` subcommand and parse its JSON. */
function coordination(options, args, run) {
  const out = run(options.cli, ['-C', options.store, 'coordination', '--project', options.project, ...args]);
  return JSON.parse(ok(out, `coordination ${args[0]}`));
}

function identity(options) {
  return ['--id', options.integratorId, '--session', options.integratorSession];
}

function actor(options) {
  return ['--actor-role', 'ai', '--actor-id', options.integratorId];
}

function batchOf(options, run) {
  const ledger = coordination(options, ['batch-status'], run);
  const batch = ledger.batches?.[options.batch];
  if (!batch) throw new Error(`Unknown batch ${options.batch}`);
  return { ledger, batch };
}

function projectId(options, run) {
  return coordination(options, ['status'], run).state.project_id;
}

export function qualified(project, ticket) {
  return `@${project}/${ticket.connection_id}:${ticket.native_id}`;
}

/** The regular note each batch ticket carries: the manifest and its CI gate commit. */
export function manifestNote(batch, ticket, project) {
  const manifest = {
    batch_id: batch.id,
    ticket: qualified(project, ticket.ticket),
    slug: ticket.slug,
    operation: ticket.assignment,
    source_commits: [ticket.source_commit],
    shipping_commits: ticket.shipping_commits,
    main_commit: batch.main_commit,
    members: batch.tickets.map((member) => member.slug),
    smoke_checks: batch.smoke_checks,
  };
  return [
    '## Batch manifest',
    '',
    `Integrated with ${batch.tickets.length - 1} other ticket(s) in batch ${batch.id}, gated by the smoke checks below and pushed once. The batch CI run tests them together (HS2-9G0M3V).`,
    '',
    `CI gate commit: ${batch.main_commit}`,
    '',
    '```json',
    JSON.stringify(manifest, null, 2),
    '```',
    '',
    `${MARKER} ${batch.id}`,
  ].join('\n');
}

/** After the one push: a manifest note and an integration record for every batch ticket. */
export function record(options, { run = command, recordIntegrationFn = recordIntegration } = {}) {
  const { batch } = batchOf(options, run);
  if (batch.state === 'integrating' || batch.state === 'aborted')
    throw new Error(`Batch ${batch.id} is ${batch.state}; record it after coordination batch-pushed`);
  if (!SHA.test(batch.main_commit ?? '')) throw new Error(`Batch ${batch.id} has no pushed main commit`);
  const project = projectId(options, run);
  const results = [];
  for (const ticket of batch.tickets) {
    const shown = ok(run(options.cli, ['-C', options.store, 'show', ticket.slug]), `show ${ticket.slug}`);
    const noted = shown.split('\n').some((line) => line.trim() === `${MARKER} ${batch.id}`);
    if (!noted)
      ok(
        run(options.cli, ['-C', options.store, 'edit', ticket.slug, '--note-file', '-', ...actor(options)], {
          input: manifestNote(batch, ticket, project),
        }),
        `note ${ticket.slug}`,
      );
    recordIntegrationFn({
      project: options.project,
      ticket: qualified(project, ticket.ticket),
      operation: ticket.assignment,
      source: [ticket.source_commit],
      shipping: ticket.shipping_commits,
    });
    results.push({ slug: ticket.slug, note: noted ? 'existing' : 'added', integration: 'recorded' });
  }
  return { batch: batch.id, main_commit: batch.main_commit, tickets: results };
}

/** The newest CI push run on main for exactly this commit, or null. */
export function batchRun(runs, mainCommit) {
  if (!Array.isArray(runs)) throw new Error('gh returned a non-array CI run list');
  const matching = runs.filter(
    (item) =>
      item &&
      item.headSha === mainCommit &&
      item.workflowName === 'CI' &&
      item.event === 'push' &&
      Number.isInteger(item.databaseId),
  );
  matching.sort((left, right) => Date.parse(right.createdAt) - Date.parse(left.createdAt));
  return matching[0] ?? null;
}

/** The HS2-HAHX20 manifest (version 1) for a red batch run. */
export function attributionManifest(batch, project) {
  return {
    version: 1,
    batch_id: batch.id,
    project_id: project,
    shipping_sha: batch.main_commit,
    tickets: batch.tickets.map((ticket) => ({
      slug: ticket.slug,
      ticket: { project_id: project, ...ticket.ticket },
      source_commits: [ticket.source_commit],
      shipping_commits: ticket.shipping_commits,
      changed_files: ticket.paths.filter((path) => !path.includes('#')),
    })),
  };
}

/**
 * Read the batch commit's CI run once. Green records the result; red records it, files one
 * follow-up per failure through the attribution tool and records them, so the batch tickets may
 * complete. A missing, running or cancelled run records nothing.
 */
export function ci(options, { run = command, scratch } = {}) {
  const { batch } = batchOf(options, run);
  if (!['ci', 'red'].includes(batch.state))
    return { batch: batch.id, status: batch.state, completable: batch.state === 'green' };
  const listed = run('gh', [
    'run',
    'list',
    '--branch',
    'main',
    '--workflow',
    'CI',
    '--event',
    'push',
    '--commit',
    batch.main_commit,
    '--limit',
    '10',
    '--json',
    'databaseId,status,conclusion,headSha,event,workflowName,createdAt,url',
  ]);
  const found = batchRun(JSON.parse(ok(listed, 'gh run list')), batch.main_commit);
  if (!found) return { batch: batch.id, status: 'pending', reason: `no CI push run for ${batch.main_commit} yet` };
  if (found.status !== 'completed') return { batch: batch.id, status: 'pending', run: found.databaseId };
  const runId = String(found.databaseId);
  const resultArgs = [...identity(options), '--batch', batch.id, '--run', runId];
  if (found.conclusion === 'success') {
    const result = coordination(options, ['batch-result', ...resultArgs, '--conclusion', 'success'], run);
    return { batch: batch.id, status: 'green', run: found.databaseId, completable: true, breaker: result.breaker };
  }
  if (!RED.has(found.conclusion))
    return { batch: batch.id, status: 'inconclusive', run: found.databaseId, conclusion: found.conclusion };
  const result = coordination(options, ['batch-result', ...resultArgs, '--conclusion', 'failure'], run);
  if (result.batch.attributed)
    return { batch: batch.id, status: 'red', run: found.databaseId, completable: true, notify: result.notify };
  const project = projectId(options, run);
  const directory = scratch ?? mkdtempSync(join(tmpdir(), 'hs2-batch-'));
  const manifestPath = join(directory, `${batch.id}-manifest.json`);
  writeFileSync(manifestPath, `${JSON.stringify(attributionManifest(batch, project), null, 2)}\n`);
  const attributed = run(process.execPath, [
    options.attribution,
    '--manifest',
    manifestPath,
    '--store',
    options.store,
    '--run',
    runId,
    '--project',
    options.project,
    '--cli',
    options.cli,
    ...actor(options),
  ]);
  const output = JSON.parse(ok(attributed, 'CI failure attribution (HS2-HAHX20)'));
  const failures = (output.filed ?? []).filter((filed) => filed.kind !== 'flaky');
  if (!failures.length)
    return {
      batch: batch.id,
      status: 'red',
      run: found.databaseId,
      completable: false,
      notify: result.notify,
      reason: 'attribution filed no failure follow-up; file one by hand and record it with coordination batch-follow-ups',
    };
  const outputPath = join(directory, `${batch.id}-attribution.json`);
  writeFileSync(outputPath, `${JSON.stringify(output)}\n`);
  const recorded = coordination(
    options,
    ['batch-follow-ups', ...identity(options), '--batch', batch.id, '--attribution-file', outputPath],
    run,
  );
  return {
    batch: batch.id,
    status: 'red',
    run: found.databaseId,
    completable: true,
    notify: result.notify,
    follow_ups: recorded.failures.map((failure) => failure.follow_up),
  };
}

/** Clear a tripped breaker once the newest green main run covers the last red batch push. */
export function recover(options, { run = command } = {}) {
  const listed = run('gh', [
    'run',
    'list',
    '--branch',
    'main',
    '--workflow',
    'CI',
    '--event',
    'push',
    '--status',
    'success',
    '--limit',
    '20',
    '--json',
    'conclusion,status,headBranch,event,workflowName,headSha,databaseId,createdAt,url',
  ]);
  const green = latestGreenRun(JSON.parse(ok(listed, 'gh run list')));
  if (!green) return { status: 'red', reason: 'no successful main push run yet' };
  const breaker = coordination(
    options,
    ['batch-recover', ...identity(options), '--run', String(green.databaseId), '--main-commit', green.headSha],
    run,
  );
  return { status: 'recovered', run: green.databaseId, breaker };
}

export function parseArgs(argv) {
  const [action, ...rest] = argv;
  const options = {
    action,
    cli: process.env.HOTSHEET_CLI ?? 'hotsheet-cli',
    attribution: join(root, 'scripts/ci-failure-attribution.mjs'),
  };
  const names = {
    '--project': 'project',
    '--store': 'store',
    '--batch': 'batch',
    '--integrator-id': 'integratorId',
    '--integrator-session': 'integratorSession',
    '--cli': 'cli',
    '--attribution': 'attribution',
  };
  for (let index = 0; index < rest.length; index += 2) {
    const name = names[rest[index]];
    if (!name || !rest[index + 1]) throw new Error(`Unknown or valueless option ${rest[index]}`);
    options[name] = rest[index + 1];
  }
  if (!['record', 'ci', 'recover'].includes(action)) throw new Error('Expected record, ci or recover');
  for (const required of ['project', 'store', 'integratorId', 'integratorSession'])
    if (!options[required]) throw new Error(`--${required.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)} is required`);
  if (action !== 'recover' && !options.batch) throw new Error('--batch is required');
  options.project = resolve(options.project);
  options.store = resolve(options.store);
  return options;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const options = parseArgs(process.argv.slice(2));
    const result = { record, ci, recover }[options.action](options);
    console.log(JSON.stringify(result, null, 2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
