#!/usr/bin/env node
// HS2-1R2RGQ: bounded MSVC C runtime directive evidence for the LNK4098 MSVCRTD warnings that
// HS2-PN8D9F must fix. Diagnostic only: it changes no build, runtime or linker flag. It scans the
// cached native archive candidates, reports which archive member requests which C runtime,
// and fails explicitly when its tool or its inputs are missing rather than reporting a clean scan.

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, posix, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** The MSVC C runtime import and static libraries whose mixing produces LNK4098. */
export const CRT_LIBRARIES = ['MSVCRT', 'MSVCRTD', 'LIBCMT', 'LIBCMTD'];

const MEMBER = /^Archive member name at [0-9A-Fa-f]+: (.+)$/;
const DEFAULTLIB = /^\s*[/-]defaultlib:"?([^"\s]+)"?/i;
const RUNTIME = /^\s*[/-]failifmismatch:"?RuntimeLibrary=([^"\s]+)"?/i;

function libraryName(raw) {
  return raw.replace(/\.lib$/i, '').toUpperCase();
}

/**
 * Parse `dumpbin /ARCHIVEMEMBERS /DIRECTIVES` output into per-member default libraries and
 * runtime-library mismatch tags. A non-archive object reports one member named by its file.
 */
export function parseDirectives(text, artifact = '') {
  const members = [];
  let current;
  const ensure = (name) => {
    current = { member: name, defaultlibs: [], runtimeLibrary: [] };
    members.push(current);
    return current;
  };
  for (const line of text.split(/\r?\n/)) {
    const member = MEMBER.exec(line);
    if (member) {
      ensure(member[1].trim());
      continue;
    }
    const runtime = RUNTIME.exec(line);
    if (runtime) {
      (current ?? ensure(artifact)).runtimeLibrary.push(runtime[1]);
      continue;
    }
    const library = DEFAULTLIB.exec(line);
    if (library) (current ?? ensure(artifact)).defaultlibs.push(libraryName(library[1]));
  }
  return members.filter((member) => member.defaultlibs.length || member.runtimeLibrary.length);
}

/** Members that request any C runtime library, with only those libraries listed. */
export function crtRequests(members) {
  return members
    .map((member) => ({
      member: member.member,
      crt: [...new Set(member.defaultlibs.filter((library) => CRT_LIBRARIES.includes(library)))].sort(),
      runtimeLibrary: [...new Set(member.runtimeLibrary)].sort(),
    }))
    .filter((member) => member.crt.length || member.runtimeLibrary.length);
}

/**
 * Report a path relative to one of the known roots, else only its file name, so no machine-local
 * directory or user name enters the log or artifact.
 */
export function identity(path, roots) {
  // Archive members are named like `/0   C:\\build\\obj.obj`; drop the member-offset prefix.
  const normalized = path
    .replace(/^\/\d+\s+/, '')
    .trim()
    .replaceAll('\\', '/')
    .replace(/(.)\/+$/, '$1');
  for (const [label, root] of roots) {
    if (!root) continue;
    const base = root.replaceAll('\\', '/').replace(/\/+$/, '');
    if (normalized.toLowerCase() === base.toLowerCase()) return label;
    if (normalized.toLowerCase().startsWith(`${base.toLowerCase()}/`))
      return `${label}/${normalized.slice(base.length + 1)}`;
  }
  return basename(normalized);
}

/** Complete member spelling, never its privacy-reduced display basename (HS2-9E879K). */
export function memberIdentity(member) {
  const canonical = posix
    .normalize(
      member
        .replace(/^\/\d+\s+/, '')
        .trim()
        .replaceAll('\\', '/'),
    )
    .replace(/\/$/, '')
    .toLowerCase();
  return { version: 1, sha256: createHash('sha256').update(canonical).digest('hex') };
}

function walk(directory, accept, found = [], depth = 0) {
  if (depth > 8 || !existsSync(directory)) return found;
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) walk(path, accept, found, depth + 1);
    else if (accept(entry.name)) found.push(path);
  }
  return found;
}

