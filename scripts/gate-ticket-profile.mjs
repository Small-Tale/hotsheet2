// HS2-VQ5ND5: resolve local-gate profiles from normalized provider tickets.
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, realpathSync, statSync } from 'node:fs';
import { delimiter, isAbsolute, join, resolve } from 'node:path';
import { parseBudget } from './gate-budget.mjs';
import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';

const slugPattern = /\bHS2-[A-Z0-9]+\b/g;

/**
 * HS2-EBZ0CS: every hotsheet-cli call the gate and push scripts make runs with native secrets
 * denied and a bounded timeout. Finding the commit's ticket must never read the OS keychain (a
 * macOS keychain prompt would block an unattended push) or wait on a network provider.
 */
export const CLI_TIMEOUT_MS = 30_000;
export function cliEnv(inherited = process.env) {
  return { ...withoutGitRepositoryEnv(inherited), HOTSHEET_NATIVE_SECRETS: 'deny' };
}
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

// HS2-BF79YX: the shared settings file is the policy source for the local gate.
// An absent setting intentionally leaves the installed risk-tier behavior intact.
const presets = new Set(['ticket-assured', 'steady-main', 'risk-guided', 'ci-first']);
// HS2-ZGS7JP: CI First's best-effort local budget when the policy names none.
export const DEFAULT_BEST_EFFORT_BUDGET_MINUTES = 10;
const validBudgetMinutes = (minutes) => Number.isInteger(minutes) && minutes >= 1 && minutes <= 1440;
const customKeys = new Set([
  'integration',
  'local_gate',
  'minimum_tier',
  'budget_minutes',
  'browser',
  'platform',
  'perf',
  'test_plan_timing',
  'ci_gate',
  'staging_branch',
  'green_main_goal_minutes',
  'red_ci_recovery',
]);
const customName = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/;

function validateCustomDevelopmentStrategy(name, custom, settings) {
  if (!customName.test(name) || name.length > 40 || presets.has(name))
    throw new Error(`Invalid or reserved custom development strategy name ${name}`);
  if (
    !custom ||
    typeof custom !== 'object' ||
    Array.isArray(custom) ||
    Object.keys(custom).some((key) => !customKeys.has(key))
  )
    throw new Error(`Invalid custom development strategy ${name}`);
  if (!['serial', 'batch', 'ci-staged'].includes(custom.integration))
    throw new Error(`Invalid custom integration for ${name}`);
  if (!['full-local', 'targeted', 'risk-selected'].includes(custom.local_gate))
    throw new Error(`Invalid custom local gate for ${name}`);
  if (!['T0', 'T1', 'T2'].includes(custom.minimum_tier)) throw new Error(`Invalid custom minimum tier for ${name}`);
  if (
    custom.budget_minutes !== undefined &&
    (!Number.isInteger(custom.budget_minutes) || custom.budget_minutes < 1 || custom.budget_minutes > 1440)
  )
    throw new Error(`Invalid custom local budget for ${name}`);
  if ((custom.minimum_tier === 'T2' || custom.local_gate === 'full-local') && custom.budget_minutes !== undefined)
    throw new Error(`Custom strategy ${name} cannot budget a T2 or full local gate`);
  for (const key of ['browser', 'platform', 'perf'])
    if (custom[key] !== undefined && typeof custom[key] !== 'boolean')
      throw new Error(`Invalid ${key} minimum for ${name}`);
  if (!['before-work', 'before-handoff'].includes(custom.test_plan_timing))
    throw new Error(`Invalid test-plan timing for ${name}`);
  if (custom.ci_gate !== 'CI')
    throw new Error(`Custom strategy ${name} requires the connected CI gate; this checkout supports only CI`);
  if (
    custom.green_main_goal_minutes !== undefined &&
    (!Number.isInteger(custom.green_main_goal_minutes) ||
      custom.green_main_goal_minutes < 1 ||
      custom.green_main_goal_minutes > 10080)
  )
    throw new Error(`Invalid custom green-main goal for ${name}`);
  if (!['attribute-and-retry', 'fallback-to-serial'].includes(custom.red_ci_recovery))
    throw new Error(`Invalid red-CI recovery for ${name}`);
  if (custom.red_ci_recovery !== (custom.integration === 'serial' ? 'attribute-and-retry' : 'fallback-to-serial'))
    throw new Error(`Custom strategy ${name} red-CI recovery must match the integration circuit breaker`);
  if (custom.integration === 'ci-staged') {
    const allowed = settings.integration_strategies?.allowed?.find((item) => item.strategy === 'ci-staged');
    if (!allowed) throw new Error(`Custom strategy ${name} requires an allowed ci-staged integration strategy`);
    if (!custom.staging_branch || custom.staging_branch !== (allowed.staging_branch ?? 'integration'))
      throw new Error(`Custom strategy ${name} requires the allowed staging branch`);
  } else if (custom.staging_branch !== undefined) {
    throw new Error(`Only ci-staged custom strategy ${name} may name a staging branch`);
  }
  if (
    custom.integration === 'batch' &&
    !settings.integration_strategies?.allowed?.some((item) => item.strategy === 'batch')
  )
    throw new Error(`Custom strategy ${name} requires an allowed batch integration strategy`);
}

