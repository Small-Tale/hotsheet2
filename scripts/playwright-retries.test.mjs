import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { formatAnnotations, formatSummary, parseArgs, retriedTests, run } from "./playwright-retries.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
// A real Playwright 1.63 JSON report (retries: 2): one test passes on its retry, one fails every
// attempt, one passes first time, and one is skipped.
const fixture = path.join(here, "fixtures/playwright-retries/playwright.json");
const report = () => JSON.parse(fs.readFileSync(fixture, "utf8"));

test("lists only the tests that passed after a retry", () => {
  assert.deepEqual(retriedTests(report()), [
    { project: "", location: "a.spec.mjs:4", title: "tabs › passes after one retry", attempts: 2, failures: 1 },
  ]);
});

test("a run with no retries and an empty report list nothing", () => {
  const clean = report();
  clean.suites[0].suites = [];
  assert.deepEqual(retriedTests(clean), []);
  assert.deepEqual(retriedTests({}), []);
});

test("names the project and escapes table cells", () => {
  const retried = retriedTests({
    suites: [
      {
        title: "x.spec.ts",
        file: "x.spec.ts",
        specs: [
          {
            title: "a | b",
            file: "x.spec.ts",
            line: 7,
            tests: [{ projectName: "webkit", status: "flaky", results: [{ status: "timedOut" }, { status: "failed" }, { status: "passed" }] }],
          },
        ],
      },
    ],
  });
  assert.deepEqual(retried, [{ project: "webkit", location: "x.spec.ts:7", title: "a | b", attempts: 3, failures: 2 }]);
  assert.match(formatSummary(retried), /\| `x\.spec\.ts:7` \| a \\\| b \| webkit \| 2 of 3 \|/);
  assert.match(formatSummary([...retried, ...retried]), /2 tests failed and then passed on a retry/);
});

test("the summary says so when nothing needed a retry", () => {
  assert.match(formatSummary([]), /None: every test passed on its first attempt/);
});

test("the summary and annotations list every retried test", () => {
  const retried = retriedTests(report());
  const summary = formatSummary(retried);
  assert.match(summary, /1 test failed and then passed on a retry/);
  assert.match(summary, /\| `a\.spec\.mjs:4` \| tabs › passes after one retry \| default \| 1 of 2 \|/);
  assert.deepEqual(formatAnnotations(retried), [
    "::warning title=Browser test needed a retry::a.spec.mjs:4 › tabs › passes after one retry failed 1 of 2 attempts before passing",
  ]);
});

test("parses the report and the optional summary file", () => {
  assert.deepEqual(parseArgs(["r.json"]), { report: "r.json", summary: undefined });
  assert.deepEqual(parseArgs(["r.json", "--summary", "s.md"]), { report: "r.json", summary: "s.md" });
  assert.throws(() => parseArgs([]), /usage/);
  assert.throws(() => parseArgs(["r.json", "--summary"]), /needs a file/);
  assert.throws(() => parseArgs(["a.json", "b.json"]), /unexpected argument/);
});

test("run appends the summary and logs one warning per retried test", () => {
  const appended = [];
  const logged = [];
  run([fixture, "--summary", "summary.md"], { append: (file, text) => appended.push([file, text]), log: (line) => logged.push(line) });
  assert.equal(appended.length, 1);
  assert.equal(appended[0][0], "summary.md");
  assert.match(appended[0][1], /tabs › passes after one retry/);
  assert.equal(logged.length, 2);
  assert.match(logged[0], /^::warning /);
  assert.equal(logged[1], "1 browser test needed a retry.");
});

test("the CLI writes the job summary and exits 0", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "playwright-retries-"));
  const summary = path.join(dir, "summary.md");
  const result = spawnSync(process.execPath, [path.join(here, "playwright-retries.mjs"), fixture, "--summary", summary], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /::warning title=Browser test needed a retry::a\.spec\.mjs:4/);
  assert.match(fs.readFileSync(summary, "utf8"), /## Browser tests that needed a retry/);
  fs.rmSync(dir, { recursive: true, force: true });
});
