#!/usr/bin/env node
// Lists the Playwright tests that needed a retry to pass (HS2-8YHY6V).
//
//   node scripts/playwright-retries.mjs <report.json> [--summary <file>]
//
// The full browser suite in CI (`browser-full`) retries a failed test up to twice. A test that passes
// on a retry is reported `flaky`: it does not fail the run, but it is not healthy. This prints each one
// as a GitHub Actions warning and, with `--summary` (CI passes `$GITHUB_STEP_SUMMARY`), appends a
// Markdown table so the job summary keeps every retried test visible. Each listed test needs a
// follow-up ticket (docs/test-quarantine.md). It always exits 0; failures still fail the run.

import fs from "node:fs";
import { fileURLToPath } from "node:url";

/** Playwright JSON report: every test whose final status is `flaky` (it failed, then passed on a retry). */
export function retriedTests(report) {
  const retried = [];
  const visit = (suite, titles) => {
    const nextTitles = suite.title && !suite.file?.endsWith(suite.title) ? [...titles, suite.title] : titles;
    for (const spec of suite.specs ?? []) {
      for (const test of spec.tests ?? []) {
        if (test.status !== "flaky") continue;
        const results = test.results ?? [];
        retried.push({
          project: test.projectName ?? "",
          location: `${spec.file}:${spec.line}`,
          title: [...nextTitles, spec.title].join(" › "),
          attempts: results.length,
          failures: results.filter((result) => result.status !== "passed" && result.status !== "skipped").length,
        });
      }
    }
    for (const child of suite.suites ?? []) visit(child, nextTitles);
  };
  for (const suite of report.suites ?? []) visit(suite, []);
  return retried;
}

const cell = (text) => text.replaceAll("|", "\\|");

/** The job summary's Markdown section. */
export function formatSummary(retried) {
  if (!retried.length) return "## Browser tests that needed a retry\n\nNone: every test passed on its first attempt.\n";
  const rows = retried.map(
    (test) =>
      `| \`${cell(test.location)}\` | ${cell(test.title)} | ${cell(test.project || "default")} | ${test.failures} of ${test.attempts} |`,
  );
  return [
    "## Browser tests that needed a retry",
    "",
    `${retried.length} test${retried.length === 1 ? "" : "s"} failed and then passed on a retry. Each needs a follow-up ticket (docs/test-quarantine.md).`,
    "",
    "| Location | Test | Project | Failed attempts |",
    "| --- | --- | --- | --- |",
    ...rows,
    "",
  ].join("\n");
}

/** One GitHub Actions warning annotation per retried test. */
export function formatAnnotations(retried) {
  return retried.map(
    (test) =>
      `::warning title=Browser test needed a retry::${test.location} › ${test.title} failed ${test.failures} of ${test.attempts} attempts before passing`,
  );
}

export function parseArgs(argv) {
  const options = { report: undefined, summary: undefined };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--summary") {
      options.summary = argv[++index];
      if (!options.summary) throw new Error("--summary needs a file");
    } else if (!options.report) {
      options.report = arg;
    } else {
      throw new Error(`unexpected argument: ${arg}`);
    }
  }
  if (!options.report) throw new Error("usage: playwright-retries.mjs <report.json> [--summary <file>]");
  return options;
}

export function run(argv, { read = (file) => fs.readFileSync(file, "utf8"), append = fs.appendFileSync, log = console.log } = {}) {
  const options = parseArgs(argv);
  const retried = retriedTests(JSON.parse(read(options.report)));
  for (const line of formatAnnotations(retried)) log(line);
  log(`${retried.length} browser test${retried.length === 1 ? "" : "s"} needed a retry.`);
  if (options.summary) append(options.summary, `${formatSummary(retried)}\n`);
  return retried;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) run(process.argv.slice(2));
