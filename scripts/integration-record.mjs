#!/usr/bin/env node
// HS2-39D5YS: explicit source-to-shipping evidence for checkout Code Review.
import { execFileSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { existsSync, linkSync, mkdirSync, readFileSync, unlinkSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const sha = /^[0-9a-f]{40}$/i;
const refPart = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;
const operationPart = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;

function git(project, args) {
  return execFileSync('git', args, { cwd: project, encoding: 'utf8', timeout: 120_000 }).trim();
}

function qualifiedTicket(value) {
  const match = /^@([^/]+)\/([^:]+):(.+)$/.exec(value ?? '');
  if (!match || !refPart.test(match[1]) || !refPart.test(match[2]) || /[\x00-\x1f\x7f]/.test(match[3]))
    throw new Error('Ticket must be a qualified @project/connection:native-id');
  return { project_id: match[1], connection_id: match[2], native_id: match[3] };
}

export function integrationRecordPath(project, ticket, operation) {
  qualifiedTicket(ticket);
  if (!operationPart.test(operation ?? '')) throw new Error('Operation needs a stable id');
  const common = git(project, ['rev-parse', '--path-format=absolute', '--git-common-dir']);
  const ticketHash = createHash('sha256').update(ticket).digest('hex');
  return join(common, 'hotsheet-integrations', ticketHash, `${operation}.json`);
}

function uniqueCommits(values, label) {
  if (!values?.length || !values.every((value) => sha.test(value)))
    throw new Error(`${label} needs one or more full commit SHAs`);
  const commits = [...new Set(values.map((value) => value.toLowerCase()))];
  if (commits.length !== values.length) throw new Error(`${label} contains a duplicate SHA`);
  return commits;
}

export function recordIntegration({
  project,
  ticket,
  operation,
  source,
  shipping,
  remote = 'origin',
  branch = 'main',
}) {
  project = resolve(project);
  const identity = qualifiedTicket(ticket);
  if (!refPart.test(remote) || !refPart.test(branch)) throw new Error('Use a named remote and branch');
  const sourceCommits = uniqueCommits(source, 'Source');
  const shippingCommits = uniqueCommits(shipping, 'Shipping');
  for (const commit of [...sourceCommits, ...shippingCommits]) git(project, ['cat-file', '-e', `${commit}^{commit}`]);
  const remoteLine = git(project, ['ls-remote', '--exit-code', remote, `refs/heads/${branch}`]);
  const [remoteSha, remoteRef, extra] = remoteLine.split(/\s+/);
  if (!sha.test(remoteSha ?? '') || remoteRef !== `refs/heads/${branch}` || extra)
    throw new Error('Remote shipping branch returned no single commit');
  const remoteTip = remoteSha.toLowerCase();
  try {
    git(project, ['cat-file', '-e', `${remoteTip}^{commit}`]);
  } catch {
    git(project, ['fetch', '--no-tags', '--no-write-fetch-head', remote, branch]);
  }
  for (const commit of shippingCommits) {
    try {
      git(project, ['merge-base', '--is-ancestor', commit, remoteTip]);
    } catch {
      throw new Error(`Remote ${remote}/${branch} does not contain shipping commit ${commit}`);
    }
  }
  const path = integrationRecordPath(project, ticket, operation);
  const stable = {
    version: 1,
    ticket: identity,
    operation,
    remote,
    branch,
    source_commits: sourceCommits,
    shipping_commits: shippingCommits,
  };
  const retainSources = () => {
    // Keep source objects outside refs/heads before cleanup can see the ledger.
    for (const commit of sourceCommits)
      git(project, ['update-ref', `refs/hotsheet/integration-source/${commit}`, commit]);
  };
  if (existsSync(path)) {
    const old = JSON.parse(readFileSync(path, 'utf8'));
    const { remote_tip: _oldTip, recorded_at: _oldAt, ...oldStable } = old;
    if (JSON.stringify(oldStable) !== JSON.stringify(stable))
      throw new Error(
        'Integration operation already records a different mapping; review a correction under a new operation',
      );
    retainSources();
    return { path, record: old, replayed: true };
  }
  retainSources();
  const record = { ...stable, remote_tip: remoteTip, recorded_at: new Date().toISOString() };
  mkdirSync(dirname(path), { recursive: true });
  const temporary = `${path}.${randomUUID()}.tmp`;
  writeFileSync(temporary, `${JSON.stringify(record, null, 2)}\n`, { flag: 'wx', mode: 0o600 });
  try {
    linkSync(temporary, path);
  } catch (error) {
    if (error.code !== 'EEXIST') throw error;
    const old = JSON.parse(readFileSync(path, 'utf8'));
    const { remote_tip: _oldTip, recorded_at: _oldAt, ...oldStable } = old;
    if (JSON.stringify(oldStable) !== JSON.stringify(stable))
      throw new Error('A competing integration operation recorded a different mapping');
    return { path, record: old, replayed: true };
  } finally {
    unlinkSync(temporary);
  }
  return { path, record, replayed: false };
}

function parseArgs(args) {
  const options = { source: [], shipping: [] };
  for (let index = 0; index < args.length; index++) {
    const key = args[index];
    const value = args[++index];
    if (!value || value.startsWith('--')) throw new Error(`${key} needs a value`);
    if (key === '--source') options.source.push(value);
    else if (key === '--shipping') options.shipping.push(value);
    else if (['--project', '--ticket', '--operation', '--remote', '--branch'].includes(key)) {
      const name = key.slice(2);
      if (options[name]) throw new Error(`Duplicate ${key}`);
      options[name] = value;
    } else throw new Error(`Unsupported option ${key}`);
  }
  if (!options.project || !options.ticket || !options.operation)
    throw new Error(
      'Usage: integration-record.mjs --project <checkout> --ticket @project/connection:native --operation <id> --source <sha> --shipping <sha>',
    );
  return options;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const result = recordIntegration(parseArgs(process.argv.slice(2)));
    console.log(JSON.stringify(result));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
