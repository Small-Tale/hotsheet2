// HS2-5340N1: a failed suite stage reruns only its failed cases on the next gate attempt
// for the same exact stage inputs, instead of the whole suite.
//
// The full run's fresh report must map every failed case to a selector the runner can
// rerun on its own. Anything the gate cannot prove (an interrupted run, a missing or
// unparseable report, a nonzero exit with no failed case, an unmappable case, an
// unexpected command shape) leaves no record, so the next attempt reruns the whole stage.
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { relative } from 'node:path';

const VERSION = 1;
const SAFE_TOKEN = /^[A-Za-z0-9_@%+=:,./-]+$/;
const SAFE_NEXTEST = /^[A-Za-z0-9_:.\-/ ]+$/;
// Playwright options whose next token is a value, never a file filter.
const PLAYWRIGHT_VALUE_OPTIONS = new Set([
  '-c',
  '--config',
  '-g',
  '--grep',
  '--grep-invert',
  '--project',
  '--shard',
  '-j',
  '--workers',
  '--repeat-each',
  '--retries',
  '--timeout',
  '--reporter',
  '--output',
  '--global-timeout',
  '--max-failures',
]);

const quote = (value) => `'${value.replaceAll("'", "'\\''")}'`;

/** Single-quoted or plain tokens only; anything else (expansion, escapes, double quotes) is unsupported. */
export function tokenize(command) {
  const tokens = [];
  const pattern = /\s*(?:'([^']*)'|([^\s'"\\$`]+))(?=\s|$)/y;
  let index = 0;
  while (index < command.length) {
    if (/^\s*$/.test(command.slice(index))) break;
    pattern.lastIndex = index;
    const match = pattern.exec(command);
    if (!match) return null;
    tokens.push(match[1] ?? match[2]);
    index = pattern.lastIndex;
  }
  return tokens;
}

function playwrightCases(report) {
  const cases = [];
  const visit = (suite, titles) => {
    const next = suite.title && !suite.file?.endsWith(suite.title) ? [...titles, suite.title] : titles;
    for (const spec of suite.specs ?? [])
      for (const test of spec.tests ?? []) {
        const results = test.results ?? [];
        const last = results.at(-1)?.status;
        const outcome =
          test.status === 'flaky' || test.status === 'expected'
            ? last === 'skipped'
              ? 'skip'
              : 'pass'
            : test.status === 'skipped' || (!results.length && test.status === undefined)
              ? 'skip'
              : last === 'interrupted'
                ? 'interrupted'
                : test.status === 'unexpected' || (last !== undefined && last !== 'passed' && last !== 'skipped')
                  ? 'fail'
                  : last === 'skipped'
                    ? 'skip'
                    : 'pass';
        const title = [...next, spec.title].join(' › ');
        cases.push({
          key: `${test.projectName ?? ''}|${spec.file}:${spec.line}|${title}`,
          name: `${test.projectName ? `[${test.projectName}] ` : ''}${spec.file}:${spec.line} › ${title}`,
          selector: typeof spec.file === 'string' && Number.isInteger(spec.line) ? `${spec.file}:${spec.line}` : null,
          outcome,
        });
      }
    for (const child of suite.suites ?? []) visit(child, next);
  };
  for (const suite of report.suites ?? []) visit(suite, []);
  return cases;
}

function vitestCases(report, root) {
  const cases = [];
  for (const file of report.testResults ?? []) {
    const path = typeof file.name === 'string' ? relative(root, file.name).replaceAll('\\', '/') : null;
    const assertions = file.assertionResults ?? [];
    // A file that failed outside any test (an import or collection error) has no case to rerun.
    if (file.status === 'failed' && !assertions.some((result) => result.status === 'failed'))
      cases.push({ key: `${path}|`, name: `${path} (file)`, selector: null, outcome: 'fail' });
    for (const result of assertions) {
      const fullName = result.fullName ?? [...(result.ancestorTitles ?? []), result.title].join(' ');
      cases.push({
        key: `${path}|${fullName}`,
        name: `${path} > ${fullName}`,
        selector:
          path && !path.startsWith('..') && typeof fullName === 'string' ? { file: path, name: fullName } : null,
        outcome:
          result.status === 'passed'
            ? 'pass'
            : result.status === 'failed'
              ? 'fail'
              : ['skipped', 'pending', 'todo'].includes(result.status)
                ? 'skip'
                : 'fail',
      });
    }
  }
  return cases;
}

const xmlAttribute = (tag, name) => {
  const value = new RegExp(`\\b${name}="([^"]*)"`).exec(tag)?.[1];
  return value
    ?.replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&quot;', '"')
    .replaceAll('&apos;', "'")
    .replaceAll('&amp;', '&');
};

function nextestCases(xml) {
  return [...xml.matchAll(/<testcase\b([^>]*?)(?:\/>|>([\s\S]*?)<\/testcase>)/g)].map(([, tag, body = '']) => {
    const binary = xmlAttribute(tag, 'classname');
    const name = xmlAttribute(tag, 'name');
    return {
      key: `${binary} ${name}`,
      name: `${binary} ${name}`,
      selector: binary && name && SAFE_NEXTEST.test(binary) && SAFE_NEXTEST.test(name) ? { binary, name } : null,
      outcome: /<(?:failure|error)\b/.test(body) ? 'fail' : /<skipped\b/.test(body) ? 'skip' : 'pass',
    };
  });
}

/** Every case in one runner report with its final outcome and rerun selector, or null when unreadable. */
export function reportCases(kind, body, root) {
  try {
    if (kind === 'playwright') return playwrightCases(JSON.parse(body));
    if (kind === 'vitest') return vitestCases(JSON.parse(body), root);
    if (kind === 'nextest') return nextestCases(body);
  } catch {
    return null;
  }
  return null;
}

