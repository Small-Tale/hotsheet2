// HS2-VQ5ND5: resolve local-gate profiles from normalized provider tickets.
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { parseBudget } from './gate-budget.mjs';

const slugPattern = /\bHS2-[A-Z0-9]+\b/g;
const defaultProfile = (reason) => ({
  tier: 'T1',
  tags: [],
  ticket: null,
  reason,
  deep: false,
  browser: false,
  platform: false,
  perf: false,
  budgetMs: undefined,
});

export function profileFromTags(ticket) {
  const tags = ticket.tags ?? [];
  const tiers = tags.filter((tag) => tag.startsWith('tier:'));
  if (tiers.some((tag) => !['tier:T0', 'tier:T1', 'tier:T2'].includes(tag)))
    throw new Error(`Unknown risk tier tag on ${ticket.qualified_id}: ${tiers.join(', ')}`);
  if (new Set(tiers).size > 1)
    throw new Error(`Conflicting risk tier tags on ${ticket.qualified_id}: ${tiers.join(', ')}`);
  const unknown = tags.filter(
    (tag) => tag.startsWith('testing:') && !['testing:browser', 'testing:platform', 'testing:perf'].includes(tag),
  );
  if (unknown.length) throw new Error(`Unknown testing tag on ${ticket.qualified_id}: ${unknown.join(', ')}`);
  const budgetTags = tags.filter((tag) => tag.startsWith('test-budget:'));
  if (budgetTags.length > 1)
    throw new Error(`Conflicting test-budget tags on ${ticket.qualified_id}: ${budgetTags.join(', ')}`);
  const tier = tiers[0]?.slice(5) ?? 'T1';
  if (tier === 'T2' && budgetTags.length) throw new Error(`T2 has no time budget: ${ticket.qualified_id}`);
  const budgetMs = budgetTags.length ? parseBudget(budgetTags[0].slice('test-budget:'.length)) : undefined;
  return {
    tier,
    tags,
    ticket: ticket.qualified_id,
    reason: tiers.length ? `tier from ${tiers[0]}` : 'tier tag missing; default T1 budget',
    deep: tags.includes('deep-testing'),
    browser: tags.includes('testing:browser'),
    platform: tags.includes('testing:platform'),
    perf: tags.includes('testing:perf'),
    budgetMs,
  };
}

export function chooseTicket({ explicit, subject = '', dirty = false, workerId, tickets }) {
  const byRef = (ref) => tickets.filter((ticket) => ticket.qualified_id === ref || ticket.slug === ref);
  if (explicit) {
    const found = byRef(explicit);
    if (found.length !== 1)
      throw new Error(
        `--ticket ${explicit} resolved to ${found.length} provider tickets; use connection_id:native_id.`,
      );
    return { ticket: found[0], reason: 'explicit --ticket' };
  }
  if (!dirty) {
    const slugs = [...new Set(subject.match(slugPattern) ?? [])];
    if (slugs.length) {
      if (slugs.length > 1)
        return { ticket: null, reason: `commit subject names ${slugs.length} tickets; default profile` };
      const found = tickets.filter((ticket) => slugs.includes(ticket.slug));
      if (found.length === 1) return { ticket: found[0], reason: 'HEAD commit subject' };
      return { ticket: null, reason: `commit subject resolved ${found.length} tickets; default profile` };
    }
  }
  if (workerId) {
    const found = tickets.filter((ticket) => ticket.claimed_by === workerId);
    if (found.length === 1) return { ticket: found[0], reason: 'worker live claim' };
    return { ticket: null, reason: `worker has ${found.length} live claims; default profile` };
  }
  return { ticket: null, reason: 'no ticket reference or worker claim; default profile' };
}

export function projectTicketRoot(repo) {
  const listing = execFileSync('git', ['worktree', 'list', '--porcelain'], { cwd: repo, encoding: 'utf8' });
  const paths = [...listing.matchAll(/^worktree (.+)$/gm)].map((match) => resolve(match[1]));
  return paths.find((path) => existsSync(join(path, '.hotsheet2/store'))) ?? paths[0] ?? repo;
}

export function readTicketProfile(
  repo,
  { ticket: explicit, workerId = process.env.HOTSHEET_WORKER_ID, dirty = false } = {},
) {
  const project = projectTicketRoot(repo);
  if (!existsSync(join(project, '.hotsheet2/store'))) {
    if (explicit) throw new Error(`--ticket ${explicit} cannot be resolved: no linked ticket source.`);
    return defaultProfile('ticket source unavailable; default T1 budget');
  }
  const run = (...args) =>
    execFileSync('hotsheet-cli', args, { cwd: project, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (explicit?.includes(':')) {
    const split = explicit.indexOf(':');
    const ticket = JSON.parse(run('provider-get', explicit.slice(0, split), explicit.slice(split + 1)));
    const profile = profileFromTags(ticket);
    return { ...profile, reason: `explicit --ticket; ${profile.reason}` };
  }
  const providers = run('providers')
    .trim()
    .split('\n')
    .filter(Boolean)
    .map((line) => line.trim().split(/\s+/)[0]);
  const tickets = providers.flatMap((connection) => JSON.parse(run('provider-ls', connection)));
  const subject = execFileSync('git', ['log', '-1', '--format=%s'], { cwd: repo, encoding: 'utf8' }).trim();
  const chosen = chooseTicket({ explicit, subject, dirty, workerId, tickets });
  if (!chosen.ticket) return defaultProfile(chosen.reason);
  const ticket = JSON.parse(run('provider-get', chosen.ticket.connection_id, chosen.ticket.native_id));
  const profile = profileFromTags(ticket);
  return { ...profile, reason: `${chosen.reason}; ${profile.reason}` };
}
