// HS2-TCCKRZ: bounded, machine-local observations for planning a local gate budget.
import { createHash, randomUUID } from 'node:crypto';
import { hostname, arch, platform } from 'node:os';
import {
  closeSync,
  existsSync,
  mkdirSync,
  openSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { dirname, isAbsolute, relative, resolve } from 'node:path';
import { nextestTimings, playwrightTimings, nextestOutcomes, vitestOutcomes, playwrightOutcomes } from './test-profile.mjs';

const WINDOW = 20;
const empty = () => ({ version: 1, stages: {}, files: {}, imports: {}, attempts: [] });
const ATTEMPT_WINDOW = 20;
const CHECK_WINDOW = 500;
const CASE_WINDOW = 4000;
const OUTCOMES = new Set(['pass', 'fail', 'skip', 'reused', 'interrupted']);
const hostId = createHash('sha256').update(hostname()).digest('hex').slice(0, 12);
const safeIdentity = (value) =>
  typeof value === 'string' && value.length <= 300 && !value.includes('\\') && !value.startsWith('/') &&
  !/[\r\n\t\0]/.test(value) && !/(?:[A-Za-z]:\/|\/Users\/|\/home\/|\/private\/)/.test(value);
const percentile = (values, rank) => values.length ? [...values].sort((a, b) => a - b)[Math.ceil(values.length * rank) - 1] : null;
const validMs = (ms) => Number.isFinite(ms) && ms > 0;
const sample = (ms, host) => ({ ms, host });
const pause = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);

function updateHistory(path, change) {
  mkdirSync(dirname(path), { recursive: true });
  const lock = `${path}.lock`;
  let handle;
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      handle = openSync(lock, 'wx');
      break;
    } catch (error) {
      if (error.code !== 'EEXIST') throw error;
      // A killed gate can leave a lock, but a live writer holds it only briefly.
      try {
        if (Date.now() - statSync(lock).mtimeMs > 10_000) rmSync(lock, { force: true });
      } catch (statError) {
        if (statError.code !== 'ENOENT') throw statError;
      }
      pause(20);
    }
  }
  if (handle === undefined) throw new Error('Timing history is busy');
  const temporary = `${path}.${process.pid}.${randomUUID()}.tmp`;
  try {
    const history = loadTimingHistory(path);
    change(history);
    writeFileSync(temporary, JSON.stringify(history));
    renameSync(temporary, path);
    return history;
  } finally {
    rmSync(temporary, { force: true });
    closeSync(handle);
    rmSync(lock, { force: true });
  }
}

export function loadTimingHistory(path) {
  try {
    const value = JSON.parse(readFileSync(path, 'utf8'));
    return value?.version === 1 && value.stages && value.files ? { ...value, imports: value.imports ?? {}, attempts: value.attempts ?? [] } : empty();
  } catch {
    return empty();
  }
}

