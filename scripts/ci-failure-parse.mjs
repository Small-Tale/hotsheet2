// HS2-HAHX20: parse failed CI output into failure identities for batch failure attribution.
//
// Input is the text of `gh run view <run> --log-failed` (each line `<job>\t<step>\t<timestamp> <text>`)
// or a test report the CI uploads. Output keeps identifiers only: the job and step, the framework, the
// test id, the test file as printed, diagnostic locations, and one short excerpt (the identifier line
// itself). No other log text leaves this module (customer data hygiene).
//
// Formats: cargo-nextest, vitest, Playwright (list/line output and JSON), `node --test` (spec and
// TAP), and step diagnostics from rustc/clippy, `cargo fmt --check`, tsc, ESLint (stylish) and
// Prettier `--check`. JUnit XML (nextest's report) and vitest JSON reports are read too. A failed step
// whose output matches none of these becomes one step-level failure, so nothing red is dropped.
//
// Everything here is pure.

export const EXCERPT_LIMIT = 160;
const ANSI = /\u001b\[[0-9;?]*[ -/]*[@-~]/g;
const TIMESTAMP = /^﻿?\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z ?/;

export const excerpt = (line) => {
  const text = line.trim().replace(/\s*[─━]{2,}\s*$/, '').replace(/\s+/g, ' ');
  return text.length > EXCERPT_LIMIT ? `${text.slice(0, EXCERPT_LIMIT - 1)}…` : text;
};

/** Split `gh run view --log-failed` text into `{ job, step, text }` lines. Unprefixed text has no job. */
export function splitLog(raw) {
  const lines = [];
  for (const original of raw.replace(/\r\n?/g, '\n').split('\n')) {
    const plain = original.replace(ANSI, '');
    const parts = plain.split('\t');
    if (parts.length >= 3 && TIMESTAMP.test(parts.slice(2).join('\t'))) {
      lines.push({ job: parts[0], step: parts[1], text: parts.slice(2).join('\t').replace(TIMESTAMP, '') });
    } else lines.push({ job: '', step: '', text: plain.replace(TIMESTAMP, '') });
  }
  return lines;
}

// cargo-nextest: `FAIL [   0.012s] (  3/120) hotsheet-cli::cli some::test` (the counter is optional).
const NEXTEST =
  /^\s*(?:TRY (\d+) )?(FAIL|TIMEOUT|SIGSEGV|SIGABRT|SIGKILL|ABORT|LEAK-FAIL|PASS|FLAKY(?: \d+\/\d+)?) \[\s*[\d.]+m?s\]\s+(?:\(\s*\d+\/\s*\d+\)\s+)?(\S+)\s+(\S+)\s*$/;
// vitest: ` FAIL  src/a.test.ts > suite > name` or ` FAIL  |project| src/a.test.ts [ src/a.test.ts ]`.
const VITEST = /^\s*FAIL\s+(?:\|[^|]+\|\s+)?(\S+\.(?:test|spec)\.[cm]?[jt]sx?)(?:\s+>\s+(.+?))?(?:\s+\[[^\]]*\])?\s*$/;
// Playwright: `  1) [chromium] › tests/a.spec.ts:12:5 › suite › title ──────` and summary rows.
const PLAYWRIGHT =
  /^\s*(?:(\d+)\)\s+)?\[([\w .-]+)\]\s+[›>]\s+(\S+?\.(?:spec|test)\.[cm]?[jt]sx?):(\d+):(\d+)\s+[›>]\s+(.+?)\s*$/;
const PLAYWRIGHT_SECTION = /^\s*(\d+) (failed|flaky|passed|skipped|did not run|interrupted)\b/;
// node --test spec reporter: `test at scripts/a.test.mjs:12:1` followed by `✖ name (1.2ms)`.
const NODE_AT = /^\s*test at (\S+?):(\d+):(\d+)\s*$/;
const NODE_CROSS = /^\s*✖\s+(.+?)(?:\s+\([\d.]+m?s\))?\s*$/;
const TAP_NOT_OK = /^\s*not ok \d+ - (.+?)\s*$/;
const TAP_LOCATION = /^\s*location:\s*'?(.+?):(\d+):(\d+)'?\s*$/;
// Step diagnostics.
const RUST_HEADER = /^\s*(error|warning)(?:\[[A-Z]\d+\])?:/;
const RUST_LOCATION = /^\s*-->\s+(\S+?\.rs):(\d+):(\d+)/;
const FMT_DIFF = /^\s*Diff in (\S+?\.rs)(?::\d+:?| at line \d+:?)/;
const TSC = /^\s*(\S+?\.[cm]?tsx?)(?:\((\d+),(\d+)\)|:(\d+):(\d+))\s*:?\s*-?\s*error\s+TS\d+/;
const ESLINT_FILE = /^\s*(\/\S+\.(?:[cm]?[jt]sx?|json|css|md))\s*$/;
const ESLINT_PROBLEM = /^\s+\d+:\d+\s+error\s+/;
const PRETTIER = /^\s*\[warn\]\s+(\S+\.[a-z]+)\s*$/;
const STEP_ERROR = /##\[error\]|Process completed with exit code [1-9]/;