/**
 * The bounded cached candidate set (not the current linked Cargo graph): build-script outputs, the selected vcpkg dav1d archives (release
 * and debug configurations of the one triplet the job installs), and native `-sys` crate rlibs.
 */
export function candidateArtifacts({ target, vcpkgRoot, triplet = 'x64-windows-static-md' }) {
  const artifacts = [];
  const build = join(target, 'debug', 'build');
  if (existsSync(build))
    for (const crate of readdirSync(build)) {
      const out = join(build, crate, 'out');
      for (const path of walk(out, (name) => name.toLowerCase().endsWith('.lib')))
        artifacts.push({ kind: 'build-script-lib', path });
    }
  if (vcpkgRoot)
    for (const configuration of ['lib', join('debug', 'lib')]) {
      const directory = join(vcpkgRoot, 'installed', triplet, configuration);
      if (!existsSync(directory)) continue;
      for (const name of readdirSync(directory))
        if (/^dav1d.*\.lib$/i.test(name))
          artifacts.push({
            kind: `vcpkg-${configuration.includes('debug') ? 'debug' : 'release'}`,
            path: join(directory, name),
          });
    }
  const deps = join(target, 'debug', 'deps');
  if (existsSync(deps))
    for (const name of readdirSync(deps))
      if (/^lib[a-z0-9_]+_sys-[0-9a-f]+\.rlib$/.test(name))
        artifacts.push({ kind: 'sys-rlib', path: join(deps, name) });
  return artifacts.sort((left, right) => left.path.localeCompare(right.path));
}

/** Summarize scan results: which artifacts and members request each C runtime. */
export function summarize(results) {
  const counts = Object.fromEntries(CRT_LIBRARIES.map((library) => [library, 0]));
  const requesters = {};
  for (const result of results)
    for (const member of result.requests ?? [])
      for (const library of member.crt) {
        counts[library] += 1;
        (requesters[library] ??= []).push({
          artifact: result.artifact,
          kind: result.kind,
          member: member.member,
          ...(member.memberIdentity ? { memberIdentity: member.memberIdentity } : {}),
          runtimeLibrary: member.runtimeLibrary,
        });
      }
  return {
    scanned: results.length,
    failed: results.filter((result) => result.error).length,
    counts,
    requesters,
  };
}