/** Record one gate invocation idempotently. A started check is provisionally interrupted, so SIGKILL retains evidence. */
export function recordAttempt(path, attempt) {
  if (!safeIdentity(attempt?.id) || !['local', 'ci'].includes(attempt.source)) throw new Error('Invalid gate attempt identity');
  const sanitizeCheck = (check) => {
    if (!safeIdentity(check.id) || !OUTCOMES.has(check.outcome)) throw new Error('Invalid check observation');
    return {
      id: check.id,
      outcome: check.outcome,
      startedAt: Number.isFinite(check.startedAt) ? check.startedAt : null,
      endedAt: Number.isFinite(check.endedAt) ? check.endedAt : null,
      ms: validMs(check.ms) ? check.ms : null,
      category: safeIdentity(check.category) ? check.category : null,
    };
  };
  const selected = [...new Set(attempt.selected ?? [])];
  const deferred = [...new Set(attempt.deferred ?? [])];
  if (![...selected, ...deferred].every(safeIdentity)) throw new Error('Invalid selected/deferred identity');
  const value = {
    id: attempt.id, source: attempt.source, cohort: safeIdentity(attempt.cohort) ? attempt.cohort : attempt.source,
    os: safeIdentity(attempt.os) ? attempt.os : attempt.source === 'ci' ? 'unknown-ci' : `${platform()}-${arch()}`,
    host: safeIdentity(attempt.host) ? attempt.host : attempt.source === 'ci' ? 'ci-runner' : hostId,
    runId: safeIdentity(attempt.runId) ? attempt.runId : null,
    runAttempt: safeIdentity(attempt.runAttempt) ? attempt.runAttempt : null,
    startedAt: Number.isFinite(attempt.startedAt) ? attempt.startedAt : null,
    endedAt: Number.isFinite(attempt.endedAt) ? attempt.endedAt : null,
    selected: selected.slice(0, CHECK_WINDOW), deferred: deferred.slice(0, CHECK_WINDOW),
    checks: (attempt.checks ?? []).slice(-CHECK_WINDOW).map(sanitizeCheck),
    cases: (attempt.cases ?? []).slice(-CASE_WINDOW).map(sanitizeCheck),
  };
  return updateHistory(path, (history) => {
    history.attempts ??= [];
    const old = history.attempts.findIndex((entry) => entry.id === value.id);
    if (old >= 0) history.attempts.splice(old, 1);
    history.attempts.push(value);
    if (history.attempts.length > ATTEMPT_WINDOW) history.attempts.splice(0, history.attempts.length - ATTEMPT_WINDOW);
  });
}

function summarizeAttemptCohort(attempts, requested = []) {
  const ids = new Set([...requested, ...attempts.flatMap((entry) => [...entry.selected, ...entry.deferred, ...entry.checks.map((check) => check.id)])]);
  const summarize = (id, kind) => {
    const selected = kind === 'checks' ? attempts.filter((entry) => entry.selected.includes(id)).length : null;
    const deferred = kind === 'checks' ? attempts.filter((entry) => entry.deferred.includes(id)).length : null;
    const observations = attempts.flatMap((entry) => entry[kind].filter((check) => check.id === id));
    const durations = (outcome) => observations.filter((check) => check.outcome === outcome && validMs(check.ms)).map((check) => check.ms);
    const distribution = (outcome) => {
      const values = durations(outcome);
      return { samples: values.length, p50: percentile(values, 0.5), p75: percentile(values, 0.75), p90: percentile(values, 0.9) };
    };
    return {
      id, selected, deferred, attempted: observations.length,
      pass: observations.filter((check) => check.outcome === 'pass').length,
      fail: observations.filter((check) => check.outcome === 'fail').length,
      skip: observations.filter((check) => check.outcome === 'skip').length,
      reused: observations.filter((check) => check.outcome === 'reused').length,
      interrupted: observations.filter((check) => check.outcome === 'interrupted').length,
      successMs: distribution('pass'), failureMs: distribution('fail'),
      unknown: observations.length === 0,
    };
  };
  const checks = [...ids].sort().map((id) => summarize(id, 'checks'));
  const cases = [...new Set(attempts.flatMap((entry) => entry.cases.map((check) => check.id)))].sort().map((id) => summarize(id, 'cases'));
  return { attempts: attempts.length, checks, cases };
}

export function summarizeObservations(history, requested = []) {
  const attempts = history.attempts ?? [];
  return {
    ...summarizeAttemptCohort(attempts, requested),
    cohorts: [...new Set(attempts.map((entry) => entry.cohort))].sort().map((cohort) => ({
      cohort,
      ...summarizeAttemptCohort(attempts.filter((entry) => entry.cohort === cohort), requested),
    })),
  };
}

