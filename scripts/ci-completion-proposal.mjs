#!/usr/bin/env node
// HS2-1C1565: explicit, read-only evidence collection for an integrator's CI handoff.
// `register` records the integrator-reviewed source-to-main mapping once. `refresh`
// rechecks it and writes a deduplicated local proposal; neither changes a ticket or pool.

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isCovered } from './ci-gate.mjs';

const SHA = /^[0-9a-f]{40}$/i;
const OPERATION = /^[a-zA-Z0-9][a-zA-Z0-9._-]*$/;
const hash = (value) => createHash('sha256').update(value).digest('hex');

export function command(binary, args, cwd, input) {
  const result = spawnSync(binary, args, { cwd, encoding: 'utf8', input });
  if (result.error) throw result.error;
  return result;
}

function checked(binary, args, cwd, run) {
  const result = run(binary, args, cwd);
  if (result.status !== 0) throw new Error(result.stderr?.trim() || `${binary} ${args.join(' ')} failed`);
  return result.stdout.trim();
}

function git(project, args, run) {
  return checked('git', args, project, run);
}

function ticket(store, slug, run) {
  return checked('hotsheet-cli', ['-C', store, 'show', slug], store, run);
}

function storeConnection(store) {
  try { return hash(realpathSync(store)).slice(0, 16); }
  catch { return null; }
}

function coordination(project, store, run) {
  return JSON.parse(checked('hotsheet-cli', ['-C', store, 'coordination', '--project', project, 'status'], project, run));
}

