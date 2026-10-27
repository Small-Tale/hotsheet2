#!/usr/bin/env node
// HS2-9E879K: correlate the HS2-1R2RGQ cached MSVCRTD candidates with one current affected link.
// Relinks exactly one owned test target with the linker's /VERBOSE output (which includes the
// /VERBOSE:LIB library search plus the archive members it selects), reads that output from rustc's
// linker_messages diagnostics, and reports which candidate members this link actually loaded beside
// its LNK4098 emissions. Diagnostic only: no CRT, linker, suppression or warning-budget change.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, posix, resolve } from 'node:path';
import { performance } from 'node:perf_hooks';
import { fileURLToPath } from 'node:url';

import { identity, memberIdentity } from './windows-crt-directives.mjs';

/** Bounds on every list kept in the report, so a verbose link can never produce an unbounded artifact. */
export const LIMITS = { searched: 200, archives: 200, defaultlibs: 100, matches: 500 };

const SEARCHING = /^\s*Searching (.+?):\s*$/;
const LOADED = /^\s*Loaded (.+?)\((.+)\)\s*$/;
const PROCESSED = /^\s*Processed \/DEFAULTLIB:(\S+)\s*$/i;
const LNK4098 = /\bLNK4098: defaultlib '([^']+)' conflicts/i;

/** The exact relink of one owned test target; only its own fingerprint changes. */
export function linkCommand({ packageName, testName }) {
  return [
    'rustc',
    '-p',
    packageName,
    '--test',
    testName,
    '--message-format=json',
    '--',
    '-C',
    'link-arg=/VERBOSE',
    '-W',
    'linker_messages',
  ];
}

/** Linker text from Cargo's JSON stream: rustc reports the linker's stdout/stderr as diagnostics. */
function cargoEvents(jsonLines) {
  const events = [];
  for (const line of jsonLines.split('\n')) {
    if (!line.startsWith('{')) continue;
    let event;
    try {
      event = JSON.parse(line);
    } catch {
      continue;
    }
    if (event && typeof event === 'object') events.push(event);
  }
  return events;
}

function diagnosticText(event) {
  if (event.reason !== 'compiler-message') return '';
  const message = event.message ?? {};
  const texts = [
    message.message,
    ...(Array.isArray(message.children) ? message.children : []).map((child) => child?.message),
  ];
  return texts.some(
    (text) => typeof text === 'string' && /\blinker (stdout|stderr)\b|\bLNK\d{4}\b|link\.exe/.test(text),
  )
    ? texts.filter((text) => typeof text === 'string').join('\n')
    : '';
}

export function linkerText(jsonLines) {
  return cargoEvents(jsonLines).map(diagnosticText).filter(Boolean).join('\n');
}

/** Cargo package ids are opaque: resolve the requested target from Cargo's own metadata. */
export function requestedTarget(json, link) {
  let metadata;
  try {
    metadata = JSON.parse(json);
  } catch {
    return undefined;
  }
  const packages = Array.isArray(metadata?.packages)
    ? metadata.packages.filter((item) => item?.name === link.packageName)
    : [];
  if (packages.length !== 1 || typeof packages[0].id !== 'string' || !packages[0].id) return undefined;
  const targets = Array.isArray(packages[0].targets)
    ? packages[0].targets.filter(
        (item) =>
          item?.name === link.testName &&
          Array.isArray(item.kind) &&
          item.kind.includes('test') &&
          typeof item.src_path === 'string' &&
          item.src_path,
      )
    : [];
  return targets.length === 1 ? { packageId: packages[0].id, target: targets[0] } : undefined;
}

/** Never join separate compiler diagnostics into a fictitious link (HS2-9E879K). */
function currentLinkEvidence(jsonLines, expected) {
  const events = cargoEvents(jsonLines);
  const owned = (event) =>
    event.package_id === expected.packageId &&
    event.target?.name === expected.target.name &&
    Array.isArray(event.target.kind) &&
    event.target.kind.length === 1 &&
    event.target.kind[0] === 'test' &&
    typeof event.target.src_path === 'string' &&
    normalized(event.target.src_path) === normalized(expected.target.src_path);
  const artifacts = events.filter((event) => event.reason === 'compiler-artifact' && owned(event));
  const groups = events.filter(owned).map(diagnosticText).filter(Boolean).map(parseVerboseLink);
  return {
    fresh: artifacts.length === 1 && artifacts[0].fresh === false,
    artifacts: artifacts.length,
    diagnostics: groups.length,
    groups,
  };
}