/** Stable runner case ids only; reports' raw diagnostics and absolute paths never enter history. */
export function reportCaseObservations(kind, body, root) {
  const rows = kind === 'nextest' ? nextestOutcomes(body) : kind === 'vitest' ? vitestOutcomes(JSON.parse(body)) : playwrightOutcomes(JSON.parse(body));
  return rows.flatMap((row) => {
    let id;
    if (kind === 'nextest') id = safeIdentity(row.name) ? `nextest:${row.name}` : null;
    else {
      const normalized = row.name.replaceAll('\\', '/');
      const marker = kind === 'vitest' ? ' > ' : ' › ';
      const boundary = normalized.indexOf(marker);
      if (boundary < 0) return [];
      const prefix = normalized.slice(0, boundary);
      const suffix = normalized.slice(boundary);
      const filePart = kind === 'playwright' ? prefix.replace(/^\[[^\]]+\] /, '').replace(/:\d+$/, '') : prefix;
      const file = safeFile(root, filePart);
      if (!file) return [];
      id = `${kind}:${file}#${createHash('sha256').update(suffix).digest('hex').slice(0, 16)}`;
    }
    if (!safeIdentity(id)) return [];
    return [{ id, outcome: row.outcome, ms: row.seconds * 1000 }];
  });
}

export function reportFileObservations(kind, body, root) {
  const files = new Map();
  const add = (file, outcome, ms) => {
    if (!file) return;
    const id = `${kind}:${file}`;
    const previous = files.get(id) ?? { id, outcome: 'skip', ms: 0 };
    if (outcome === 'fail' || (outcome === 'interrupted' && previous.outcome !== 'fail')) previous.outcome = outcome;
    else if (outcome === 'pass' && previous.outcome === 'skip') previous.outcome = 'pass';
    previous.ms += validMs(ms) ? ms : 0;
    files.set(id, previous);
  };
  if (kind === 'nextest') {
    for (const row of nextestOutcomes(body)) {
      const separator = row.name.indexOf(' ');
      if (separator >= 0) add(rustTestFile(root, row.name.slice(0, separator), row.name.slice(separator + 1)), row.outcome, row.seconds * 1000);
    }
  } else if (kind === 'vitest') {
    const parsed = JSON.parse(body);
    for (const file of parsed.testResults ?? []) {
      const outcome = file.status === 'passed' || (file.status === undefined && parsed.success === true) ? 'pass' : file.status === 'pending' ? 'skip' : 'fail';
      add(safeFile(root, file.name), outcome, file.endTime - file.startTime);
    }
  } else {
    const parsed = JSON.parse(body);
    const visit = (suite) => {
      for (const spec of suite.specs ?? []) for (const test of spec.tests ?? [])
        for (const result of test.results ?? []) add(safeFile(root, spec.file), result.status === 'passed' ? 'pass' : result.status === 'skipped' ? 'skip' : result.status === 'interrupted' ? 'interrupted' : 'fail', result.duration);
      for (const child of suite.suites ?? []) visit(child);
    };
    for (const suite of parsed.suites ?? []) visit(suite);
  }
  return [...files.values()].map((row) => ({ ...row, ms: validMs(row.ms) ? row.ms : null }));
}

export function appendTiming(history, kind, key, ms, host = 'local') {
  if (!['stages', 'files'].includes(kind) || !key || !validMs(ms) || !['local', 'ci'].includes(host)) return false;
  const records = (history[kind][key] ??= []);
  records.push(sample(ms, host));
  if (records.length > WINDOW) records.splice(0, records.length - WINDOW);
  return true;
}

export function timingEstimate(records = []) {
  const local = records.filter((entry) => entry.host === 'local' && validMs(entry.ms));
  const ci = records.filter((entry) => entry.host === 'ci' && validMs(entry.ms));
  const chosen = local.length ? local : ci;
  if (!chosen.length) return { ms: null, samples: 0, host: 'unknown' };
  const sorted = chosen.map((entry) => entry.ms).sort((a, b) => a - b);
  return {
    ms: sorted[Math.ceil(sorted.length * 0.75) - 1],
    samples: sorted.length,
    host: local.length ? 'local' : 'ci',
  };
}

export function summarizeTimings(history, keys = {}) {
  const summarize = (kind, requested) =>
    [...new Set(requested ?? Object.keys(history[kind]))]
      .sort()
      .map((key) => ({ key, ...timingEstimate(history[kind][key]) }));
  return { stages: summarize('stages', keys.stages), files: summarize('files', keys.files) };
}

export function stageTimingKey(stage) {
  return `${stage.cwd}: ${stage.command}`;
}