function run({ argv, env, dumpbin }) {
  const option = (name, fallback) => {
    const index = argv.indexOf(name);
    return index >= 0 && argv[index + 1] ? argv[index + 1] : fallback;
  };
  const repo = resolve(option('--repo', process.cwd()));
  const target = resolve(repo, option('--target', 'target'));
  const output = resolve(repo, option('--output', join('target', 'crt-directives', 'report.json')));
  const vcpkgRoot = env.VCPKG_INSTALLATION_ROOT;
  const roots = [
    ['target', target],
    ['vcpkg', vcpkgRoot],
    ['repo', repo],
  ];
  const started = performance.now();
  const fields = [
    ['sourceSha', 'GITHUB_SHA', /^[0-9a-f]{40}$/i],
    ['runId', 'GITHUB_RUN_ID', /^[0-9]+$/],
    ['runAttempt', 'GITHUB_RUN_ATTEMPT', /^[0-9]+$/],
  ];
  const provenance = { unavailable: [] };
  for (const [field, variable, pattern] of fields) {
    const value = env[variable];
    provenance[field] = typeof value === 'string' && pattern.test(value) ? value : null;
    if (provenance[field] === null) provenance.unavailable.push(field);
  }
  const report = {
    ticket: 'HS2-1R2RGQ',
    status: 'started',
    startedAt: new Date().toISOString(),
    provenance,
    selection: 'cached_artifact_candidates',
    currentLinkConfirmed: false,
    nextStep: 'Confirm candidate membership in a current affected link with LINK /VERBOSE:LIB (HS2-PN8D9F).',
    vcpkgTriplet: 'x64-windows-static-md',
    vcpkgRootAvailable: Boolean(vcpkgRoot),
    elapsedMs: 0,
    ...summarize([]),
    results: [],
  };
  const persist = () => {
    report.elapsedMs = Math.round(performance.now() - started);
    writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`);
  };
  // HS2-1R2RGQ: target is cached by CI. Remove only this output, then publish a fresh
  // started report before any probe; an early failure can never upload prior evidence.
  mkdirSync(dirname(output), { recursive: true });
  rmSync(output, { force: true });
  persist();
  const fail = (status, failure, message) => {
    report.status = 'failed';
    report.failure = failure;
    persist();
    return { status, report, message };
  };
  const probe = dumpbin(['/NOLOGO']);
  if (probe.error) {
    const code = /^[A-Z0-9_]+$/.test(probe.error.code ?? '') ? probe.error.code : 'execution_failed';
    return fail(2, 'dumpbin_unavailable', `dumpbin is unavailable (${code}); no CRT evidence was collected`);
  }
  const artifacts = candidateArtifacts({ target, vcpkgRoot });
  if (artifacts.length === 0)
    return fail(
      3,
      'no_candidates',
      `no native build artifacts were found under ${identity(target, roots)}; no CRT evidence was collected`,
    );
  const results = artifacts.map(({ kind, path }) => {
    const artifact = identity(path, roots);
    const scan = dumpbin(['/NOLOGO', '/ARCHIVEMEMBERS', '/DIRECTIVES', path]);
    if (scan.error || scan.status !== 0)
      return { artifact, kind, bytes: statSync(path).size, error: scan.error?.code ?? `exit ${scan.status}` };
    const requests = crtRequests(parseDirectives(scan.stdout, path)).map((request) => ({
      ...request,
      memberIdentity: memberIdentity(request.member),
      member: identity(request.member, roots),
    }));
    return { artifact, kind, bytes: statSync(path).size, requests };
  });
  const summary = summarize(results);
  Object.assign(report, summary, { results });
  if (summary.failed === summary.scanned)
    return fail(
      4,
      'all_candidates_unreadable',
      'dumpbin failed on every scanned artifact; no CRT evidence was collected',
    );
  report.status = 'completed';
  persist();
  return { status: 0, report, output: identity(output, roots) };
}

export function main({ argv = process.argv.slice(2), env = process.env, log = console.log, dumpbin } = {}) {
  const invoke =
    dumpbin ??
    ((args) => spawnSync('dumpbin', args, { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, windowsHide: true }));
  const result = run({ argv, env, dumpbin: invoke });
  if (result.status !== 0 && !result.report) {
    log(`HS2-1R2RGQ CRT directive scan failed: ${result.message}`);
    return result.status;
  }
  const { report } = result;
  log(
    `HS2-1R2RGQ scanned ${report.scanned} cached native candidates in ${report.elapsedMs} ms (${report.failed} unreadable)`,
  );
  log(
    `CRT default-library requests: ${CRT_LIBRARIES.map((library) => `${library}=${report.counts[library]}`).join(' ')}`,
  );
  for (const library of ['MSVCRTD', 'LIBCMTD']) {
    for (const requester of report.requesters[library] ?? [])
      log(
        `candidate ${library} <- ${requester.artifact} [${requester.kind}] member ${requester.member}${requester.runtimeLibrary.length ? ` RuntimeLibrary=${requester.runtimeLibrary.join(',')}` : ''}`,
      );
  }
  for (const failure of report.results.filter((entry) => entry.error))
    log(`unreadable: ${failure.artifact} [${failure.kind}] ${failure.error}`);
  if (result.status !== 0) {
    log(`HS2-1R2RGQ CRT directive scan failed: ${result.message}`);
    return result.status;
  }
  log(`Report: ${result.output}`);
  log(`Candidate scan only; current link is unconfirmed. ${report.nextStep}`);
  if (report.provenance.unavailable.length) log(`Unavailable provenance: ${report.provenance.unavailable.join(', ')}`);
  if (!report.counts.MSVCRTD)
    log(
      'No scanned candidate archive requests MSVCRTD; next native step: LINK /VERBOSE:LIB on one LNK4098 link (HS2-PN8D9F).',
    );
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = main();
