#!/usr/bin/env node
// HS2-HW6VED: the browser engines a Playwright command actually runs, derived from source.
//
// `clients/web/playwright.config.ts` declares no `projects` and no `use.browserName`, so a
// command without `--browser` runs every selected test in Chromium. WebKit runs only where a
// spec launches it itself (`webkit.launch()`), or where a command passes `--browser=webkit`.
// Gate plans, receipts and CI evidence report these facts so a handoff cannot describe a
// focused WebKit group as a full engine matrix. The CI matrix below is pinned to the table in
// docs/12-code-organization-and-testing.md by scripts/browser-engines.test.mjs.
//
//   node scripts/browser-engines.mjs            # the CI browser matrix as JSON
//   node scripts/browser-engines.mjs --markdown # the documented matrix table
//   node scripts/browser-engines.mjs --report <playwright-report.json> [--summary <file>]
import { appendFileSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const ENGINES = ['chromium', 'firefox', 'webkit'];
export const WORKFLOWS = ['.github/workflows/ci.yml', '.github/workflows/platforms.yml'];

/**
 * The engine a test runs in when its command names none. A config that gains `projects` or
 * a `browserName` is refused rather than guessed, so this report cannot silently go stale.
 */
export function defaultEngine(configText) {
  if (/\bprojects\s*:/.test(configText))
    throw new Error('playwright.config.ts declares projects; teach scripts/browser-engines.mjs to read them.');
  const named = /\bbrowserName\s*:\s*'(\w+)'/.exec(configText);
  if (named && !ENGINES.includes(named[1])) throw new Error(`Unknown Playwright browserName: ${named[1]}`);
  return named?.[1] ?? 'chromium';
}

/**
 * What one spec does about engines: the default it overrides with `test.use({ browserName })`,
 * the engines it launches itself, and the environment opt-in that otherwise skips it.
 */
export function specEngines(text) {
  const used = /^test\.use\(\{\s*browserName:\s*'(\w+)'/m.exec(text)?.[1] ?? null;
  const launched = [];
  const imports = /import\s*\{([^}]*)\}\s*from\s*'@playwright\/test'/.exec(text)?.[1] ?? '';
  const names = imports
    .split(',')
    .map((name) => name.trim())
    .filter((name) => !name.startsWith('type '));
  if (/\.launch\(/.test(text)) for (const engine of ENGINES) if (names.includes(engine)) launched.push(engine);
  // A spec is opt-in when a file-level skip gates it on an environment flag, or when every
  // test it declares carries such a skip. One gated test inside a larger spec does not count.
  const fileLevel = [];
  const perTest = [];
  const lines = text.split('\n');
  lines.forEach((line, index) => {
    if (!/\btest\.(?:describe\.)?skip\(/.test(line)) return;
    const window = lines.slice(index, index + 3).join(' ');
    const flags = [...window.matchAll(/process\.env\.(HOTSHEET_\w+)\s*!==\s*'1'/g)].map((match) => match[1]);
    if (!flags.length) return;
    (/^test\./.test(line) ? fileLevel : perTest).push(...flags);
  });
  const declared = lines.filter((line) => /^\s*test\((?:'|`|")/.test(line)).length;
  const optIn = fileLevel.length ? fileLevel : declared > 0 && perTest.length >= declared ? perTest : [];
  return { use: used, launched, optIn: [...new Set(optIn)] };
}

/** Every spec under the Playwright `testDir`, keyed by its package-relative path. */
export function specIndex(webRoot) {
  const index = {};
  const walk = (directory) => {
    for (const entry of readdirSync(join(webRoot, directory), { withFileTypes: true })) {
      const path = `${directory}/${entry.name}`;
      if (entry.isDirectory()) walk(path);
      else if (entry.name.endsWith('.spec.ts')) index[path] = specEngines(readFileSync(join(webRoot, path), 'utf8'));
    }
  };
  walk('tests');
  return index;
}

/** Shell-like words with single and double quotes removed; enough for the gate's own commands. */
export function words(command) {
  const result = [];
  for (const match of command.matchAll(/'([^']*)'|"([^"]*)"|(\S+)/g)) result.push(match[1] ?? match[2] ?? match[3]);
  return result;
}

/**
 * The Playwright invocation inside a command, resolving `npm run <script>` through package.json.
 * Returns null for a command that runs no Playwright tests (for example `merge-reports`).
 */
export function playwrightInvocation(command, scripts = {}) {
  const npm = /\bnpm run ([\w:.-]+)/.exec(command);
  if (npm && !/\bplaywright test\b/.test(command)) {
    const script = scripts[npm[1]];
    return script === undefined ? null : playwrightInvocation(script, scripts);
  }
  const start = command.search(/\b(?:npx )?playwright test\b/);
  if (start < 0) return null;
  const tail = command.slice(start).split(/\s*(?:&&|\|\||;)\s*/)[0];
  return tail.replace(/^npx /, '');
}

/**
 * The engines and selection of one Playwright invocation. `files` is null for the whole suite.
 * `specLaunched` lists the non-default engines that selected, non-opt-in specs launch themselves.
 */
export function commandCoverage(command, { engine, specs, scripts = {} }) {
  const invocation = playwrightInvocation(command, scripts);
  if (!invocation) return null;
  const argv = words(invocation).slice(2);
  let browser = null;
  const files = [];
  const grep = [];
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--project' || arg.startsWith('--project='))
      throw new Error(`Playwright projects are not configured: ${command}`);
    if (arg.startsWith('--browser=')) browser = arg.slice('--browser='.length);
    else if (arg === '--browser') browser = argv[++index];
    else if (arg === '--grep' || arg === '-g') grep.push(argv[++index]);
    else if (arg.startsWith('--grep=')) grep.push(arg.slice('--grep='.length));
    else if (/^tests\/.*\.spec\.ts$/.test(arg)) files.push(arg);
  }
  if (browser !== null && !ENGINES.includes(browser) && browser !== 'all')
    throw new Error(`Unknown --browser value in: ${command}`);
  const selected = files.length ? files : Object.keys(specs).sort();
  const defaultEngines = new Set();
  const specLaunched = {};
  const optIn = [];
  for (const file of selected) {
    const spec = specs[file];
    if (!spec) continue;
    if (spec.optIn.length) {
      optIn.push(file);
      continue;
    }
    const base = browser ?? spec.use ?? engine;
    if (base === 'all') ENGINES.forEach((name) => defaultEngines.add(name));
    else defaultEngines.add(base);
    for (const launched of spec.launched) if (launched !== base) (specLaunched[launched] ??= []).push(file);
  }
  return {
    defaultEngine: browser ?? engine,
    engines: [...new Set([...defaultEngines, ...Object.keys(specLaunched)])].sort(),
    files: files.length ? files.sort() : null,
    grep,
    specLaunched,
    optIn,
  };
}

/**
 * `name` and `run` of each step, grouped by job, from a GitHub workflow's plain YAML text. A
 * block scalar (`run: |`) yields one command per logical line, joining `\` continuations.
 */
export function workflowRuns(text) {
  const runs = [];
  let inJobs = false;
  let job = null;
  let step = null;
  let block = null;
  let pending = '';
  for (const line of text.split('\n')) {
    if (block) {
      const indent = /^ */.exec(line)[0].length;
      if (!line.trim() || indent > block.indent) {
        const command = line.trim();
        if (!command) continue;
        pending = pending ? `${pending} ${command}` : command;
        if (pending.endsWith('\\')) pending = pending.slice(0, -1).trimEnd();
        else {
          runs.push({ job, step, run: pending });
          pending = '';
        }
        continue;
      }
      if (pending) runs.push({ job, step, run: pending });
      [block, pending] = [null, ''];
    }
    if (/^jobs:\s*$/.test(line)) inJobs = true;
    else if (/^\S/.test(line)) inJobs = false;
    if (!inJobs) continue;
    const jobKey = /^ {2}([\w-]+):\s*$/.exec(line);
    if (jobKey) [job, step] = [jobKey[1], null];
    // Each `- ` item under `steps:` starts a new step, named or not.
    if (/^\s+- /.test(line)) step = /^\s+- name:\s*(.+)$/.exec(line)?.[1].trim() ?? null;
    const run = /^(\s+)(?:- )?run:\s*(.+)$/.exec(line);
    if (!run || !job) continue;
    if (/^[|>][-+]?$/.test(run[2].trim())) block = { indent: run[1].length };
    else runs.push({ job, step, run: run[2].trim() });
  }
  if (block && pending) runs.push({ job, step, run: pending });
  return runs;
}

/** The browser matrix of the checked-in workflows: every step that runs Playwright tests. */
export function ciBrowserMatrix(repo = root) {
  const webRoot = join(repo, 'clients/web');
  const engine = defaultEngine(readFileSync(join(webRoot, 'playwright.config.ts'), 'utf8'));
  const specs = specIndex(webRoot);
  const { scripts } = JSON.parse(readFileSync(join(webRoot, 'package.json'), 'utf8'));
  const rows = [];
  for (const workflow of WORKFLOWS) {
    for (const { job, step, run } of workflowRuns(readFileSync(join(repo, workflow), 'utf8'))) {
      const coverage = commandCoverage(run, { engine, specs, scripts });
      if (!coverage) continue;
      rows.push({ workflow: workflow.replace('.github/workflows/', ''), job, step, command: run, ...coverage });
    }
  }
  return rows;
}

const short = (file) => file.replace(/^tests\//, '').replace(/\.spec\.ts$/, '');

/** One-line selection: the whole suite, or the named files and grep. */
export function selectionLabel(row) {
  const files = row.files ? row.files.map(short).join(', ') : 'all specs';
  return row.grep.length ? `${files}; grep ${row.grep.join(' | ')}` : files;
}

/** Spec-launched engines, for example `webkit: browser-ids-lan, terminal-safari`. */
export function launchedLabel(row) {
  const entries = Object.entries(row.specLaunched).sort(([a], [b]) => a.localeCompare(b));
  return entries.length
    ? entries.map(([name, files]) => `${name}: ${files.map(short).sort().join(', ')}`).join('; ')
    : 'none';
}

/** The documented table body: Workflow, Job, Default engine, Selection, Spec-launched engines. */
export function matrixTable(rows) {
  return [
    '| Workflow | Job | Default engine | Selection | Spec-launched engines |',
    '| --- | --- | --- | --- | --- |',
    ...rows.map(
      (row) =>
        `| \`${row.workflow}\` | \`${row.job}\` | ${row.defaultEngine} | ${selectionLabel(row)} | ${launchedLabel(row)} |`,
    ),
  ].join('\n');
}

/**
 * HS2-HW6VED gate evidence: the engines each Playwright stage runs, and an exact sentence a
 * handoff can quote. `stages` are gate stages (`cwd`, `command`); non-browser stages are ignored.
 */
export function gateBrowserCoverage(stages, repo = root) {
  const webRoot = join(repo, 'clients/web');
  const engine = defaultEngine(readFileSync(join(webRoot, 'playwright.config.ts'), 'utf8'));
  const specs = specIndex(webRoot);
  const { scripts } = JSON.parse(readFileSync(join(webRoot, 'package.json'), 'utf8'));
  const annotated = [];
  for (const stage of stages) {
    if (stage.cwd !== 'clients/web') continue;
    const coverage = commandCoverage(stage.command, { engine, specs, scripts });
    if (coverage) annotated.push({ command: stage.command, ...coverage });
  }
  return { stages: annotated, summary: coverageSummary(annotated) };
}

/** A plain statement of which engines ran which selection. */
export function coverageSummary(rows) {
  if (!rows.length) return 'no Playwright tests';
  const byEngine = {};
  for (const row of rows) {
    (byEngine[row.defaultEngine] ??= []).push(selectionLabel(row));
    for (const [name, files] of Object.entries(row.specLaunched))
      (byEngine[name] ??= []).push(`spec-launched in ${files.map(short).join(', ')}`);
  }
  return Object.entries(byEngine)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([name, selections]) => `${name}: ${[...new Set(selections)].join('; ')}`)
    .join(' / ');
}

/**
 * The engines a finished run actually used, from its Playwright JSON report. A test counts as
 * run unless its final status is `skipped`. Tests run in the default engine of their spec; files
 * that launch another engine themselves are listed with their run counts, because which of their
 * tests open that second browser is decided at run time.
 */
export function reportEngines(report, { engine, specs }) {
  const configured = report.config?.projects?.map((project) => project.use?.browserName).filter(Boolean) ?? [];
  if (configured.length) throw new Error('The report names project browsers; teach browser-engines.mjs to read them.');
  const byEngine = {};
  const launched = {};
  const visit = (suite) => {
    for (const spec of suite.specs ?? []) {
      const file = `tests/${spec.file}`;
      const info = specs[file] ?? { use: null, launched: [], optIn: [] };
      const base = info.use ?? engine;
      for (const test of spec.tests ?? []) {
        const ran = test.status !== 'skipped';
        const counts = (byEngine[base] ??= { ran: 0, skipped: 0 });
        counts[ran ? 'ran' : 'skipped'] += 1;
        if (!ran) continue;
        for (const name of info.launched.filter((other) => other !== base))
          ((launched[name] ??= {})[file] ??= { ran: 0 }).ran += 1;
      }
    }
    for (const child of suite.suites ?? []) visit(child);
  };
  for (const suite of report.suites ?? []) visit(suite);
  return { byEngine, launched };
}

/** The job-summary section for a run's actual engines. */
export function formatReportSummary({ byEngine, launched }) {
  const lines = [
    '## Browser engines in this run (HS2-HW6VED)',
    '',
    '| Engine | Tests run | Tests skipped | How |',
    '| --- | --- | --- | --- |',
  ];
  for (const [name, counts] of Object.entries(byEngine).sort(([a], [b]) => a.localeCompare(b)))
    lines.push(`| ${name} | ${counts.ran} | ${counts.skipped} | default engine |`);
  for (const [name, files] of Object.entries(launched).sort(([a], [b]) => a.localeCompare(b))) {
    const ran = Object.values(files).reduce((sum, file) => sum + file.ran, 0);
    lines.push(
      `| ${name} | ${ran} in launching files | - | spec-launched in ${Object.keys(files).map(short).sort().join(', ')} |`,
    );
  }
  lines.push(
    '',
    'Only the default engine runs the whole selection; a spec-launched engine covers only the listed files.',
    '',
  );
  return lines.join('\n');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const option = (name) => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined);
  const reportPath = option('--report');
  if (reportPath) {
    const webRoot = join(root, 'clients/web');
    const result = reportEngines(JSON.parse(readFileSync(resolve(reportPath), 'utf8')), {
      engine: defaultEngine(readFileSync(join(webRoot, 'playwright.config.ts'), 'utf8')),
      specs: specIndex(webRoot),
    });
    const text = formatReportSummary(result);
    const summary = option('--summary');
    if (summary) appendFileSync(summary, text);
    console.log(text);
  } else {
    const rows = ciBrowserMatrix();
    console.log(args.includes('--markdown') ? matrixTable(rows) : JSON.stringify(rows, null, 2));
  }
}