/** Stable, path-safe gate identity; long shell commands are never stored in observation rows. */
export function observationStageKey(stage) {
  return `gate:${stage.cwd}:${createHash('sha256').update(stage.command).digest('hex').slice(0, 16)}`;
}

function safeFile(root, name) {
  if (typeof name !== 'string' || !name) return null;
  const normalized = name.replaceAll('\\', '/');
  const anchor = '/clients/web/';
  const candidate =
    (isAbsolute(normalized) || /^[A-Za-z]:\//.test(normalized)) && !normalized.startsWith(`${root.replaceAll('\\', '/')}/`) && normalized.includes(anchor)
      ? normalized.slice(normalized.lastIndexOf(anchor) + anchor.length)
      : normalized;
  const path = resolve(root, candidate);
  const rel = relative(root, path).replaceAll('\\', '/');
  return rel && rel !== '..' && !rel.startsWith('../') && !isAbsolute(rel) ? rel : null;
}

function rustTestFile(root, suite, testName) {
  const [crate, target] = suite.split('::');
  if (!crate || !/^hotsheet-[a-z-]+$/.test(crate)) return null;
  const packageRoot = `crates/${crate}`;
  if (target) {
    const candidate = target.startsWith('bin/')
      ? `${packageRoot}/src/${target}.rs`
      : `${packageRoot}/tests/${target}.rs`;
    return existsSync(resolve(root, candidate)) ? candidate : null;
  }
  const modules = testName
    .split('::')
    .slice(0, -1)
    .filter((part) => part !== 'tests');
  for (let count = modules.length; count > 0; count -= 1) {
    const stem = `${packageRoot}/src/${modules.slice(0, count).join('/')}`;
    for (const candidate of [`${stem}.rs`, `${stem}/mod.rs`]) {
      if (existsSync(resolve(root, candidate))) return candidate;
    }
  }
  for (const candidate of [`${packageRoot}/src/lib.rs`, `${packageRoot}/src/main.rs`]) {
    if (existsSync(resolve(root, candidate))) return candidate;
  }
  return null;
}

export function reportFileTimings(kind, body, root) {
  if (kind === 'nextest') {
    // JUnit identifies the test target and module. Map only to source paths that
    // actually exist; an unknown target stays unknown instead of inventing a file.
    const byFile = new Map();
    for (const entry of nextestTimings(body)) {
      const separator = entry.name.indexOf(' ');
      if (separator < 0) continue;
      const file = rustTestFile(root, entry.name.slice(0, separator), entry.name.slice(separator + 1));
      if (!file || !Number.isFinite(entry.seconds)) continue;
      byFile.set(file, (byFile.get(file) ?? 0) + entry.seconds);
    }
    return [...byFile].map(([file, seconds]) => ({ key: `nextest:${file}`, ms: seconds * 1000 }));
  }
  const parsed = JSON.parse(body);
  const files =
    kind === 'vitest'
      ? (parsed.testResults ?? []).map((file) => ({ name: file.name, seconds: (file.endTime - file.startTime) / 1000 }))
      : playwrightTimings(parsed).files;
  return files
    .map(({ name, seconds }) => ({ name: safeFile(root, name), ms: seconds * 1000 }))
    .filter(({ name, ms }) => name && validMs(ms))
    .map(({ name, ms }) => ({ key: `${kind}:${name}`, ms }));
}

function reportSucceeded(kind, body) {
  if (kind === 'nextest') return !/<testsuites?\b[^>]*(?:failures|errors)="[1-9]|<(?:failure|error)\b/.test(body);
  const report = JSON.parse(body);
  if (kind === 'vitest') return report.success === true;
  return report.stats?.unexpected === 0 && (report.stats?.flaky ?? 0) === 0;
}

export function recordCompletedGate(path, observations) {
  if (!observations.length) return loadTimingHistory(path);
  return updateHistory(path, (history) => {
    for (const observation of observations) {
      if (observation.reused || observation.status !== 0) continue;
      appendTiming(history, 'stages', stageTimingKey(observation), observation.ms);
      for (const file of observation.files ?? []) appendTiming(history, 'files', file.key, file.ms);
    }
  });
}

export function importCiReports(path, reports, roots, metadata = {}) {
  const attemptId = safeIdentity(metadata.runId) && safeIdentity(metadata.runAttempt)
    ? `ci:${metadata.runId}:${metadata.runAttempt}` : null;
  const observations = [];
  const history = updateHistory(path, (history) => {
    for (const [kind, report] of Object.entries(reports)) {
      if (!report) continue;
      const body = readFileSync(report, 'utf8');
      const digest = createHash('sha256').update(body).digest('hex');
      const passed = reportSucceeded(kind, body);
      const identity = attemptId ? `${attemptId}:${kind}` : `ci:digest:${kind}:${digest}`;
      observations.push({ identity, kind, passed, body });
      if (!passed || history.imports[`${kind}:${digest}`]) continue;
      for (const file of reportFileTimings(kind, body, roots[kind])) {
        appendTiming(history, 'files', file.key, file.ms, 'ci');
      }
      history.imports[`${kind}:${digest}`] = true;
    }
    for (const key of Object.keys(history.imports).slice(0, -100)) delete history.imports[key];
  });
  for (const report of observations) {
    const cases = reportCaseObservations(report.kind, report.body, roots[report.kind]);
    const files = reportFileObservations(report.kind, report.body, roots[report.kind]);
    recordAttempt(path, {
      id: report.identity, source: 'ci', cohort: metadata.cohort ?? 'ci-reports',
      os: metadata.os, host: metadata.host,
      runId: metadata.runId, runAttempt: metadata.runAttempt,
      startedAt: metadata.startedAt, endedAt: metadata.endedAt,
      selected: [`report:${report.kind}`], deferred: [],
      checks: [{ id: `report:${report.kind}`, outcome: report.passed ? 'pass' : 'fail', category: report.passed ? null : 'test-report-failure' }, ...files],
      cases,
    });
  }
  return history;
}

/** Import GitHub's finished jobs API JSON; run/attempt/job identities make reruns idempotent. */
export function importCiJobs(path, body, metadata = {}) {
  const parsed = typeof body === 'string' ? JSON.parse(body) : body;
  const jobs = Array.isArray(parsed) ? parsed : parsed.jobs;
  if (!Array.isArray(jobs) || !safeIdentity(String(metadata.runId ?? '')) || !safeIdentity(String(metadata.runAttempt ?? '')))
    throw new Error('CI jobs import needs jobs, run id, and attempt');
  for (const job of jobs) {
    if (!Number.isInteger(job.id) || !safeIdentity(job.name)) continue;
    const steps = (job.steps ?? []).filter((step) => safeIdentity(step.name));
    const key = (step) => `ci:${job.name}:${step.name}`;
    const selected = steps.filter((step) => step.conclusion !== 'skipped').map(key);
    const deferred = steps.filter((step) => step.conclusion === 'skipped').map(key);
    const checks = steps.map((step) => {
      const startedAt = Date.parse(step.started_at);
      const endedAt = Date.parse(step.completed_at);
      const outcome = step.conclusion === 'skipped' ? 'skip' : step.conclusion === 'success' ? 'pass' :
        ['cancelled', 'timed_out', 'action_required'].includes(step.conclusion) ? 'interrupted' : 'fail';
      return {
        id: key(step), outcome,
        startedAt, endedAt, ms: Number.isFinite(startedAt) && Number.isFinite(endedAt) ? endedAt - startedAt : null,
        category: outcome === 'pass' ? null : `ci:${step.conclusion ?? 'unknown'}`,
      };
    });
    recordAttempt(path, {
      id: `ci:${metadata.runId}:${metadata.runAttempt}:job:${job.id}`,
      source: 'ci', cohort: metadata.cohort ?? 'ci-main', os: metadata.os ?? 'ci-runner',
      runId: String(metadata.runId), runAttempt: String(metadata.runAttempt),
      startedAt: Date.parse(job.started_at), endedAt: Date.parse(job.completed_at),
      selected, deferred, checks,
    });
  }
  return loadTimingHistory(path);
}