export function parseTicket(markdown) {
  const frontmatter = /^---\n([\s\S]*?)\n---/.exec(markdown)?.[1];
  if (!frontmatter) throw new Error('ticket output has no frontmatter');
  const field = (name) => new RegExp(`^${name}: (.*)$`, 'm').exec(frontmatter)?.[1];
  const followUps = [];
  const lines = frontmatter.split('\n');
  const start = lines.indexOf('follow_ups:');
  let block = '';
  if (start >= 0) {
    for (const line of lines.slice(start + 1)) {
      if (!line.startsWith(' ') && !line.startsWith('- ')) break;
      block += `${line}\n`;
    }
  }
  for (const match of block.matchAll(/project_id: (\S+)[\s\S]*?connection_id: (\S+)[\s\S]*?native_id: (\S+)/g)) {
    followUps.push(`@${match[1]}/${match[2]}:${match[3]}`);
  }
  const notes = [...markdown.matchAll(/<!-- hotsheet:note:begin (\S+) ([^>]*?)-->\n?([\s\S]*?)<!-- hotsheet:note:end -->/g)];
  const reviewFindings = notes
    .filter(([, , , body]) => /^## Review return\b/m.test(body))
    .map(([, id]) => id);
  return {
    id: field('id'),
    slug: field('slug'),
    status: field('status'),
    updatedAt: field('updated_at'),
    completedAt: field('completed_at'),
    revision: hash(markdown),
    scope: hash(markdown.split('<!-- hotsheet:body:begin -->')[1]?.split('<!-- hotsheet:body:end -->')[0] ?? ''),
    followUps,
    reviewFindings,
    noteIds: notes.map(([, id]) => id),
  };
}

export function classifyRuns(runs, integrated, covers) {
  if (!Array.isArray(runs)) return { state: 'unavailable', runs: [], limitation: 'gh run list unavailable; run refresh again when GitHub is reachable' };
  const mainRuns = runs.filter((run) => run.workflowName === 'CI' && run.headBranch === 'main' && run.event === 'push' && SHA.test(run.headSha) && Number.isInteger(run.databaseId));
  const qualifying = runs
    .filter((run) => mainRuns.includes(run))
    .map((run) => {
      let coverage;
      try { coverage = covers(integrated, run.headSha); } catch { coverage = 'unknown'; }
      return { ...run, coverage };
    })
    .filter((run) => run.coverage !== 'uncovered')
    .sort((a, b) => Date.parse(a.createdAt) - Date.parse(b.createdAt));
  const evidence = qualifying.map(({ databaseId, headSha, status, conclusion, coverage, url, createdAt }) => ({
    id: databaseId, headSha, status, conclusion, coverage, url, createdAt,
  }));
  if (qualifying.some((run) => run.coverage === 'covered' && run.status === 'completed' && run.conclusion === 'success')) {
    return { state: 'success', runs: evidence };
  }
  if (qualifying.some((run) => run.coverage === 'unknown')) {
    return { state: 'unavailable', runs: evidence, limitation: 'fetch origin/main so local Git can prove CI run ancestry' };
  }
  const latest = qualifying.at(-1);
  if (!latest) return { state: 'unavailable', runs: [], limitation: 'no covering main push CI run in the returned window; inspect GitHub Actions and refresh explicitly' };
  if (latest.status !== 'completed') return { state: 'pending', runs: evidence };
  if (latest.conclusion === 'cancelled') {
    const superseded = mainRuns.some((run) => Date.parse(run.createdAt) > Date.parse(latest.createdAt));
    return { state: superseded ? 'superseded' : 'cancelled', runs: evidence };
  }
  return { state: 'failure', runs: evidence };
}

function worktree(project, path, expected, run) {
  if (!path) return { state: 'none' };
  try {
    if (realpathSync(path) === realpathSync(project)) return { state: 'main_checkout', path: resolve(path) };
  } catch { return { state: 'missing', path: resolve(path) }; }
  const list = git(project, ['worktree', 'list', '--porcelain'], run);
  const canonical = (value) => { try { return realpathSync(value); } catch { return null; } };
  const expectedPath = canonical(path);
  if (!expectedPath) return { state: 'missing', path: resolve(path) };
  const found = list.split('\n\n').find((entry) => {
    const listed = entry.split('\n')[0]?.replace(/^worktree /, '');
    return listed && canonical(listed) === expectedPath;
  });
  if (!found) return { state: 'missing', path: resolve(path) };
  let head, dirty;
  try {
    head = git(path, ['rev-parse', 'HEAD'], run);
    dirty = git(path, ['status', '--porcelain'], run).length > 0;
  } catch { return { state: 'unavailable', path: resolve(path) }; }
  const branch = /^branch (.+)$/m.exec(found)?.[1] ?? null;
  return { state: head === expected && !dirty ? 'clean' : 'changed', path: resolve(path), branch, head, dirty };
}

function mapping(project, source, integrated, run) {
  if (source === integrated) return 'identity';
  let sourceDiff, mainDiff;
  try {
    sourceDiff = git(project, ['show', '--format=', source], run);
    mainDiff = git(project, ['show', '--format=', integrated], run);
  } catch { return 'unverified'; }
  const left = run('git', ['patch-id', '--stable'], project, sourceDiff);
  const right = run('git', ['patch-id', '--stable'], project, mainDiff);
  const leftId = left.stdout.split(' ')[0], rightId = right.stdout.split(' ')[0];
  if (left.status === 0 && right.status === 0 && SHA.test(leftId) && leftId === rightId) return 'stable_patch_id';
  return 'unverified';
}

export function evaluate(record, evidence) {
  const reasons = [];
  if (evidence.assignment?.id !== record.operation || evidence.assignment?.ticket?.connection_id !== record.ticket.connection_id || evidence.assignment?.ticket?.native_id !== record.ticket.native_id) reasons.push('assignment identity changed');
  if (evidence.assignment?.generation !== record.workerGeneration) reasons.push('worker generation changed');
  if (!record.integrator || evidence.integrator?.id !== record.integrator.id || evidence.integrator?.generation !== record.integrator.generation) reasons.push('integrator session changed or is unavailable; review and register the proposal again');
  if (!evidence.integratorLive) reasons.push('integrator session is not live');
  if (evidence.assignment?.phase === 'blocked' || evidence.assignment?.recovery_note) reasons.push('returned or blocked operation is not completion-ready');
  if (record.kind === 'code' && evidence.assignment?.phase !== 'ci') reasons.push('code assignment has not reached verified post-push CI phase');
  if (record.kind === 'findings' && evidence.assignment?.phase !== 'verifying') reasons.push('findings assignment is not in review verification');
  if (evidence.ticket.revision !== record.ticketRevision) reasons.push('ticket changed since registration; review current scope, notes and follow-ups');
  if (evidence.ticket.unavailable) reasons.push('ticket provider evidence is unavailable; obtain the authoritative ticket before completion');
  if (!evidence.ticketIdentityMatches) reasons.push('selected store does not own the assignment ticket identity');
  if (!['started', 'completed'].includes(evidence.ticket.status)) reasons.push('ticket is not open for completion');
  if (evidence.activeClaims?.length) reasons.push('ticket has an active claim; inspect its owner before completion');
  if (evidence.outstandingReview.length) reasons.push(`unresolved review notes: ${evidence.outstandingReview.join(', ')}`);
  if (record.kind === 'code') {
    if (evidence.assignment?.handoff?.kind !== 'code' || evidence.assignment.handoff.commit !== record.sourceCommit) reasons.push('source handoff changed');
    if (evidence.integrated !== record.integrated || evidence.tree !== record.tree) reasons.push('integrated main commit or tree changed');
    if (evidence.mapping === 'unverified') reasons.push('source-to-main mapping is not independently verified');
    if (evidence.mainBranch !== 'main' || !evidence.mainContains) reasons.push('current main HEAD does not contain the integrated commit');
    if (evidence.remoteAncestry !== 'covered') reasons.push(`remote main ancestry is ${evidence.remoteAncestry}; query origin and fetch main if the remote commit is absent locally`);
    if (!evidence.gateReceipt) reasons.push('exact integrated tree has no successful local root gate receipt');
    if (evidence.ci.state !== 'success') reasons.push(`covering main CI is ${evidence.ci.state}`);
    if (evidence.sourceWorktree.state !== 'clean') reasons.push('source worktree is changed or missing; cleanup is not ready');
  } else if (evidence.assignment?.handoff?.kind !== 'findings' || evidence.assignment.handoff.note_id !== record.findingsNote) {
    reasons.push('findings handoff changed');
  } else if (!evidence.ticket.noteIds.includes(record.findingsNote)) {
    reasons.push('findings handoff note is unavailable from the authoritative ticket');
  }
  return { ready: reasons.length === 0, reasons };
}

export function collect(record, { run = command, runs } = {}) {
  const project = resolve(record.project), store = resolve(record.store);
  const snapshot = coordination(project, store, run);
  const assignment = snapshot.state.assignments[record.operation];
  const sourceMatches = storeConnection(store) === record.ticket.connection_id;
  let currentTicket;
  try {
    if (!sourceMatches) throw new Error('selected store is not the assignment source');
    currentTicket = parseTicket(ticket(store, record.slug, run));
  } catch {
    currentTicket = { slug: record.slug, status: 'unavailable', revision: null, scope: null, followUps: [], reviewFindings: [], noteIds: [], unavailable: true };
  }
  const outstandingReview = currentTicket.reviewFindings.filter((id) => !record.resolvedReviewNotes.includes(id));
  const activeClaims = (snapshot.live_claims ?? []).filter(({ ticket }) => ticket.connection_id === record.ticket.connection_id && ticket.native_id === record.ticket.native_id);
  const ticketIdentityMatches = sourceMatches && currentTicket.id === record.ticket.native_id && currentTicket.slug === record.slug;
  const evidence = { assignment, integrator: snapshot.state.integrator, integratorLive: snapshot.live_integrator, ticket: currentTicket, ticketIdentityMatches, outstandingReview, activeClaims };
  if (record.kind === 'code') {
    evidence.integrated = git(project, ['rev-parse', `${record.integrated}^{commit}`], run);
    evidence.tree = git(project, ['rev-parse', `${record.integrated}^{tree}`], run);
    evidence.mapping = mapping(project, record.sourceCommit, record.integrated, run);
    evidence.mainHead = git(project, ['rev-parse', 'HEAD'], run);
    evidence.mainBranch = git(project, ['symbolic-ref', '--short', 'HEAD'], run);
    evidence.mainContains = run('git', ['merge-base', '--is-ancestor', record.integrated, 'HEAD'], project).status === 0;
    const remote = run('git', ['ls-remote', '--exit-code', 'origin', 'refs/heads/main'], project);
    const remoteMatch = remote.status === 0 ? /^([0-9a-f]{40})\s+refs\/heads\/main\s*$/i.exec(remote.stdout) : null;
    evidence.remoteHead = remoteMatch?.[1] ?? null;
    try {
      evidence.remoteAncestry = remoteMatch
        ? isCovered(record.integrated, remoteMatch[1], (binary, args) => run(binary, args, project))
        : 'unavailable';
    } catch { evidence.remoteAncestry = 'unavailable'; }
    const gatePath = git(project, ['rev-parse', '--git-path', 'hotsheet-root-gate'], run);
    const receipt = join(resolve(project, gatePath), evidence.tree);
    evidence.gateReceiptPath = receipt;
    try { evidence.gateReceipt = existsSync(receipt) && readFileSync(receipt, 'utf8').trim() === evidence.tree; }
    catch { evidence.gateReceipt = false; }
    evidence.sourceWorktree = worktree(project, record.sourceWorktree, record.sourceCommit, run);
    let listed = runs;
    if (listed === undefined) {
      const result = run('gh', ['run', 'list', '--branch', 'main', '--workflow', 'CI', '--event', 'push', '--limit', '100', '--json', 'databaseId,headSha,headBranch,event,workflowName,status,conclusion,createdAt,url'], project);
      try { listed = result.status === 0 ? JSON.parse(result.stdout) : null; } catch { listed = null; }
    }
    evidence.ci = classifyRuns(listed, record.integrated, (commit, head) => isCovered(commit, head, (binary, args) => run(binary, args, project)));
  } else {
    evidence.ci = { state: 'not_required', runs: [] };
    evidence.sourceWorktree = { state: 'none' };
  }
  const assessment = evaluate(record, evidence);
  const transition = (kind) => assignment?.transitions?.find((event) => event.kind === kind)?.at;
  const handoffAt = transition('handoff');
  const ciAt = transition('ci');
  const duration = (end) => Number.isFinite(Date.parse(handoffAt)) && Number.isFinite(Date.parse(end)) && Date.parse(end) >= Date.parse(handoffAt)
    ? Date.parse(end) - Date.parse(handoffAt) : null;
  const factors = record.kind === 'code' ? {
    sourceMapping: evidence.mapping, localGate: evidence.gateReceipt ? 'passed_exact_tree' : 'missing',
    remoteAncestry: evidence.remoteAncestry, mainCi: evidence.ci.state,
    ticketRevision: currentTicket.revision === record.ticketRevision ? 'current' : 'changed',
    reviewFindings: outstandingReview.length, sourceWorktree: evidence.sourceWorktree.state,
  } : {
    findingsNote: currentTicket.noteIds.includes(record.findingsNote) ? 'present' : 'unavailable',
    ticketRevision: currentTicket.revision === record.ticketRevision ? 'current' : 'changed',
    reviewFindings: outstandingReview.length,
  };
  return {
    version: 1, operation: record.operation, qualifiedTicket: record.ticket, slug: record.slug,
    source: record.kind === 'code' ? { kind: 'code', commit: record.sourceCommit } : { kind: 'findings', noteId: record.findingsNote },
    integrated: record.kind === 'code' ? { commit: record.integrated, tree: evidence.tree, mapping: evidence.mapping, mainHead: evidence.mainHead, mainBranch: evidence.mainBranch, mainContains: evidence.mainContains, remoteHead: evidence.remoteHead, remoteAncestry: evidence.remoteAncestry, gateReceipt: evidence.gateReceipt, gateReceiptPath: evidence.gateReceiptPath } : null,
    ticket: { id: currentTicket.id, status: currentTicket.status, identityMatches: ticketIdentityMatches, revision: currentTicket.revision, scopeHash: currentTicket.scope, scopeCommand: `hotsheet-cli -C ${store} show ${record.slug}`, followUps: currentTicket.followUps, outstandingReview, activeClaims },
    assignment: { phase: assignment?.phase, generation: assignment?.generation },
    ci: evidence.ci, sourceWorktree: evidence.sourceWorktree, confidenceFactors: factors, ...assessment,
    measurements: { handoffAt: handoffAt ?? null, handoffToCiMs: duration(ciAt), handoffToCompletionMs: duration(currentTicket.completedAt), baselineProductionDelta: 'unknown' },
    cleanup: assessment.ready && record.kind === 'code' ? { worktree: evidence.sourceWorktree.path, branch: evidence.sourceWorktree.branch } : null,
    next: assessment.ready ? (record.kind === 'code' ? `Integrator: refresh again immediately before completing ${record.slug} and marking ${record.operation} done; remove the listed clean worktree after verified resolution` : `Integrator: refresh again immediately before completing ${record.slug} and marking ${record.operation} done`) : 'Resolve reasons and refresh explicitly; no ticket or pool state was changed',
  };
}

function options(args) {
  const parsed = {};
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i], value = args[i + 1];
    if (!key?.startsWith('--') || !value) throw new Error(`invalid option ${key ?? ''}`);
    const name = key.slice(2);
    if (name === 'resolved-review-note') (parsed.resolvedReviewNotes ??= []).push(value);
    else if (parsed[name] !== undefined) throw new Error(`duplicate ${key}`);
    else parsed[name] = value;
  }
  return parsed;
}

