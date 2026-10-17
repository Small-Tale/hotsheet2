#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const SHA = /^[0-9a-f]{40}$/i;
const TICKET_ROW = /^\s*[*●]*\s*(HS2-[A-Z0-9]+)\s/;
const COMMIT_MARKER = /^CI gate commit:[ \t]*([0-9a-f]{40})[ \t]*$/gim;

export function latestGreenRun(value) {
  if (!Array.isArray(value)) throw new Error('gh returned a non-array CI run list');
  const runs = value.filter(
    (run) =>
      run &&
      run.conclusion === 'success' &&
      run.status === 'completed' &&
      run.headBranch === 'main' &&
      run.event === 'push' &&
      run.workflowName === 'CI' &&
      SHA.test(run.headSha) &&
      Number.isInteger(run.databaseId) &&
      Number.isFinite(Date.parse(run.createdAt)),
  );
  runs.sort((left, right) => Date.parse(right.createdAt) - Date.parse(left.createdAt));
  return runs[0] ?? null;
}

export function startedSlugs(listOutput) {
  return [
    ...new Set(
      listOutput
        .split('\n')
        .map((line) => line.match(TICKET_ROW)?.[1])
        .filter(Boolean),
    ),
  ];
}

export function waitingCommit(ticketOutput) {
  if (!/^status: started$/m.test(ticketOutput) || !/^started_phase: final_testing$/m.test(ticketOutput)) {
    return null;
  }
  const notes = ticketOutput.split('<!-- hotsheet:notes:begin -->')[1] ?? '';
  const matches = [...notes.matchAll(COMMIT_MARKER)];
  return matches.at(-1)?.[1].toLowerCase() ?? null;
}

export function command(binary, args, options = {}) {
  const result = spawnSync(binary, args, { encoding: 'utf8', ...options });
  if (result.error) throw result.error;
  return result;
}

export function isCovered(commit, greenSha, run = command) {
  if (!SHA.test(commit) || !SHA.test(greenSha)) throw new Error('coverage requires full commit SHAs');
  for (const sha of [commit, greenSha]) {
    const known = run('git', ['cat-file', '-e', `${sha}^{commit}`]);
    if (known.status !== 0) return 'unknown';
  }
  const ancestry = run('git', ['merge-base', '--is-ancestor', commit, greenSha]);
  if (ancestry.status === 0) return 'covered';
  if (ancestry.status === 1) return 'uncovered';
  throw new Error(ancestry.stderr?.trim() || 'git ancestry check failed');
}

export function gateStatus({ covers, run = command } = {}) {
  const list = run('gh', [
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
  if (list.status !== 0) throw new Error(list.stderr?.trim() || 'gh run list failed');
  const green = latestGreenRun(JSON.parse(list.stdout));
  if (!green) return { green: null, covers: null, waiting: [] };

  const coverage = covers ? isCovered(covers, green.headSha, run) : null;
  const tickets = run('hotsheet-cli', ['ls', '--status', 'started']);
  if (tickets.status !== 0) throw new Error(tickets.stderr?.trim() || 'hotsheet-cli ls failed');
  const waiting = [];
  for (const slug of startedSlugs(tickets.stdout)) {
    const ticket = run('hotsheet-cli', ['show', slug]);
    if (ticket.status !== 0) throw new Error(ticket.stderr?.trim() || `hotsheet-cli show ${slug} failed`);
    const commit = waitingCommit(ticket.stdout);
    if (commit) waiting.push({ slug, commit, coverage: isCovered(commit, green.headSha, run) });
  }
  return { green, covers: coverage, waiting };
}

function main(args) {
  if (
    args[0] !== 'status' ||
    (args.length !== 1 && args.length !== 3) ||
    (args.length === 3 && (args[1] !== '--covers' || !SHA.test(args[2])))
  ) {
    throw new Error('usage: node scripts/ci-gate.mjs status [--covers <full-sha>]');
  }
  const result = gateStatus({ covers: args[2] });
  if (!result.green) {
    console.log('No successful main push CI run found in the latest 20 successful run records.');
    return;
  }
  console.log(`Last green main CI: ${result.green.headSha} (run ${result.green.databaseId})`);
  console.log(result.green.url);
  if (args[2]) console.log(`Covers ${args[2]}: ${result.covers}`);
  console.log('Waiting final_testing tickets with an explicit CI gate commit:');
  for (const ticket of result.waiting) console.log(`  ${ticket.slug} ${ticket.commit} ${ticket.coverage}`);
  if (result.waiting.length === 0) console.log('  none');
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
