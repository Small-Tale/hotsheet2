#!/usr/bin/env node

import { spawn, spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import { StringDecoder } from 'node:string_decoder';
import { fileURLToPath } from 'node:url';
import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';

const SHA = /^[0-9a-f]{40}$/i;
const TICKET_ROW = /^\s*[*●]*\s*(HS2-[A-Z0-9]+)\s/;
const COMMIT_MARKER = /^CI gate commit:[ \t]*([0-9a-f]{40})[ \t]*$/i;
const MAX_SCAN_LINE = 256;

// HS2-GT57BQ: a ci-staged project's integration branch runs the same CI workflow.
const BRANCH = /^[A-Za-z0-9][A-Za-z0-9._/-]*$/;

export function latestGreenRun(value, branch = 'main') {
  if (!Array.isArray(value)) throw new Error('gh returned a non-array CI run list');
  const runs = value.filter(
    (run) =>
      run &&
      run.conclusion === 'success' &&
      run.status === 'completed' &&
      run.headBranch === branch &&
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

function ticketScanner() {
  let line = '';
  let discardLine = false;
  let inHeader = true;
  let inNotes = false;
  let frontmatterDelimiters = 0;
  let started = false;
  let finalTesting = false;
  let commit = null;

  function readLine() {
    if (discardLine) return;
    const value = line.endsWith('\r') ? line.slice(0, -1) : line;
    if (inHeader && value === '---' && ++frontmatterDelimiters === 2) inHeader = false;
    if (value === '<!-- hotsheet:body:begin -->') inHeader = false;
    if (value === '<!-- hotsheet:notes:begin -->') inNotes = true;
    if (value === '<!-- hotsheet:notes:end -->') inNotes = false;
    if (inHeader) {
      if (value === 'status: started') started = true;
      if (value === 'started_phase: final_testing') finalTesting = true;
    }
    if (inNotes) {
      const marker = value.match(COMMIT_MARKER);
      if (marker) commit = marker[1].toLowerCase();
    }
  }

  function append(part) {
    if (discardLine) return;
    if (line.length + part.length > MAX_SCAN_LINE) {
      line = '';
      discardLine = true;
    } else {
      line += part;
    }
  }

  return {
    push(chunk) {
      let start = 0;
      let end = chunk.indexOf('\n', start);
      while (end !== -1) {
        append(chunk.slice(start, end));
        readLine();
        line = '';
        discardLine = false;
        start = end + 1;
        end = chunk.indexOf('\n', start);
      }
      append(chunk.slice(start));
    },
    finish() {
      if (line || discardLine) readLine();
      return started && finalTesting ? commit : null;
    },
  };
}

export function waitingCommit(ticketOutput) {
  const scanner = ticketScanner();
  scanner.push(ticketOutput);
  return scanner.finish();
}

// `show` can contain years of notes. Scan its stdout instead of retaining it in
// spawnSync's fixed buffer; only short metadata/marker lines matter (HS2-HBGGRQ).
export function streamWaitingCommit(binary, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, args, { env: withoutGitRepositoryEnv(process.env), stdio: ['ignore', 'pipe', 'pipe'] });
    const scanner = ticketScanner();
    const decoder = new StringDecoder('utf8');
    let stderr = '';
    child.stdout.on('data', (chunk) => scanner.push(decoder.write(chunk)));
    child.stderr.on('data', (chunk) => {
      stderr += chunk.toString('utf8').slice(0, Math.max(0, 4096 - stderr.length));
    });
    child.on('error', reject);
    child.on('close', (status, signal) => {
      scanner.push(decoder.end());
      if (status !== 0) {
        reject(new Error(stderr.trim() || `${binary} exited with ${signal ?? status}`));
      } else {
        resolve(scanner.finish());
      }
    });
  });
}

export function command(binary, args, options = {}) {
  const result = spawnSync(binary, args, { encoding: 'utf8', env: withoutGitRepositoryEnv(process.env), ...options });
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

export async function gateStatus({
  covers,
  branch = 'main',
  run = command,
  readTicket = (slug) => streamWaitingCommit('hotsheet-cli', ['show', slug]),
} = {}) {
  if (!BRANCH.test(branch) || branch.includes('..')) throw new Error('Use a plain branch name');
  const list = run('gh', [
    'run',
    'list',
    '--branch',
    branch,
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
  const green = latestGreenRun(JSON.parse(list.stdout), branch);
  if (!green) return { green: null, covers: null, waiting: [] };

  const coverage = covers ? isCovered(covers, green.headSha, run) : null;
  const tickets = run('hotsheet-cli', ['ls', '--status', 'started']);
  if (tickets.status !== 0) throw new Error(tickets.stderr?.trim() || 'hotsheet-cli ls failed');
  const waiting = [];
  for (const slug of startedSlugs(tickets.stdout)) {
    const commit = await readTicket(slug);
    if (commit) waiting.push({ slug, commit, coverage: isCovered(commit, green.headSha, run) });
  }
  return { green, covers: coverage, waiting };
}

export function parseStatusArgs(args) {
  const usage = 'usage: node scripts/ci-gate.mjs status [--branch <name>] [--covers <full-sha>]';
  if (args[0] !== 'status') throw new Error(usage);
  const options = { branch: 'main', covers: undefined };
  const seen = new Set();
  for (let index = 1; index < args.length; index += 2) {
    const [flag, value] = [args[index], args[index + 1]];
    if (seen.has(flag) || !value) throw new Error(usage);
    seen.add(flag);
    if (flag === '--covers' && SHA.test(value)) options.covers = value;
    else if (flag === '--branch' && BRANCH.test(value) && !value.includes('..')) options.branch = value;
    else throw new Error(usage);
  }
  return options;
}

async function main(args) {
  const options = parseStatusArgs(args);
  const result = await gateStatus(options);
  if (!result.green) {
    console.log(`No successful ${options.branch} push CI run found in the latest 20 successful run records.`);
    return;
  }
  console.log(`Last green ${options.branch} CI: ${result.green.headSha} (run ${result.green.databaseId})`);
  console.log(result.green.url);
  if (options.covers) console.log(`Covers ${options.covers}: ${result.covers}`);
  console.log('Waiting final_testing tickets with an explicit CI gate commit:');
  for (const ticket of result.waiting) console.log(`  ${ticket.slug} ${ticket.commit} ${ticket.coverage}`);
  if (result.waiting.length === 0) console.log('  none');
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
