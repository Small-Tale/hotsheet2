// HS2-TCCKRZ: bounded, machine-local observations for planning a local gate budget.
import { createHash, randomUUID } from 'node:crypto';
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
import { nextestTimings, playwrightTimings } from './test-profile.mjs';

const WINDOW = 20;
const empty = () => ({ version: 1, stages: {}, files: {}, imports: {} });
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
    return value?.version === 1 && value.stages && value.files ? { ...value, imports: value.imports ?? {} } : empty();
  } catch {
    return empty();
  }
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

function safeFile(root, name) {
  if (typeof name !== 'string' || !name) return null;
  const normalized = name.replaceAll('\\', '/');
  const anchor = '/clients/web/';
  const candidate =
    isAbsolute(normalized) && !normalized.startsWith(`${root.replaceAll('\\', '/')}/`) && normalized.includes(anchor)
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

export function importCiReports(path, reports, roots) {
  return updateHistory(path, (history) => {
    for (const [kind, report] of Object.entries(reports)) {
      if (!report) continue;
      const body = readFileSync(report, 'utf8');
      if (!reportSucceeded(kind, body)) continue;
      const digest = createHash('sha256').update(body).digest('hex');
      if (history.imports[`${kind}:${digest}`]) continue;
      for (const file of reportFileTimings(kind, body, roots[kind])) {
        appendTiming(history, 'files', file.key, file.ms, 'ci');
      }
      history.imports[`${kind}:${digest}`] = true;
    }
    for (const key of Object.keys(history.imports).slice(0, -100)) delete history.imports[key];
  });
}