/**
 * Failures and flaky tests from one attempt's failed log. Each failure:
 * `{ framework, kind: 'test'|'diagnostic'|'step', job, step, test, file, locations, projects, excerpt }`.
 */
export function parseLog(raw) {
  const lines = splitLog(raw);
  const failures = new Map();
  const flaky = new Map();
  const steps = new Map();
  const stepKey = (line) => `${line.job}\u0000${line.step}`;
  const touch = (line) => {
    const key = stepKey(line);
    if (!steps.has(key)) steps.set(key, { job: line.job, step: line.step, parsed: false, errored: false, excerpt: '' });
    return steps.get(key);
  };
  const add = (map, line, entry) => {
    const id = `${entry.framework}\u0000${entry.test ?? ''}\u0000${entry.file ?? ''}${entry.kind === 'test' ? '' : `\u0000${line.job}\u0000${line.step}`}`;
    const existing = map.get(id);
    if (existing) {
      for (const location of entry.locations ?? []) if (!existing.locations.includes(location)) existing.locations.push(location);
      for (const project of entry.projects ?? []) if (!existing.projects.includes(project)) existing.projects.push(project);
      return existing;
    }
    const value = { kind: 'test', locations: [], projects: [], job: line.job, step: line.step, excerpt: excerpt(line.text), ...entry };
    map.set(id, value);
    if (map === failures) touch(line).parsed = true;
    return value;
  };

  const nextest = { failed: new Map(), passed: new Set() };
  // Playwright: numbered headers, plus the summary sections when the reporter prints them.
  const playwright = { headers: new Map(), failed: new Map(), flaky: new Map(), section: null, sectionStep: null };
  let rustLevel = null;
  let eslintFile = null;
  let nodeAt = null;
  let tap = null;
  // A TAP parent (a describe or file suite) reports `not ok` after its failing, deeper children.
  let tapChildIndent = -1;

  for (const [index, line] of lines.entries()) {
    const text = line.text;
    const step = touch(line);
    if (STEP_ERROR.test(text)) {
      step.errored = true;
      if (!step.excerpt) step.excerpt = excerpt(text);
    }

    let match = NEXTEST.exec(text);
    if (match) {
      const [, , status, binary, test] = match;
      const id = `${binary} ${test}`;
      if (status === 'PASS' || status.startsWith('FLAKY')) nextest.passed.add(id);
      if (status.startsWith('FLAKY')) add(flaky, line, { framework: 'nextest', test: id, file: null, binary });
      else if (status !== 'PASS' && !nextest.failed.has(id)) nextest.failed.set(id, { line, binary, test: id });
      continue;
    }
    match = VITEST.exec(text);
    if (match) {
      add(failures, line, { framework: 'vitest', file: match[1], test: match[2] ? `${match[1]} > ${match[2]}` : match[1] });
      continue;
    }
    match = PLAYWRIGHT_SECTION.exec(text);
    if (match && /^\s*\d+ \w/.test(text) && !/[›>]/.test(text)) {
      playwright.section = match[2];
      playwright.sectionStep = stepKey(line);
      continue;
    }
    match = PLAYWRIGHT.exec(text);
    if (match) {
      const [, number, project, file, , , rawTitle] = match;
      const title = rawTitle.replace(/\s*[─━-]{2,}\s*$/, '').replace(/\s*\(retry #\d+\)\s*$/, '').trim();
      const entry = { line, project, file, test: `${file} › ${title}` };
      if (number) playwright.headers.set(entry.test, playwright.headers.get(entry.test) ?? entry);
      else if (playwright.sectionStep === stepKey(line) && playwright.section === 'failed') playwright.failed.set(entry.test, entry);
      else if (playwright.sectionStep === stepKey(line) && playwright.section === 'flaky') playwright.flaky.set(entry.test, entry);
      const target = number ? playwright.headers : null;
      if (target) {
        const kept = target.get(entry.test);
        kept.projects = [...new Set([...(kept.projects ?? []), project])];
      }
      continue;
    }
    if (playwright.section && !/^\s{2,}/.test(text) && text.trim()) playwright.section = null;

    match = NODE_AT.exec(text);
    if (match) {
      nodeAt = { file: match[1], line };
      continue;
    }
    if (nodeAt) {
      match = NODE_CROSS.exec(text);
      if (match) add(failures, nodeAt.line, { framework: 'node-test', file: nodeAt.file, test: `${nodeAt.file} > ${match[1]}`, excerpt: excerpt(text) });
      nodeAt = null;
      if (match) continue;
    }
    match = TAP_NOT_OK.exec(text);
    if (match) {
      const indent = /^\s*/.exec(text)[0].length;
      tap = tapChildIndent > indent ? null : { name: match[1], line };
      tapChildIndent = indent;
      continue;
    }
    if (tap) {
      match = TAP_LOCATION.exec(text);
      if (match) {
        const file = match[1].replace(/^file:\/\//, '');
        // A TAP suite (`not ok` for a whole file) names the file itself; keep only test entries.
        if (!file.endsWith(tap.name)) add(failures, tap.line, { framework: 'node-test', file, test: `${file} > ${tap.name}` });
        tap = null;
        continue;
      }
      if (/^\s*\.\.\.\s*$/.test(text)) tap = null;
    }

    match = RUST_HEADER.exec(text);
    if (match) {
      rustLevel = match[1];
      continue;
    }
    match = RUST_LOCATION.exec(text);
    if (match) {
      if (rustLevel === 'error') add(failures, line, { framework: 'rustc', kind: 'diagnostic', file: null, test: null, locations: [match[1]] });
      rustLevel = null;
      continue;
    }
    match = FMT_DIFF.exec(text);
    if (match) {
      add(failures, line, { framework: 'rustfmt', kind: 'diagnostic', file: null, test: null, locations: [match[1]] });
      continue;
    }
    match = TSC.exec(text);
    if (match) {
      add(failures, line, { framework: 'tsc', kind: 'diagnostic', file: null, test: null, locations: [match[1]] });
      continue;
    }
    match = ESLINT_FILE.exec(text);
    if (match && ESLINT_PROBLEM.test(lines[index + 1]?.text ?? '')) {
      eslintFile = match[1];
      continue;
    }
    if (eslintFile && ESLINT_PROBLEM.test(text)) {
      add(failures, line, { framework: 'eslint', kind: 'diagnostic', file: null, test: null, locations: [eslintFile] });
      continue;
    }
    if (!/^\s/.test(text)) eslintFile = null;
    match = PRETTIER.exec(text);
    if (match) add(failures, line, { framework: 'prettier', kind: 'diagnostic', file: null, test: null, locations: [match[1]] });
  }

  for (const [id, { line, binary }] of nextest.failed) {
    if (nextest.passed.has(id)) add(flaky, line, { framework: 'nextest', test: id, file: null, binary });
    else add(failures, line, { framework: 'nextest', test: id, file: null, binary });
  }
  const sectioned = playwright.failed.size + playwright.flaky.size > 0;
  for (const [id, entry] of playwright.headers) {
    const failed = sectioned ? playwright.failed.has(id) : true;
    const projects = [...new Set([...(entry.projects ?? []), ...(playwright.failed.get(id) ? [playwright.failed.get(id).project] : [])])];
    add(failed ? failures : flaky, entry.line, { framework: 'playwright', file: entry.file, test: id, projects });
  }
  for (const [id, entry] of playwright.failed)
    if (!playwright.headers.has(id)) add(failures, entry.line, { framework: 'playwright', file: entry.file, test: id, projects: [entry.project] });
  for (const [id, entry] of playwright.flaky)
    if (!playwright.headers.has(id)) add(flaky, entry.line, { framework: 'playwright', file: entry.file, test: id, projects: [entry.project] });

  for (const step of steps.values()) {
    if (!step.errored || step.parsed || !step.job) continue;
    const id = `step\u0000${step.job}\u0000${step.step}`;
    failures.set(id, { framework: 'step', kind: 'step', job: step.job, step: step.step, test: null, file: null, locations: [], projects: [], excerpt: step.excerpt });
  }
  // Playwright prints the same test as both a numbered header and a summary row; a nextest test is
  // never both failed and flaky. A flaky entry never doubles as a failure.
  const failedIds = new Set([...failures.values()].map(identity));
  return {
    failures: [...failures.values()],
    flaky: [...flaky.values()].filter((entry) => !failedIds.has(identity(entry))),
  };
}

/**
 * The stable identity of a failure. A test is the same failure whichever job or shard ran it; a
 * diagnostic or failed step is identified by its job and step.
 */
export function identity(failure) {
  if (failure.kind === 'test') return `${failure.framework}:${failure.test}`;
  return `${failure.framework}:${failure.job} / ${failure.step}`;
}

const xmlUnescape = (value) =>
  value.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&amp;/g, '&');
const attribute = (tag, name) => {
  const match = new RegExp(`\\s${name}="([^"]*)"`).exec(tag);
  return match ? xmlUnescape(match[1]) : null;
};

/** Failed test cases from a JUnit XML report (nextest writes `classname` = binary id, `name` = test). */
export function parseJUnit(xml, { job = 'report', step = 'junit' } = {}) {
  const failures = [];
  const flaky = [];
  for (const match of xml.matchAll(/<testcase\b([^>]*?)(?:\/>|>([\s\S]*?)<\/testcase>)/g)) {
    const [, attrs, body = ''] = match;
    const name = attribute(attrs, 'name');
    const classname = attribute(attrs, 'classname') ?? '';
    const file = attribute(attrs, 'file');
    if (!name) continue;
    const test = classname ? `${classname} ${name}` : name;
    const failed = /<(failure|error)\b/.test(body);
    const retried = /<(flakyFailure|flakyError|rerunFailure)\b/.test(body);
    const entry = { framework: 'nextest', kind: 'test', job, step, test, file, binary: classname || null, locations: [], projects: [], excerpt: excerpt(test) };
    if (failed) failures.push(entry);
    else if (retried) flaky.push(entry);
  }
  return { failures, flaky };
}

/** Failed tests from a vitest JSON report (`--reporter=json`). */
export function parseVitestJson(report, { job = 'report', step = 'vitest', root = '' } = {}) {
  const failures = [];
  const strip = (path) => (root && path.startsWith(`${root}/`) ? path.slice(root.length + 1) : path);
  for (const file of report.testResults ?? []) {
    const path = strip(file.name ?? '');
    const failed = (file.assertionResults ?? []).filter((result) => result.status === 'failed');
    for (const result of failed) {
      const name = result.fullName ?? [...(result.ancestorTitles ?? []), result.title].join(' > ');
      failures.push({ framework: 'vitest', kind: 'test', job, step, file: path, test: `${path} > ${name}`, locations: [], projects: [], excerpt: excerpt(`${path} > ${name}`) });
    }
    if (!failed.length && file.status === 'failed')
      failures.push({ framework: 'vitest', kind: 'test', job, step, file: path, test: path, locations: [], projects: [], excerpt: excerpt(path) });
  }
  return { failures, flaky: [] };
}

/** Failed (`unexpected`) and flaky tests from a Playwright JSON report. */
export function parsePlaywrightJson(report, { job = 'report', step = 'playwright' } = {}) {
  const failures = new Map();
  const flaky = new Map();
  const visit = (suite, titles) => {
    const nextTitles = suite.title && !suite.file?.endsWith(suite.title) ? [...titles, suite.title] : titles;
    for (const spec of suite.specs ?? [])
      for (const test of spec.tests ?? []) {
        const target = test.status === 'unexpected' ? failures : test.status === 'flaky' ? flaky : null;
        if (!target) continue;
        const id = `${spec.file} › ${[...nextTitles, spec.title].join(' › ')}`;
        const entry = target.get(id) ?? { framework: 'playwright', kind: 'test', job, step, file: spec.file, test: id, locations: [], projects: [], excerpt: excerpt(id) };
        if (test.projectName && !entry.projects.includes(test.projectName)) entry.projects.push(test.projectName);
        target.set(id, entry);
      }
    for (const child of suite.suites ?? []) visit(child, nextTitles);
  };
  for (const suite of report.suites ?? []) visit(suite, []);
  return { failures: [...failures.values()], flaky: [...flaky.values()] };
}

/** Parse one report file by its content: JUnit XML, a vitest JSON report, or a Playwright JSON report. */
export function parseReport(text, options = {}) {
  const trimmed = text.trimStart();
  if (trimmed.startsWith('<')) return parseJUnit(text, options);
  const json = JSON.parse(text);
  if (Array.isArray(json.testResults)) return parseVitestJson(json, options);
  if (Array.isArray(json.suites)) return parsePlaywrightJson(json, options);
  throw new Error('unrecognized test report: expected JUnit XML, vitest JSON, or Playwright JSON');
}

/** Merge parsed attempts: failures of the latest attempt stand; earlier-only failures are flaky. */
export function combineAttempts(latest, earlier = []) {
  const failed = new Set(latest.failures.map(identity));
  const flaky = new Map(latest.flaky.map((entry) => [identity(entry), { ...entry, reason: 'passed on a retry within the run' }]));
  for (const attempt of earlier)
    for (const failure of attempt.failures) {
      const id = identity(failure);
      if (!failed.has(id) && !flaky.has(id)) flaky.set(id, { ...failure, reason: 'failed on an earlier attempt and cleared on re-run' });
    }
  for (const id of failed) flaky.delete(id);
  return { failures: latest.failures, flaky: [...flaky.values()] };
}