/** The narrowed command for exactly these failed cases, or null when the stage cannot be narrowed. */
export function narrowedCommand(command, kind, failed) {
  if (!failed.length || failed.some((entry) => !entry.selector)) return null;
  if (kind === 'playwright') {
    const tokens = tokenize(command);
    if (!tokens || tokens[0] !== 'npx' || tokens[1] !== 'playwright' || tokens[2] !== 'test') return null;
    const options = [];
    for (let index = 3; index < tokens.length; index += 1) {
      const token = tokens[index];
      if (token.startsWith('-')) {
        options.push(token);
        if (PLAYWRIGHT_VALUE_OPTIONS.has(token) && index + 1 < tokens.length) options.push(tokens[++index]);
      }
      // Any other token is a file filter; the failed cases' locations replace them.
    }
    const locations = [...new Set(failed.map((entry) => entry.selector))];
    // Playwright matches a location filter as a pattern against the absolute path; the leading
    // separator keeps `layout.spec.ts:9` from also selecting `mobile-layout.spec.ts:9`.
    if (!locations.every((location) => SAFE_TOKEN.test(location))) return null;
    return ['npx playwright test', ...locations.map((location) => quote(`/${location}`)), ...options.map(quote)].join(
      ' ',
    );
  }
  if (kind === 'vitest') {
    if (command !== 'npm run test:unit') return null;
    const files = [...new Set(failed.map((entry) => entry.selector.file))];
    if (!files.every((file) => SAFE_TOKEN.test(file))) return null;
    const names = [...new Set(failed.map((entry) => entry.selector.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')))];
    return `${command} -- ${files.map(quote).join(' ')} -t ${quote(`^(?:${names.join('|')})$`)}`;
  }
  if (kind === 'nextest') {
    if (!command.startsWith('cargo nextest run ') || /\s(?:-E|--filterset|--filter-expr)\b/.test(command)) return null;
    const expression = failed
      .map((entry) => `(binary_id(=${entry.selector.binary}) & test(=${entry.selector.name}))`)
      .join(' | ');
    return `${command} -E ${quote(expression)}`;
  }
  return null;
}

/** What a full stage run leaves for the next attempt, or null to rerun the whole stage. */
export function partialFromRun({ key, command, kind, status, signal, body, root }) {
  if (status === 0 || signal || body === null || body === undefined) return null;
  const cases = reportCases(kind, body, root);
  if (!cases?.length || cases.some((entry) => entry.outcome === 'interrupted')) return null;
  const failed = cases.filter((entry) => entry.outcome === 'fail');
  if (!failed.length) return null;
  const narrowed = narrowedCommand(command, kind, failed);
  if (!narrowed) return null;
  return {
    version: VERSION,
    key,
    kind,
    command,
    narrowed,
    total: cases.length,
    failed: failed.map(({ key: caseKey, name, selector }) => ({ key: caseKey, name, selector })),
    flaky: [],
  };
}

/**
 * Judge a narrowed rerun. `passed`: every requested case passed (the stage passes, and these
 * cases are flaky). `retry`: a smaller record for the cases that still fail. `full`: the rerun
 * could not be proven, so drop the record and rerun the whole stage next time.
 */
export function judgeNarrowedRun(partial, { status, signal, body, root }) {
  if (signal) return { result: 'full', reason: 'the targeted rerun was interrupted' };
  const cases = body === null || body === undefined ? null : reportCases(partial.kind, body, root);
  if (!cases) return { result: 'full', reason: 'the targeted rerun left no readable report' };
  const byKey = new Map(cases.map((entry) => [entry.key, entry]));
  const missing = partial.failed.filter((entry) => !byKey.has(entry.key));
  if (missing.length)
    return { result: 'full', reason: `the targeted rerun did not run ${missing.length} requested case(s)` };
  const failing = partial.failed.filter((entry) => byKey.get(entry.key).outcome !== 'pass');
  const recovered = partial.failed.filter((entry) => byKey.get(entry.key).outcome === 'pass');
  if (!failing.length && status === 0)
    return { result: 'passed', flaky: [...partial.flaky, ...recovered.map((entry) => entry.name)] };
  // Extra cases a location filter selected may fail too; they were not part of the reused passes.
  const extra = cases.filter(
    (entry) => entry.outcome === 'fail' && !partial.failed.some((wanted) => wanted.key === entry.key),
  );
  const stillFailing = [...failing, ...extra];
  if (!stillFailing.length) return { result: 'full', reason: 'the targeted rerun failed without a failed case' };
  const narrowed = narrowedCommand(partial.command, partial.kind, stillFailing);
  if (!narrowed) return { result: 'full', reason: 'the remaining failures cannot be rerun on their own' };
  return {
    result: 'retry',
    partial: {
      ...partial,
      narrowed,
      failed: stillFailing.map(({ key, name, selector }) => ({ key, name, selector })),
      flaky: [...partial.flaky, ...recovered.map((entry) => entry.name)],
    },
  };
}

export function readPartial(path, key, command) {
  if (!existsSync(path)) return null;
  try {
    const value = JSON.parse(readFileSync(path, 'utf8'));
    if (
      value?.version === VERSION &&
      value.key === key &&
      value.command === command &&
      Array.isArray(value.failed) &&
      value.failed.length &&
      typeof value.narrowed === 'string' &&
      Array.isArray(value.flaky)
    )
      return value;
  } catch {
    /* A corrupt record falls back to the whole stage. */
  }
  rmSync(path, { force: true });
  return null;
}

export function writePartial(path, partial) {
  writeFileSync(path, JSON.stringify(partial, null, 2) + '\n');
}
