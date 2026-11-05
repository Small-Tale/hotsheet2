#!/usr/bin/env node
// HS2-HAHX20: best-effort attribution of a red batch CI run to the batch's source tickets.
//
//   node scripts/ci-failure-attribution.mjs --manifest <batch.json> --store <ticket-store>
//        (--run <github-run-id> | --log <failed-log> [--earlier-log <file>]... [--report <file>]... --run-id <id>)
//        [--project <checkout>] [--dry-run] [--cli <hotsheet-cli>] [--actor-role <role> --actor-id <id>]
//
// It parses the failed CI output (scripts/ci-failure-parse.mjs), ranks the batch's tickets for each
// distinct failure, and files one follow-up bug per failure with typed follow-up links and a back-link
// note on each suspect. Every write is idempotent by run and failure identity, so a re-run reuses and
// repairs what an earlier run filed. A failure that cleared on re-run is flaky: it gets no attributed
// follow-up, only the flake bug docs/test-quarantine.md requires (reused across runs). `--dry-run`
// prints the proposals and writes nothing. docs/33-ci-failure-attribution.md describes the contract.
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, realpathSync } from 'node:fs';
import { join, posix, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { command } from './ci-completion-proposal.mjs';
import { combineAttempts, identity, parseLog, parseReport } from './ci-failure-parse.mjs';
import { exerciseIndexForRepo } from './gate-browser-exercise.mjs';

export const TAG = 'ci-failure-attribution';
export const FLAKY_TAG = 'flaky-test';
export const MARKER = 'ci-failure-attribution:';
/** Signal weights, strongest first. Confidence is a suspect's weight over max(1, the suspects' total). */
export const SIGNALS = {
  failure_file_changed: 0.9,
  imports_changed: 0.6,
  exercised_by_spec: 0.6,
  same_crate: 0.45,
  crate_dependency: 0.3,
  area_owner: 0.2,
  no_signal: 0.1,
};
const SHA = /^[0-9a-f]{40}$/i;
const SLUG = /\b([A-Z][A-Z0-9]*-[A-Z0-9]{6,})\b/;
const hash = (value) => createHash('sha256').update(value).digest('hex');

// ---------------------------------------------------------------------------------------------
// Batch manifest

/**
 * The batch manifest batch mode writes (HS2-9G0M3V). Changed files are optional per ticket; without
 * them they are read from the shipping commits.
 */
export function validateManifest(manifest) {
  if (manifest?.version !== 1) throw new Error('manifest version must be 1');
  if (typeof manifest.batch_id !== 'string' || !manifest.batch_id) throw new Error('manifest needs a batch_id');
  if (!Array.isArray(manifest.tickets) || !manifest.tickets.length) throw new Error('manifest needs at least one ticket');
  for (const [index, entry] of manifest.tickets.entries()) {
    const where = `manifest ticket ${index + 1}`;
    const { project_id: projectId, connection_id: connectionId, native_id: nativeId } = entry.ticket ?? {};
    if (!connectionId || !nativeId) throw new Error(`${where} needs a qualified ticket (connection_id, native_id)`);
    if (!projectId && !manifest.project_id) throw new Error(`${where} needs a project_id (or a manifest project_id)`);
    if (typeof entry.slug !== 'string' || !entry.slug) throw new Error(`${where} needs a slug`);
    for (const field of ['source_commits', 'shipping_commits'])
      if (!Array.isArray(entry[field]) || entry[field].some((sha) => !SHA.test(sha))) throw new Error(`${where} needs full ${field}`);
    if (entry.changed_files !== undefined && !Array.isArray(entry.changed_files)) throw new Error(`${where} changed_files must be a list`);
  }
  return manifest;
}

/** The files a batch ticket changed: declared, or the union over its shipping commits. */
export function changedFiles(entry, { project, run = command } = {}) {
  if (entry.changed_files) return [...new Set(entry.changed_files)].sort();
  const files = new Set();
  for (const sha of entry.shipping_commits) {
    const result = run('git', ['diff-tree', '--no-commit-id', '--name-only', '-r', '-m', '--root', sha], project);
    if (result.status !== 0) throw new Error(`cannot read the files of ${sha}: ${result.stderr?.trim() ?? ''}`);
    for (const file of result.stdout.split('\n')) if (file.trim()) files.add(file.trim());
  }
  return [...files].sort();
}

// ---------------------------------------------------------------------------------------------
// Repository view: file set, reads, the import graph, and the browser exercise index.

export function repoView(project, { run = command } = {}) {
  const listed = run('git', ['ls-files'], project);
  if (listed.status !== 0) throw new Error(`git ls-files failed in ${project}`);
  const files = new Set(listed.stdout.split('\n').filter(Boolean));
  const cache = new Map();
  return {
    files,
    read(path) {
      if (!cache.has(path)) {
        try { cache.set(path, readFileSync(join(project, path), 'utf8')); }
        catch { cache.set(path, null); }
      }
      return cache.get(path);
    },
    exercise: (changes) => exerciseIndexForRepo(project, changes),
  };
}

const PACKAGES = ['clients/web', 'migrator', 'spikes/kerf-webawesome'];

/** A repository path for a printed path: exact, under a package root, or the longest known suffix. */
export function resolvePath(token, files, prefixes = ['', ...PACKAGES]) {
  if (!token) return null;
  const clean = token.replace(/^file:\/\//, '').replace(/^\.\//, '').replace(/:\d+(?::\d+)?$/, '');
  for (const prefix of prefixes) {
    const candidate = prefix ? `${prefix}/${clean}` : clean;
    if (files.has(candidate)) return candidate;
  }
  if (clean.startsWith('/')) {
    let best = null;
    for (const file of files) if (clean.endsWith(`/${file}`) && (!best || file.length > best.length)) best = file;
    return best;
  }
  return null;
}

/** Candidate source files for a nextest test id (`<binary-id> <test path>`). */
export function nextestFiles(test, files) {
  const [binary = '', path = ''] = test.split(' ');
  const [crate, target] = binary.split('::');
  const root = `crates/${crate}`;
  const found = [];
  if (target && !target.startsWith('bin/')) {
    for (const candidate of [`${root}/tests/${target}.rs`, `${root}/tests/${target}/main.rs`]) if (files.has(candidate)) found.push(candidate);
    return found;
  }
  const modules = path.split('::').slice(0, -1);
  const base = target?.startsWith('bin/') ? `${root}/src/bin/${target.slice(4)}` : `${root}/src`;
  // The deepest module path that names a file wins; `tests` modules may be their own file.
  for (let depth = modules.length; depth > 0; depth -= 1) {
    const stem = `${base}/${modules.slice(0, depth).join('/')}`;
    const hit = [`${stem}.rs`, `${stem}/mod.rs`].find((candidate) => files.has(candidate));
    if (hit) return [hit];
  }
  const entry = [`${base}.rs`, `${base}/main.rs`, `${root}/src/lib.rs`, `${root}/src/main.rs`].find((candidate) => files.has(candidate));
  return entry ? [entry] : [];
}

/** Repository files a failure names, and the printed paths that match no repository file. */
export function failureFiles(failure, files) {
  const resolved = new Set();
  const unknown = [];
  const prefixes = (() => {
    if (failure.framework === 'playwright') return ['clients/web', ''];
    if (failure.framework === 'vitest' || failure.framework === 'tsc' || failure.framework === 'eslint' || failure.framework === 'prettier') {
      const hint = `${failure.job} ${failure.step}`;
      const preferred = PACKAGES.find((pkg) => new RegExp(pkg.split('/').at(-1).replace('kerf-webawesome', 'spike|kerf'), 'i').test(hint));
      return preferred ? [preferred, '', ...PACKAGES.filter((pkg) => pkg !== preferred)] : ['', ...PACKAGES];
    }
    return ['', ...PACKAGES];
  })();
  const tokens = [failure.file, ...(failure.locations ?? [])].filter(Boolean);
  for (const token of tokens) {
    const hit = resolvePath(token, files, prefixes);
    if (hit) resolved.add(hit);
    else unknown.push(token);
  }
  if (failure.framework === 'nextest' && failure.test) for (const file of nextestFiles(failure.test, files)) resolved.add(file);
  return { files: [...resolved].sort(), unknown };
}

export const crateOf = (path) => /^crates\/([^/]+)\//.exec(path ?? '')?.[1] ?? null;

/** The repository area that owns a path: a crate, a package, or a top-level directory. */
export function areaOf(path) {
  const crate = crateOf(path);
  if (crate) return `crates/${crate}`;
  const pkg = PACKAGES.find((root) => path === root || path.startsWith(`${root}/`));
  if (pkg) return pkg;
  return path.includes('/') ? path.split('/')[0] : '.';
}

/** The area a failure belongs to when it names no file: from the job and step names. */
export function jobAreas(failure) {
  const text = `${failure.job} ${failure.step}`;
  if (/migrator/i.test(text)) return ['migrator'];
  if (/spike|kerf.webawesome/i.test(text)) return ['spikes/kerf-webawesome'];
  if (/playwright|browser|chromium|webkit|web client|\bweb\b/i.test(text)) return ['clients/web'];
  if (/clippy|\bfmt\b|rust|cargo|nextest|\btest\b/i.test(text)) return ['crates'];
  return [];
}

/** Path crates each crate depends on (transitively), from `crates/<name>/Cargo.toml`. */
export function crateDependencies(repo) {
  const manifests = [...repo.files].map((file) => [/^crates\/([^/]+)\/Cargo\.toml$/.exec(file)?.[1], file]).filter(([crate]) => crate);
  const known = new Set(manifests.map(([crate]) => crate));
  const direct = new Map();
  for (const [crate, file] of manifests) {
    const text = repo.read(file) ?? '';
    const names = [...text.matchAll(/^([A-Za-z0-9_-]+?)(?:\.workspace)?\s*=/gm)].map((match) => match[1]);
    direct.set(crate, new Set(names.filter((name) => name !== crate && known.has(name))));
  }
  const closure = new Map();
  const walk = (crate, seen = new Set()) => {
    for (const dep of direct.get(crate) ?? []) if (!seen.has(dep)) { seen.add(dep); walk(dep, seen); }
    return seen;
  };
  for (const crate of direct.keys()) closure.set(crate, walk(crate));
  return closure;
}

// Static imports, re-exports and dynamic import(); type-only imports never run (as in HS2-N7EY18).
const IMPORT =
  /(?:^|[;\s])(?:import|export)\s+(?!type\s)(?:[\w*{}\s,$]+?\s+from\s+)?['"](\.{1,2}\/[^'"]+)['"]|\bimport\(\s*['"](\.{1,2}\/[^'"]+)['"]\s*\)/g;
const SCRIPT = /\.(?:[cm]?[jt]sx?)$/;
export const IMPORT_CLOSURE_LIMIT = 4000;

/** Repository files a script's relative runtime imports reach, transitively. */
export function importClosure(file, repo) {
  const seen = new Set();
  const queue = [file];
  while (queue.length && seen.size < IMPORT_CLOSURE_LIMIT) {
    const current = queue.shift();
    const text = repo.read(current);
    if (!text) continue;
    for (const match of text.matchAll(IMPORT)) {
      const base = posix.normalize(posix.join(posix.dirname(current), match[1] ?? match[2]));
      const candidates = [base, `${base}.ts`, `${base}.tsx`, `${base}.mjs`, `${base}.js`, base.replace(/\.js$/, '.ts'), base.replace(/\.js$/, '.tsx'),
        `${base}/index.ts`, `${base}/index.tsx`, `${base}/index.mjs`, `${base}/index.js`];
      const hit = candidates.find((candidate) => repo.files.has(candidate));
      if (hit && !seen.has(hit) && hit !== file) {
        seen.add(hit);
        if (SCRIPT.test(hit)) queue.push(hit);
      }
    }
  }
  return seen;
}

// ---------------------------------------------------------------------------------------------
// Ranking

const level = (confidence) => (confidence >= 60 ? 'high' : confidence >= 30 ? 'medium' : 'low');

/** The strongest signal tying one ticket's changes to one failure. */
function bestSignal(failure, located, changes, repo, context) {
  const changed = new Set(changes);
  const hit = located.files.find((file) => changed.has(file));
  if (hit) return { signal: 'failure_file_changed', detail: hit, reason: `changed ${hit}, which the failure names` };
  for (const file of located.files.filter((path) => SCRIPT.test(path))) {
    const reach = context.closure(file);
    const imported = changes.find((path) => reach.has(path));
    if (imported) return { signal: 'imports_changed', detail: imported, reason: `changed ${imported}, which ${file} imports` };
  }
  if (failure.framework === 'playwright' && located.files.length) {
    const spec = located.files[0].replace(/^clients\/web\//, '');
    const index = context.exercise(changes);
    const title = (failure.test ?? '').split(' › ').slice(1).join(' › ');
    const exercised = index.files.includes(spec)
      || (spec === 'tests/providers.spec.ts' && (index.providerWhole || index.providerTitles.some((entry) => title.includes(entry))))
      || index.demoTitles?.some((entry) => spec === 'tests/ux-demo.spec.ts' && title.includes(entry));
    if (exercised) {
      const source = changes.find((path) => path.startsWith('clients/web/src/')) ?? changes[0];
      return { signal: 'exercised_by_spec', detail: source, reason: `changed ${source}, which ${spec} exercises (HS2-N7EY18 index)` };
    }
  }
  const crates = new Set(located.files.map(crateOf).filter(Boolean));
  if (failure.framework === 'nextest' && failure.binary) crates.add(failure.binary.split('::')[0]);
  for (const crate of crates) {
    const same = changes.find((path) => crateOf(path) === crate);
    if (same) return { signal: 'same_crate', detail: same, reason: `changed ${same} in the failing crate ${crate}` };
  }
  for (const crate of crates) {
    const deps = context.crateDeps().get(crate) ?? new Set();
    const dep = changes.find((path) => deps.has(crateOf(path)));
    if (dep) return { signal: 'crate_dependency', detail: dep, reason: `changed ${dep} in ${crateOf(dep)}, a dependency of ${crate}` };
  }
  const areas = located.files.length ? [...new Set(located.files.map(areaOf))] : jobAreas(failure);
  const owned = changes.find((path) => areas.some((area) => areaOf(path) === area || (area === 'crates' && crateOf(path))));
  if (owned) return { signal: 'area_owner', detail: owned, reason: `touched the failure's area ${areas.join(', ')} (${owned})` };
  return { signal: 'no_signal', detail: null, reason: 'no change reaches the failure; equally suspect' };
}

/**
 * Ranked suspects for one failure. `tickets` are manifest entries with `changes` resolved; the
 * manifest order (batch order) breaks ties.
 */
export function rankFailure(failure, tickets, repo, context = rankingContext(repo)) {
  const located = failureFiles(failure, repo.files);
  const scored = tickets.map((entry, order) => {
    const best = bestSignal(failure, located, entry.changes, repo, context);
    return { entry, order, ...best, weight: SIGNALS[best.signal] };
  });
  const signalled = scored.filter((suspect) => suspect.signal !== 'no_signal');
  const pool = signalled.length ? signalled : scored;
  const total = pool.reduce((sum, suspect) => sum + suspect.weight, 0);
  const suspects = pool
    .sort((a, b) => b.weight - a.weight || a.order - b.order)
    .map((suspect, rank) => {
      const confidence = Math.round((100 * suspect.weight) / Math.max(1, total));
      return {
        rank: rank + 1,
        slug: suspect.entry.slug,
        ticket: suspect.entry.qualified,
        signal: suspect.signal,
        confidence,
        level: level(confidence),
        reason: suspect.reason,
      };
    });
  let reasoning;
  if (!signalled.length) reasoning = `No batch ticket's changes reach the failure's files, imports, crate, or area; all ${tickets.length} tickets are equally suspect.`;
  else {
    const [top] = suspects;
    const tied = suspects.filter((suspect) => suspect.signal === top.signal).map((suspect) => suspect.slug);
    reasoning = `${top.slug} ${top.reason}${tied.length > 1 ? `; tied on ${top.signal} with ${tied.slice(1).join(', ')}` : ''}.`;
  }
  return { files: located.files, unknownFiles: located.unknown, suspects, reasoning };
}

export function rankingContext(repo) {
  const closures = new Map();
  const exercises = new Map();
  let deps = null;
  return {
    closure(file) {
      if (!closures.has(file)) closures.set(file, importClosure(file, repo));
      return closures.get(file);
    },
    exercise(changes) {
      const key = changes.join('\n');
      if (!exercises.has(key)) exercises.set(key, repo.exercise(changes));
      return exercises.get(key);
    },
    crateDeps() {
      deps ??= crateDependencies(repo);
      return deps;
    },
  };
}

// ---------------------------------------------------------------------------------------------
// Proposals

/** The idempotency key of an attributed failure (run + identity) or a flaky test (identity only). */
export function failureKey(runId, failure) {
  return hash(`${runId}\n${identity(failure)}`).slice(0, 16);
}
export function flakyKey(failure) {
  return hash(`flaky\n${identity(failure)}`).slice(0, 16);
}

const code = (text) => `\`${String(text).replaceAll('`', "'")}\``;
const cell = (text) => String(text).replaceAll('|', '\\|');
const short = (text, limit = 90) => (text.length > limit ? `${text.slice(0, limit - 1)}…` : text);
const label = (failure) => (failure.kind === 'test' ? failure.test : `${failure.framework} in ${failure.job} / ${failure.step}`);
const runLine = (run) => `CI run ${run.url ? `[${run.id}](${run.url})` : code(run.id)}${run.attempt ? `, attempt ${run.attempt}` : ''}${run.headSha ? `, head ${code(run.headSha)}` : ''}`;

export function proposeFailure(failure, ranking, { batchId, run }) {
  const key = failureKey(run.id, failure);
  const lines = [
    'Batch CI failure, attributed best effort by `scripts/ci-failure-attribution.mjs` (HS2-HAHX20). Confirm the cause before acting.',
    '',
    `- **Failure:** ${code(identity(failure))}`,
    `- **Job:** ${code(failure.job || 'report')} / step ${code(failure.step || 'report')}${failure.projects?.length ? ` (${failure.projects.join(', ')})` : ''}`,
    `- **Files:** ${ranking.files.length ? ranking.files.map(code).join(', ') : 'none resolved'}${ranking.unknownFiles.length ? `; not in the repository: ${ranking.unknownFiles.map(code).join(', ')}` : ''}`,
    `- **Batch:** ${code(batchId)}; ${runLine(run)}`,
    `- **Excerpt:** ${code(failure.excerpt || label(failure))}`,
    '',
    '## Suspects',
    '',
    '| Rank | Ticket | Confidence | Signal |',
    '| --- | --- | --- | --- |',
    ...ranking.suspects.map((suspect) => `| ${suspect.rank} | ${suspect.slug} (${code(qualifiedRef(suspect.ticket))}) | ${suspect.confidence}% ${suspect.level} | ${cell(suspect.reason)} |`),
    '',
    `**Reasoning:** ${ranking.reasoning}`,
    '',
    `${MARKER} ${key}`,
  ];
  return {
    kind: 'failure',
    key,
    identity: identity(failure),
    title: short(`CI failure: ${short(label(failure), 80)} (batch ${batchId})`, 140),
    details: lines.join('\n'),
    tags: [TAG],
    suspects: ranking.suspects,
    files: ranking.files,
    unknownFiles: ranking.unknownFiles,
    reasoning: ranking.reasoning,
    failure: { framework: failure.framework, kind: failure.kind, job: failure.job, step: failure.step, test: failure.test, excerpt: failure.excerpt },
  };
}

export function proposeFlaky(failure, { batchId, run }) {
  const key = flakyKey(failure);
  return {
    kind: 'flaky',
    key,
    identity: identity(failure),
    title: short(`Flaky in CI: ${short(label(failure), 90)}`, 140),
    details: [
      'A CI failure that cleared on re-run (HS2-HAHX20). Per docs/test-quarantine.md a test that needed a retry gets a follow-up for its root cause; it is not attributed to batch tickets.',
      '',
      `- **Test:** ${code(identity(failure))}`,
      `- **First seen:** batch ${code(batchId)}; ${runLine(run)}`,
      `- **Why flaky:** ${failure.reason ?? 'reported flaky by the test runner'}`,
      '',
      // The run suffix lets a re-run of the same CI run recognize this occurrence as recorded.
      `${MARKER} ${key} run ${run.id}`,
    ].join('\n'),
    tags: [TAG, FLAKY_TAG],
    suspects: [],
    occurrence: `Flaky again in batch ${code(batchId)}; ${runLine(run)}.\n\n${MARKER} ${key} run ${run.id}`,
    failure: { framework: failure.framework, kind: failure.kind, job: failure.job, step: failure.step, test: failure.test, excerpt: failure.excerpt },
  };
}

export const qualifiedRef = (ticket) => `@${ticket.project_id}/${ticket.connection_id}:${ticket.native_id}`;

/** Parse, rank and propose for one batch: the pure core batch mode calls. */
export function attribute({ manifest, parsed, run, repo }) {
  validateManifest(manifest);
  const tickets = manifest.tickets.map((entry) => ({
    ...entry,
    qualified: { project_id: entry.ticket.project_id ?? manifest.project_id, connection_id: entry.ticket.connection_id, native_id: entry.ticket.native_id },
    changes: entry.changes ?? entry.changed_files ?? [],
  }));
  const context = rankingContext(repo);
  const seen = new Set();
  const failures = [];
  for (const failure of parsed.failures) {
    const id = identity(failure);
    if (seen.has(id)) continue;
    seen.add(id);
    failures.push(proposeFailure(failure, rankFailure(failure, tickets, repo, context), { batchId: manifest.batch_id, run }));
  }
  const flaky = [];
  for (const failure of parsed.flaky) {
    const id = identity(failure);
    if (seen.has(id)) continue;
    seen.add(id);
    flaky.push(proposeFlaky(failure, { batchId: manifest.batch_id, run }));
  }
  return { version: 1, batch_id: manifest.batch_id, run, proposals: [...failures, ...flaky] };
}

// ---------------------------------------------------------------------------------------------
// Filing through hotsheet-cli (provider-neutral: one Hot Sheet store; links route by qualified id)

export function storeConnection(store) {
  return hash(realpathSync(store)).slice(0, 16);
}

function cli(store, args, { run, actor, input }) {
  const full = ['-C', store, ...args, ...(actor.role ? ['--actor-role', actor.role] : []), ...(actor.id ? ['--actor-id', actor.id] : [])];
  const result = run('hotsheet-cli', full, store, input);
  if (result.status !== 0) throw new Error(`hotsheet-cli ${args[0]} failed: ${result.stderr?.trim() || result.stdout?.trim() || `exit ${result.status}`}`);
  return result.stdout ?? '';
}
const read = (store, args, run) => {
  const result = run('hotsheet-cli', ['-C', store, ...args], store);
  if (result.status !== 0) throw new Error(`hotsheet-cli ${args[0]} failed: ${result.stderr?.trim() || `exit ${result.status}`}`);
  return result.stdout ?? '';
};
const nativeId = (markdown) => /^id: (\S+)$/m.exec(markdown)?.[1] ?? null;

/** The existing follow-up for a key, found by tag and marker so a re-run never files twice. */
function findExisting(store, proposal, run) {
  const listed = read(store, ['ls', '--tag', TAG, '--text', `${MARKER} ${proposal.key}`], run);
  for (const line of listed.split('\n')) {
    const slug = SLUG.exec(line)?.[1];
    if (!slug) continue;
    const shown = read(store, ['show', slug], run);
    const body = shown.split('<!-- hotsheet:body:begin -->')[1]?.split('<!-- hotsheet:body:end -->')[0] ?? '';
    if (body.includes(`${MARKER} ${proposal.key}`)) return { slug, id: nativeId(shown), markdown: shown };
  }
  return null;
}

function backLink(proposal, followUpSlug, suspect) {
  return [
    '## CI failure suspect',
    '',
    `Follow-up ${followUpSlug} records the batch CI failure ${code(proposal.identity)}. This ticket is suspect ${suspect.rank} of ${proposal.suspects.length} at ${suspect.confidence}% (${suspect.level}): ${suspect.reason}. Best-effort attribution (HS2-HAHX20); confirm before acting.`,
    '',
    `${MARKER} ${proposal.key}`,
  ].join('\n');
}

/** File or reuse one proposal, then make sure every link and back-link note exists. */
export function fileProposal(proposal, { store, projectId, connection, run = command, actor = {} }) {
  const outcome = { key: proposal.key, kind: proposal.kind, title: proposal.title, links: [], notes: [], unsupported: [] };
  let existing = findExisting(store, proposal, run);
  if (existing) {
    outcome.action = 'existing';
    if (proposal.kind === 'flaky' && !existing.markdown.includes(proposal.occurrence.split('\n').at(-1))) {
      cli(store, ['edit', existing.slug, '--note-file', '-'], { run, actor, input: proposal.occurrence });
      outcome.notes.push(existing.slug);
    }
  } else {
    const args = ['new', '--title', proposal.title, '--category', 'bug', '--priority', 'high', '--details', proposal.details];
    for (const tag of proposal.tags) args.push('--tag', tag);
    const created = cli(store, args, { run, actor });
    const slug = SLUG.exec(created)?.[1];
    if (!slug) throw new Error(`hotsheet-cli new printed no ticket slug: ${created.trim()}`);
    const shown = read(store, ['show', slug], run);
    existing = { slug, id: nativeId(shown), markdown: shown };
    outcome.action = 'created';
  }
  outcome.slug = existing.slug;
  for (const suspect of proposal.suspects) {
    if (suspect.ticket.connection_id !== connection) {
      outcome.unsupported.push({ slug: suspect.slug, ticket: suspect.ticket, reason: `follow-up links and notes need the suspect's own connection ${suspect.ticket.connection_id}; this store is ${connection}` });
      continue;
    }
    const target = qualifiedRef({ project_id: projectId ?? suspect.ticket.project_id, connection_id: connection, native_id: existing.id });
    let shown = read(store, ['show', suspect.ticket.native_id], run);
    const frontmatter = /^---\n([\s\S]*?)\n---/.exec(shown)?.[1] ?? '';
    if (!frontmatter.includes(`native_id: ${existing.id}`)) {
      cli(store, ['link-follow-up', suspect.ticket.native_id, target], { run, actor });
      shown = read(store, ['show', suspect.ticket.native_id], run);
    }
    outcome.links.push(suspect.slug);
    const notes = shown.split('<!-- hotsheet:notes:begin -->')[1] ?? '';
    if (!notes.includes(`${MARKER} ${proposal.key}`)) {
      cli(store, ['edit', suspect.ticket.native_id, '--note-file', '-'], { run, actor, input: backLink(proposal, existing.slug, suspect) });
      outcome.notes.push(suspect.slug);
    }
  }
  return outcome;
}

export function fileAll(result, { store, projectId, run = command, actor = {} }) {
  const connection = storeConnection(store);
  return result.proposals.map((proposal) => fileProposal(proposal, { store, projectId, connection, run, actor }));
}

// ---------------------------------------------------------------------------------------------
// CI inputs

/** Parse the failed output of a GitHub Actions run through `gh`, comparing attempts for flakes. */
export function fetchRun(runId, { project, run = command } = {}) {
  const viewed = run('gh', ['run', 'view', String(runId), '--json', 'databaseId,attempt,conclusion,status,headSha,url'], project);
  if (viewed.status !== 0) throw new Error(`gh run view ${runId} failed: ${viewed.stderr?.trim() ?? ''}`);
  const info = JSON.parse(viewed.stdout);
  if (info.status !== 'completed') throw new Error(`run ${runId} is ${info.status}; attribute it after CI completes`);
  const attempts = Math.max(1, Number(info.attempt) || 1);
  const log = (attempt) => {
    const result = run('gh', ['run', 'view', String(runId), '--attempt', String(attempt), '--log-failed'], project);
    if (result.status !== 0) throw new Error(`gh run view ${runId} --attempt ${attempt} --log-failed failed: ${result.stderr?.trim() ?? ''}`);
    return parseLog(result.stdout);
  };
  const earlier = [];
  for (let attempt = 1; attempt < attempts; attempt += 1) earlier.push(log(attempt));
  const latest = info.conclusion === 'success' ? { failures: [], flaky: [] } : log(attempts);
  return {
    run: { id: String(info.databaseId ?? runId), url: info.url ?? null, attempt: attempts, headSha: info.headSha ?? null, conclusion: info.conclusion },
    parsed: combineAttempts(latest, earlier),
  };
}

/** Parse saved logs and reports (offline, or reports the CI uploaded). */
export function parseInputs({ log, earlierLogs = [], reports = [], readFile = (path) => readFileSync(path, 'utf8') }) {
  const latest = log ? parseLog(readFile(log)) : { failures: [], flaky: [] };
  for (const report of reports) {
    const parsed = parseReport(readFile(report));
    latest.failures.push(...parsed.failures);
    latest.flaky.push(...parsed.flaky);
  }
  return combineAttempts(latest, earlierLogs.map((path) => parseLog(readFile(path))));
}

// ---------------------------------------------------------------------------------------------
// CLI

const REPEATABLE = new Set(['earlier-log', 'report']);
const FLAGS = new Set(['dry-run']);

export function parseArgs(argv) {
  const options = { 'earlier-log': [], report: [] };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (!arg.startsWith('--')) throw new Error(`unexpected argument ${arg}`);
    const name = arg.slice(2);
    if (FLAGS.has(name)) { options[name] = true; continue; }
    const value = argv[index + 1];
    if (value === undefined || value.startsWith('--')) throw new Error(`${arg} needs a value`);
    index += 1;
    if (REPEATABLE.has(name)) options[name].push(value);
    else if (options[name] !== undefined) throw new Error(`duplicate ${arg}`);
    else options[name] = value;
  }
  if (!options.manifest) throw new Error('--manifest is required');
  if (!options.run && !options.log && !options.report.length) throw new Error('give --run <id>, or --log and/or --report');
  if (options.run && (options.log || options.report.length || options['earlier-log'].length)) throw new Error('--run reads logs from GitHub; do not combine it with --log, --earlier-log or --report');
  if (!options.run && !options['run-id']) throw new Error('--run-id is required with --log or --report (it keys idempotency)');
  if (!options['dry-run'] && !options.store) throw new Error('--store is required unless --dry-run');
  return options;
}

export function main(argv, { run: spawn = command, log = console.log, cwd = process.cwd(), env = process.env } = {}) {
  const options = parseArgs(argv);
  // An explicit binary avoids a stale `hotsheet-cli` earlier on PATH.
  const binary = options.cli ?? env.HOTSHEET_CLI ?? 'hotsheet-cli';
  const run = (program, args, where, input) => spawn(program === 'hotsheet-cli' ? binary : program, args, where, input);
  const project = resolve(cwd, options.project ?? '.');
  const manifest = validateManifest(JSON.parse(readFileSync(resolve(cwd, options.manifest), 'utf8')));
  const repo = repoView(project, { run });
  for (const entry of manifest.tickets) entry.changes = changedFiles(entry, { project, run });
  let input;
  if (options.run) input = fetchRun(options.run, { project, run });
  else {
    const at = (path) => resolve(cwd, path);
    input = {
      run: { id: String(options['run-id']), url: options['run-url'] ?? null, attempt: null, headSha: manifest.shipping_sha ?? null },
      parsed: parseInputs({ log: options.log && at(options.log), earlierLogs: options['earlier-log'].map(at), reports: options.report.map(at) }),
    };
  }
  const result = attribute({ manifest, parsed: input.parsed, run: input.run, repo });
  if (options['dry-run']) {
    result.dryRun = true;
    log(JSON.stringify(result, null, 2));
    return result;
  }
  const store = resolve(cwd, options.store);
  if (!existsSync(store)) throw new Error(`store ${store} does not exist`);
  result.filed = fileAll(result, { store, projectId: manifest.project_id, run, actor: { role: options['actor-role'], id: options['actor-id'] } });
  log(JSON.stringify(result, null, 2));
  return result;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  try { main(process.argv.slice(2)); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