export function developmentStrategyFromSettings(settings, { session, ticket } = {}) {
  const policy = settings.development_strategy;
  if (policy === undefined && session === undefined && ticket === undefined) return null;
  if (policy !== undefined) {
    if (
      policy === null ||
      typeof policy !== 'object' ||
      Array.isArray(policy) ||
      Object.keys(policy).some(
        (key) =>
          ![
            'version',
            'default',
            'green_main_goal_minutes',
            'best_effort_budget_minutes',
            'default_custom',
            'custom',
          ].includes(key),
      ) ||
      ![1, 2].includes(policy.version) ||
      !presets.has(policy.default ?? 'ticket-assured')
    )
      throw new Error('Invalid versioned development_strategy policy');
    const goal = policy.green_main_goal_minutes ?? 360;
    if (!Number.isInteger(goal) || goal < 1 || goal > 10080)
      throw new Error('Invalid development_strategy green-main goal');
    if (policy.best_effort_budget_minutes !== undefined && !validBudgetMinutes(policy.best_effort_budget_minutes))
      throw new Error('Invalid development_strategy best-effort budget');
    if (policy.version === 1 && (policy.default_custom !== undefined || policy.custom !== undefined))
      throw new Error('Version 1 development_strategy cannot contain custom strategies');
    const definitions = policy.custom ?? {};
    if (
      !definitions ||
      typeof definitions !== 'object' ||
      Array.isArray(definitions) ||
      Object.keys(definitions).length > 20
    )
      throw new Error('Invalid custom development strategy map');
    for (const [name, definition] of Object.entries(definitions))
      validateCustomDevelopmentStrategy(name, definition, settings);
    if (
      policy.default_custom !== undefined &&
      (typeof policy.default_custom !== 'string' || !definitions[policy.default_custom])
    )
      throw new Error('Unknown default custom development strategy');
  }
  const choice = ticket ?? session ?? policy?.default_custom ?? policy?.default ?? 'ticket-assured';
  const custom = policy?.custom?.[choice];
  if (!presets.has(choice) && !custom) throw new Error(`Unknown development strategy ${choice}`);
  const integration =
    custom?.integration ?? (choice === 'ticket-assured' || choice === 'ci-first' ? 'serial' : 'ci-staged');
  if (
    integration === 'ci-staged' &&
    !settings.integration_strategies?.allowed?.some((entry) => entry.strategy === integration)
  )
    throw new Error(`Development strategy ${choice} conflicts with project integration policy`);
  return {
    preset: choice,
    source: ticket ? 'ticket' : session ? 'session' : 'project',
    integration,
    ...(custom ? { custom } : {}),
    ...(choice === 'ci-first'
      ? {
          local_gate: 'best-effort',
          local_budget_minutes: policy?.best_effort_budget_minutes ?? DEFAULT_BEST_EFFORT_BUDGET_MINUTES,
          push_without_receipt: true,
        }
      : {}),
  };
}