const memberName = (value) => value.replaceAll('\\', '/').split('/').at(-1).toLowerCase();
const archiveName = (value) => value.replaceAll('\\', '/').split('/').at(-1).toLowerCase();
const normalized = (value) => posix.normalize(value.replaceAll('\\', '/')).toLowerCase();
// identity() deliberately drops unknown roots to a basename. That privacy fallback cannot prove
// archive identity, even if both sanitized names happen to agree (HS2-9E879K review return).
const qualifiedArchive = (value) => /^(target|vcpkg|repo)\/.+/.test(normalized(value));
const exactMember = (value) =>
  value?.version === 1 && typeof value.sha256 === 'string' && /^[a-f0-9]{64}$/.test(value.sha256);

/** Libraries searched, members loaded, default libraries processed, and LNK4098 emissions. */
export function parseVerboseLink(text) {
  const searched = [],
    loaded = [],
    defaultlibs = [],
    lnk4098 = {};
  // `Loaded` lines sit under the `Searching <path>:` block that selected them and may name the archive
  // without its directory; attribute such a bare name to that searched path.
  let current;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.replace(/^linker (stdout|stderr):\s*/, '');
    let match;
    if ((match = SEARCHING.exec(line))) {
      current = match[1].trim();
      searched.push(current);
    } else if ((match = LOADED.exec(line))) {
      const archive = match[1].trim();
      const bare = !/[\\/]/.test(archive);
      loaded.push({
        archive: bare && current && archiveName(current) === archive.toLowerCase() ? current : archive,
        member: match[2].trim(),
      });
    } else if ((match = PROCESSED.exec(line))) defaultlibs.push(match[1].replace(/\.lib$/i, '').toUpperCase());
    if ((match = LNK4098.exec(line))) {
      const library = match[1].replace(/\.lib$/i, '').toUpperCase();
      lnk4098[library] = (lnk4098[library] ?? 0) + 1;
    }
  }
  return { searched, loaded, defaultlibs, lnk4098 };
}

/**
 * Join selected members with directive candidates. An `exact` match has the same normalized archive
 * and member identity; a `basename` match agrees only on archive and member file names, which is
 * weaker and reported separately.
 */
export function correlate(candidates, loaded) {
  const exact = new Map(),
    names = new Map();
  for (const candidate of candidates) {
    const key = `${normalized(candidate.artifact)}|${candidate.memberIdentity?.sha256}`;
    if (qualifiedArchive(candidate.artifact) && exactMember(candidate.memberIdentity))
      exact.set(key, [...(exact.get(key) ?? []), candidate]);
    const name = `${archiveName(candidate.artifact)}|${memberName(candidate.member)}`;
    names.set(name, [...(names.get(name) ?? []), candidate]);
  }
  const matches = [],
    seen = new Set();
  for (const entry of loaded) {
    const key = `${normalized(entry.archive)}|${entry.memberIdentity?.sha256 ?? normalized(entry.member)}`;
    if (seen.has(key)) continue;
    const direct = qualifiedArchive(entry.archive) && exactMember(entry.memberIdentity) ? exact.get(key) : undefined;
    const byName = direct ?? names.get(`${archiveName(entry.archive)}|${memberName(entry.member)}`) ?? [];
    if (byName.length === 0) continue;
    seen.add(key);
    // Several cached archives can share a file name (dav1d release and debug); a name-only match
    // to more than one candidate cannot say which archive this link loaded.
    for (const candidate of byName)
      matches.push({
        library: candidate.library,
        artifact: candidate.artifact,
        kind: candidate.kind,
        member: candidate.member,
        ...(direct ? { memberIdentity: { version: 1, sha256: candidate.memberIdentity.sha256 } } : {}),
        loadedArchive: entry.archive,
        match: direct ? 'exact' : byName.length === 1 ? 'basename' : 'ambiguous',
      });
  }
  return matches;
}

/** MSVCRTD (and other CRT) requesters recorded by the HS2-1R2RGQ directive report. */
export function directiveCandidates(report) {
  const candidates = [];
  for (const [library, requesters] of Object.entries(report.requesters ?? {}))
    for (const requester of requesters)
      if (typeof requester.artifact === 'string' && typeof requester.member === 'string')
        candidates.push({
          library,
          artifact: requester.artifact,
          kind: requester.kind,
          member: requester.member,
          ...(exactMember(requester.memberIdentity)
            ? { memberIdentity: { version: 1, sha256: requester.memberIdentity.sha256 } }
            : {}),
        });
  return candidates;
}