function proposalPath(project, operation, run) {
  if (!OPERATION.test(operation)) throw new Error('operation must be a safe identifier');
  const common = git(project, ['rev-parse', '--git-common-dir'], run);
  return join(resolve(project, common), 'hotsheet-ci-proposals', `${operation}.json`);
}

export function register(opts, run = command) {
  const project = resolve(opts.project), store = resolve(opts.store);
  const snapshot = coordination(project, store, run);
  const assignment = snapshot.state.assignments[opts.assignment];
  if (!assignment) throw new Error('assignment not found in coordination state');
  if (assignment.phase === 'blocked' || assignment.recovery_note) throw new Error('returned or blocked assignment cannot be registered');
  let currentTicket;
  try {
    if (storeConnection(store) !== assignment.ticket.connection_id) throw new Error('selected store is not the assignment source');
    currentTicket = parseTicket(ticket(store, assignment.slug, run));
    if (currentTicket.id !== assignment.ticket.native_id || currentTicket.slug !== assignment.slug) throw new Error('ticket id does not match assignment');
  }
  catch { currentTicket = { revision: null, reviewFindings: [], unavailable: true }; }
  const kind = assignment.handoff?.kind;
  if (!['code', 'findings'].includes(kind)) throw new Error('assignment has no durable handoff');
  if (kind === 'code' && !SHA.test(assignment.handoff.commit)) throw new Error('code handoff has no full source commit SHA');
  if (kind === 'code' && (!SHA.test(opts.integrated) || !opts.worktree)) throw new Error('code handoff requires --integrated full SHA and --worktree');
  if (kind === 'code' && realpathSync(opts.worktree) === realpathSync(project)) throw new Error('source worktree must not be the main checkout');
  if (kind === 'findings' && (opts.integrated || opts.worktree)) throw new Error('findings handoff has no code gate or worktree');
  const record = {
    version: 1, project, store, operation: assignment.id, ticket: assignment.ticket, slug: assignment.slug,
    workerGeneration: assignment.generation, integrator: snapshot.state.integrator ? { id: snapshot.state.integrator.id, generation: snapshot.state.integrator.generation } : null,
    ticketRevision: currentTicket.revision, kind,
    sourceCommit: kind === 'code' ? assignment.handoff.commit : undefined,
    findingsNote: kind === 'findings' ? assignment.handoff.note_id : undefined,
    integrated: kind === 'code' ? opts.integrated.toLowerCase() : undefined,
    tree: kind === 'code' ? git(project, ['rev-parse', `${opts.integrated}^{tree}`], run) : undefined,
    sourceWorktree: kind === 'code' ? resolve(opts.worktree) : undefined,
    resolvedReviewNotes: opts.resolvedReviewNotes ?? [],
  };
  if (record.resolvedReviewNotes.some((id) => !currentTicket.reviewFindings.includes(id))) throw new Error('resolved review note is not a current Review return note');
  const path = proposalPath(project, assignment.id, run);
  mkdirSync(resolve(path, '..'), { recursive: true });
  writeFileSync(path, `${JSON.stringify({ record }, null, 2)}\n`);
  return path;
}

export function refresh(path, { run = command, runs } = {}) {
  const file = JSON.parse(readFileSync(path, 'utf8'));
  if (file.record?.version !== 1) throw new Error('unsupported proposal record');
  const proposal = collect(file.record, { run, runs });
  const next = { record: file.record, proposal };
  const serialized = `${JSON.stringify(next, null, 2)}\n`;
  if (!existsSync(path) || readFileSync(path, 'utf8') !== serialized) writeFileSync(path, serialized);
  return proposal;
}

function main(args) {
  const action = args.shift();
  if (!['register', 'refresh'].includes(action)) throw new Error('usage: ci-completion-proposal.mjs <register|refresh> --project <checkout> --store <store> --assignment <operation> [--integrated <full-sha> --worktree <path>]');
  const opts = options(args);
  if (!opts.project || !opts.assignment || (action === 'register' && !opts.store)) throw new Error('project, assignment and (for register) store are required');
  const path = action === 'register' ? register(opts) : proposalPath(resolve(opts.project), opts.assignment, command);
  const proposal = refresh(path);
  console.log(JSON.stringify({ path, proposal }, null, 2));
  if (!proposal.ready) process.exitCode = 2;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  try { main(process.argv.slice(2)); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