/**
 * HS2-ZGS7JP: CI First runs affected checks within one strict budget for every tier, T2
 * included. The gate defers to CI whatever does not fit, and never needs an analyzed T2
 * selection or a reviewed test plan. A ticket's own test-budget tag can only shorten it.
 */
export function bestEffortProfile(profile, selection) {
  const minutes = selection.local_budget_minutes ?? DEFAULT_BEST_EFFORT_BUDGET_MINUTES;
  if (!validBudgetMinutes(minutes)) throw new Error('HS2-ZGS7JP: invalid best-effort local budget.');
  const strategyMs = minutes * 60_000;
  return {
    ...profile,
    developmentStrategy: selection,
    bestEffort: true,
    budgetMs: Math.min(profile.budgetMs ?? strategyMs, strategyMs),
  };
}

export function withDevelopmentStrategy(profile, settings, session = process.env.HOTSHEET_DEVELOPMENT_STRATEGY) {
  const tags = profile.tags ?? [];
  const ticketTags = tags.filter((tag) => tag.startsWith('development-strategy:'));
  if (ticketTags.length > 1) throw new Error(`Conflicting development strategy ticket tags: ${ticketTags.join(', ')}`);
  const ticket = ticketTags[0]?.slice('development-strategy:'.length);
  const chosen = developmentStrategyFromSettings(settings, { session, ticket });
  if (!chosen) return profile;
  if (chosen.local_gate === 'best-effort') return bestEffortProfile(profile, chosen);
  const custom = chosen.custom;
  const tiers = ['T0', 'T1', 'T2'];
  const tier =
    custom && tiers.indexOf(custom.minimum_tier) > tiers.indexOf(profile.tier) ? custom.minimum_tier : profile.tier;
  if (tier === 'T2' && (profile.budgetMs !== undefined || custom?.budget_minutes !== undefined))
    throw new Error('T2 custom strategy cannot use a local time budget');
  const customBudgetMs = custom?.budget_minutes === undefined ? undefined : custom.budget_minutes * 60_000;
  // Ticket Assured deliberately opts into the full local gate. The unset
  // project does not: migration must not silently expand legacy verification.
  return {
    ...profile,
    developmentStrategy: chosen,
    tier,
    budgetMs:
      customBudgetMs === undefined ? profile.budgetMs : Math.min(profile.budgetMs ?? customBudgetMs, customBudgetMs),
    deep: profile.deep || (custom ? custom.local_gate === 'full-local' : chosen.preset === 'ticket-assured'),
    browser: profile.browser || custom?.browser === true,
    platform: profile.platform || custom?.platform === true,
    perf: profile.perf || custom?.perf === true,
  };
}

/** HS2-RRNZYN: the CLI is the authority for a configured strategy's precedence and policy. */
export function withResolvedDevelopmentStrategy(profile, selection) {
  if (
    !selection ||
    typeof selection.preset !== 'string' ||
    !['project', 'session', 'ticket'].includes(selection.source)
  )
    throw new Error('HS2-RRNZYN: development-strategy-resolve returned an invalid selection.');
  if (selection.local_gate === 'best-effort') return bestEffortProfile(profile, selection);
  const custom = selection.custom;
  const tiers = ['T0', 'T1', 'T2'];
  const tier =
    custom && tiers.indexOf(custom.minimum_tier) > tiers.indexOf(profile.tier) ? custom.minimum_tier : profile.tier;
  if (tier === 'T2' && (profile.budgetMs !== undefined || custom?.budget_minutes !== undefined))
    throw new Error('T2 custom strategy cannot use a local time budget');
  const customBudgetMs = custom?.budget_minutes === undefined ? undefined : custom.budget_minutes * 60_000;
  return {
    ...profile,
    developmentStrategy: selection,
    tier,
    budgetMs:
      customBudgetMs === undefined ? profile.budgetMs : Math.min(profile.budgetMs ?? customBudgetMs, customBudgetMs),
    deep: profile.deep || selection.local_gate === 'full-local',
    browser: profile.browser || custom?.browser === true,
    platform: profile.platform || custom?.platform === true,
    perf: profile.perf || custom?.perf === true,
  };
}