function validatedProvenance(values) {
  const provenance = { unavailable: [] };
  for (const [field, pattern] of [
    ['sourceSha', /^[0-9a-f]{40}$/i],
    ['runId', /^[0-9]+$/],
    ['runAttempt', /^[0-9]+$/],
  ]) {
    const value = values?.[field];
    provenance[field] = typeof value === 'string' && pattern.test(value) ? value : null;
    if (provenance[field] === null) provenance.unavailable.push(field);
  }
  return provenance;
}

function provenanceOf(env) {
  return validatedProvenance({
    sourceSha: env.GITHUB_SHA,
    runId: env.GITHUB_RUN_ID,
    runAttempt: env.GITHUB_RUN_ATTEMPT,
  });
}

const bounded = (values, limit) => ({ total: values.length, kept: values.slice(0, limit) });

export function run({ argv, env, cargo }) {
  const option = (name, fallback) => {
    const index = argv.indexOf(name);
    return index >= 0 && argv[index + 1] ? argv[index + 1] : fallback;
  };
  const repo = resolve(option('--repo', process.cwd()));
  const target = resolve(repo, option('--target', 'target'));
  const directivesPath = resolve(repo, option('--directives', join('target', 'crt-directives', 'report.json')));
  const output = resolve(repo, option('--output', join('target', 'crt-directives', 'link-correlation.json')));
  const link = {
    packageName: option('--package', 'hotsheet-ticketing'),
    testName: option('--test', 'file_lock_process'),
  };
  const roots = [
    ['target', target],
    ['vcpkg', env.VCPKG_INSTALLATION_ROOT],
    ['repo', repo],
  ];
  const started = performance.now();
  const report = {
    ticket: 'HS2-9E879K',
    status: 'started',
    startedAt: new Date().toISOString(),
    provenance: provenanceOf(env),
    directiveProvenance: null,
    link: { package: link.packageName, test: link.testName, verbose: '/VERBOSE' },
    currentLinkConfirmed: false,
    elapsedMs: 0,
  };
  const persist = () => {
    report.elapsedMs = Math.round(performance.now() - started);
    writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`);
  };
  // Like HS2-1R2RGQ: target/ is cached, so remove only this output and publish a fresh started
  // report before relinking; an early failure can never upload an earlier run's evidence.
  mkdirSync(dirname(output), { recursive: true });
  rmSync(output, { force: true });
  persist();
  const fail = (status, failure, message) => {
    report.status = 'failed';
    report.failure = failure;
    persist();
    return { status, report, message };
  };
  if (!existsSync(directivesPath))
    return fail(2, 'directive_report_missing', 'the HS2-1R2RGQ directive report is missing; nothing to correlate');
  let directives;
  try {
    directives = JSON.parse(readFileSync(directivesPath, 'utf8'));
  } catch {
    return fail(2, 'directive_report_unreadable', 'the HS2-1R2RGQ directive report is not valid JSON');
  }
  const directiveProvenance = validatedProvenance(directives?.provenance);
  report.directiveProvenance = { status: directives?.status ?? null, ...directiveProvenance };
  report.directiveSourceMatches =
    Boolean(report.provenance.sourceSha) &&
    directiveProvenance.sourceSha?.toLowerCase() === report.provenance.sourceSha.toLowerCase();
  if (directives?.status !== 'completed')
    return fail(2, 'directive_report_incomplete', 'the HS2-1R2RGQ directive report did not complete in this run');
  report.directiveProvenanceMatches =
    report.provenance.unavailable.length === 0 &&
    directiveProvenance.unavailable.length === 0 &&
    Array.isArray(directives.provenance?.unavailable) &&
    directives.provenance.unavailable.length === 0 &&
    ['sourceSha', 'runId', 'runAttempt'].every(
      (field) => directiveProvenance[field].toLowerCase() === report.provenance[field].toLowerCase(),
    );
  if (!report.directiveProvenanceMatches)
    return fail(
      2,
      'directive_provenance_mismatch',
      'directive evidence must have the same validated source SHA, run and attempt',
    );
  const candidates = directiveCandidates(directives);
  const metadata = cargo(['metadata', '--no-deps', '--format-version=1', '--offline']);
  if (metadata.error || metadata.status !== 0)
    return fail(3, 'cargo_metadata_unavailable', 'Cargo metadata could not establish the requested package/test');
  const expected = requestedTarget(metadata.stdout ?? '', link);
  if (!expected)
    return fail(3, 'cargo_target_unavailable', 'Cargo metadata did not uniquely identify the requested package/test');
  const result = cargo(linkCommand(link));
  if (result.error) return fail(3, 'cargo_unavailable', 'cargo could not be started; no link evidence was collected');
  report.cargoExit = result.status;
  const parsed = parseVerboseLink(linkerText(result.stdout ?? ''));
  if (parsed.searched.length === 0 && parsed.loaded.length === 0)
    return fail(
      4,
      'no_verbose_linker_output',
      `cargo exited ${result.status} without verbose linker output; the target may not have relinked`,
    );
  const normalizedRoots = roots.map(([label, root]) => [label, root && posix.normalize(root.replaceAll('\\', '/'))]);
  const selectedMembers = (entries) =>
    entries.map((entry) => ({
      archive: identity(posix.normalize(entry.archive.replaceAll('\\', '/')), normalizedRoots),
      memberIdentity: memberIdentity(entry.member),
      member: identity(entry.member, roots),
    }));
  const loaded = selectedMembers(parsed.loaded);
  const archives = {};
  for (const entry of loaded) archives[entry.archive] = (archives[entry.archive] ?? 0) + 1;
  const matches = correlate(candidates, loaded);
  const current = currentLinkEvidence(result.stdout ?? '', expected);
  const confirmingGroups = current.groups
    .map((group) => ({
      warning: (group.lnk4098.MSVCRTD ?? 0) > 0,
      matches: correlate(candidates, selectedMembers(group.loaded)).filter(
        (match) => match.library === 'MSVCRTD' && match.match === 'exact',
      ),
    }))
    .filter((group) => group.warning && group.matches.length > 0);
  const confirmed = report.directiveProvenanceMatches && current.fresh && confirmingGroups.length === 1;
  Object.assign(report, {
    lnk4098: parsed.lnk4098,
    defaultlibsProcessed: bounded([...new Set(parsed.defaultlibs)].sort(), LIMITS.defaultlibs),
    librariesSearched: bounded([...new Set(parsed.searched.map((path) => identity(path, roots)))], LIMITS.searched),
    loadedMembers: loaded.length,
    loadedByArchive: bounded(
      Object.entries(archives)
        .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
        .map(([archive, members]) => ({ archive, members })),
      LIMITS.archives,
    ),
    candidates: candidates.length,
    candidateMatches: bounded(matches, LIMITS.matches),
    currentLinkMatches: bounded(confirmed ? confirmingGroups[0].matches : [], LIMITS.matches),
    currentTarget: {
      freshArtifact: current.fresh,
      artifacts: current.artifacts,
      linkDiagnostics: current.diagnostics,
      confirmingDiagnostics: confirmingGroups.length,
    },
    // Confirmed only when this very link both warns about MSVCRTD and loads an MSVCRTD requester.
    currentLinkConfirmed: confirmed,
    note: 'Cargo-wide candidateMatches are advisory; currentLinkMatches require fresh same-target, same-diagnostic evidence. Not a CRT fix or warning-free result (HS2-PN8D9F).',
  });
  report.status = result.status === 0 ? 'completed' : 'completed_link_failed';
  persist();
  return { status: 0, report, output: identity(output, roots) };
}

export function main({ argv = process.argv.slice(2), env = process.env, log = console.log, cargo } = {}) {
  const invoke =
    cargo ??
    ((args) => spawnSync('cargo', args, { encoding: 'utf8', maxBuffer: 512 * 1024 * 1024, windowsHide: true, env }));
  const result = run({ argv, env, cargo: invoke });
  const { report } = result;
  if (result.status !== 0) {
    log(`HS2-9E879K link correlation failed: ${result.message}`);
    return result.status;
  }
  log(
    `HS2-9E879K inspected ${report.link.package} --test ${report.link.test} (cargo exit ${report.cargoExit}; fresh target ${report.currentTarget.freshArtifact}) in ${report.elapsedMs} ms`,
  );
  log(
    `LNK4098: ${
      Object.entries(report.lnk4098)
        .map(([library, count]) => `${library}=${count}`)
        .join(' ') || 'none'
    }; loaded members ${report.loadedMembers}; candidates ${report.candidates}`,
  );
  for (const match of report.candidateMatches.kept)
    log(`advisory loaded ${match.library} candidate ${match.artifact} member ${match.member} [${match.match}]`);
  for (const match of report.currentLinkMatches.kept)
    log(`current link ${match.library} candidate ${match.artifact} member ${match.member} [${match.match}]`);
  log(
    `currentLinkConfirmed=${report.currentLinkConfirmed}; directive provenance matches: ${report.directiveProvenanceMatches}`,
  );
  if (report.provenance.unavailable.length) log(`Unavailable provenance: ${report.provenance.unavailable.join(', ')}`);
  log(`Report: ${result.output}`);
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = main();