function realOrUndefined(path) {
  try {
    return realpathSync(path);
  } catch {
    return undefined;
  }
}

/**
 * HS2-Y183ZJ: the hotsheet-cli builds that may resolve a configured strategy, newest first: the
 * checkout's `target/debug` build and the first `hotsheet-cli` on PATH (the installed one). A
 * newer file may still be built from older code, so callers try each in turn.
 */
export function strategyCliCandidates(repo, inherited = process.env) {
  const name = process.platform === 'win32' ? 'hotsheet-cli.exe' : 'hotsheet-cli';
  const found = [];
  const debug = join(repo, 'target/debug', name);
  if (existsSync(debug)) found.push(debug);
  for (const dir of (inherited.PATH ?? '').split(delimiter).filter(Boolean)) {
    const candidate = join(dir, name);
    if (existsSync(candidate)) {
      found.push(candidate);
      break;
    }
  }
  const unique = [...new Map(found.map((path) => [realOrUndefined(path) ?? path, path])).values()];
  const mtime = (path) => {
    try {
      return statSync(path).mtimeMs;
    } catch {
      return 0;
    }
  };
  unique.sort((a, b) => mtime(b) - mtime(a));
  return unique.length ? unique : ['hotsheet-cli'];
}

export function resolveConfiguredDevelopmentStrategy(
  repo,
  profile,
  settings,
  run = execFileSync,
  candidates = strategyCliCandidates(repo),
) {
  if (settings.development_strategy === undefined) return withDevelopmentStrategy(profile, settings);
  const tags = profile.tags ?? [];
  const overrides = tags.filter((tag) => tag.startsWith('development-strategy:'));
  if (overrides.length > 1) throw new Error(`Conflicting development strategy ticket tags: ${overrides.join(', ')}`);
  // The CLI resolves either a locator file or a directory link from its cwd.
  // Passing the locator itself as -C would mistake it for a store directory.
  const args = ['coordination', '--project', repo, 'development-strategy-resolve'];
  const session = process.env.HOTSHEET_DEVELOPMENT_STRATEGY;
  if (session) args.push('--session', session);
  const ticket = overrides[0]?.slice('development-strategy:'.length);
  if (ticket) args.push('--ticket', ticket);
  const failures = [];
  for (const binary of candidates) {
    let stdout;
    try {
      stdout = run(binary, args, {
        cwd: repo,
        env: cliEnv(),
        encoding: 'utf8',
        maxBuffer: 1024 * 1024,
        timeout: CLI_TIMEOUT_MS,
        stdio: ['ignore', 'pipe', 'pipe'],
      });
    } catch (error) {
      failures.push(`${binary}: ${error.stderr?.toString().trim() || error.message}`);
      continue;
    }
    return withResolvedDevelopmentStrategy(profile, JSON.parse(stdout));
  }
  const requested = ticket ?? session ?? settings.development_strategy?.default ?? 'the project default';
  throw new Error(
    `HS2-RRNZYN: configured project strategy requires a current hotsheet-cli development-strategy-resolve. ` +
      `Rebuild hotsheet-cli: no candidate could resolve development strategy "${requested}" (HS2-Y183ZJ). ` +
      failures.join('; '),
  );
}

function projectSettings(repo) {
  const path = join(repo, '.hotsheet2/settings.json');
  return existsSync(path) ? JSON.parse(readFileSync(path, 'utf8')) : {};
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
  const listing = execFileSync('git', ['worktree', 'list', '--porcelain'], {
    cwd: repo,
    env: withoutGitRepositoryEnv(process.env),
    encoding: 'utf8',
  });
  const paths = [...listing.matchAll(/^worktree (.+)$/gm)].map((match) => resolve(match[1]));
  return paths.find((path) => existsSync(join(path, '.hotsheet2/store'))) ?? paths[0] ?? repo;
}

/**
 * HS2-EBZ0CS, HS2-J58Z4Y: the store directories this checkout links. `.hotsheet2/store` is either
 * the in-tree store directory or a locator file naming an external one.
 */
export function linkedStores(project) {
  const link = join(project, '.hotsheet2/store');
  let target = link;
  try {
    if (statSync(link).isFile()) {
      const named = readFileSync(link, 'utf8').trim();
      target = isAbsolute(named) ? named : resolve(project, named);
    }
  } catch {
    return [];
  }
  const real = realOrUndefined(target);
  return real ? [real] : [];
}

/**
 * HS2-EBZ0CS: the connections that can hold this checkout's ticket. Only local Git stores this
 * checkout links qualify, so the gate and push never call a credentialed or network provider (such
 * as another project's GitHub connection, HS2-7QPJ05) just to find the commit's ticket. When no
 * Git connection names a linked store, every local Git connection is a candidate; never a remote
 * one.
 */
export function linkedGitConnections(connections, stores) {
  const git = connections.filter((connection) => connection.provider === 'git');
  const linked = git.filter((connection) => {
    const real = typeof connection.locator === 'string' ? realOrUndefined(connection.locator) : undefined;
    return real !== undefined && stores.includes(real);
  });
  return (linked.length ? linked : git).map((connection) => connection.connection_id);
}

export function readTicketProfile(
  repo,
  { ticket: explicit, workerId = process.env.HOTSHEET_WORKER_ID, dirty = false } = {},
) {
  const project = projectTicketRoot(repo);
  const selected = (profile) => {
    const settings = projectSettings(repo);
    return resolveConfiguredDevelopmentStrategy(repo, profile, settings);
  };
  if (!existsSync(join(project, '.hotsheet2/store'))) {
    if (explicit) throw new Error(`--ticket ${explicit} cannot be resolved: no linked ticket source.`);
    return selected(defaultProfile('ticket source unavailable; default T1 budget'));
  }
  const run = (...args) =>
    execFileSync('hotsheet-cli', args, {
      cwd: project,
      env: cliEnv(),
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
      timeout: CLI_TIMEOUT_MS,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
  if (explicit?.includes(':')) {
    const split = explicit.indexOf(':');
    const ticket = JSON.parse(run('provider-get', explicit.slice(0, split), explicit.slice(split + 1)));
    const profile = profileFromTags(ticket);
    return selected({ ...profile, reason: `explicit --ticket; ${profile.reason}` });
  }
  let descriptors;
  try {
    descriptors = JSON.parse(run('providers', '--linked', '--json'));
  } catch (error) {
    // Older installed CLIs and legacy projects with only a store link still use the Git-only scope below.
    if (
      !/unexpected argument '--linked'|no registered checkout contains the current directory/u.test(error.stderr ?? '')
    )
      throw error;
    descriptors = JSON.parse(run('providers', '--json'));
  }
  const connections = linkedGitConnections(descriptors, linkedStores(project));
  // A Git store that cannot be listed cannot hold this checkout's ticket; skip it (HS2-E5EX0M).
  const tickets = connections.flatMap((connection) => {
    try {
      return JSON.parse(run('provider-ls', connection));
    } catch {
      return [];
    }
  });
  const subject = execFileSync('git', ['log', '-1', '--format=%s'], {
    cwd: repo,
    env: withoutGitRepositoryEnv(process.env),
    encoding: 'utf8',
  }).trim();
  const chosen = chooseTicket({ explicit, subject, dirty, workerId, tickets });
  if (!chosen.ticket) return selected(defaultProfile(chosen.reason));
  const ticket = JSON.parse(run('provider-get', chosen.ticket.connection_id, chosen.ticket.native_id));
  const profile = profileFromTags(ticket);
  return selected({ ...profile, reason: `${chosen.reason}; ${profile.reason}` });
}
