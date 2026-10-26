import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { TIERS, MAX_LINE_CHARS, failureExcerpts, parseArgs, runTier, stripAnsi, summaryLines } from "./test-quiet.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
// Real runner output captured from deliberately failing nextest, Vitest, and Playwright runs.
const fixture = (name) => fs.readFileSync(path.join(here, "fixtures/test-quiet", name), "utf8");

test("nextest failure keeps one fuller block per test, ending at the panic", () => {
  const { excerpts, omitted } = failureExcerpts("rust", fixture("nextest-fail.txt"));
  assert.equal(omitted, 0);
  assert.equal(excerpts.length, 1, "the as-it-happens FAIL line and the final block are one failure");
  assert.match(excerpts[0], /^\s+FAIL \[.*t::fails_here/);
  assert.match(excerpts[0], /numbers differ/);
  assert.doesNotMatch(excerpts[0], /error: test run failed/);
  assert.deepEqual(summaryLines("rust", fixture("nextest-fail.txt")), [
    "Summary [   0.010s] 2 tests run: 1 passed, 1 failed, 0 skipped",
  ]);
});

test("nextest excerpts over the cap keep the head and the panic at the tail", () => {
  const { excerpts } = failureExcerpts("rust", fixture("nextest-fail.txt"), { maxLines: 6 });
  const lines = excerpts[0].split("\n");
  assert.equal(lines.length, 7, "six kept lines plus the elision marker");
  assert.match(lines[0], /FAIL/);
  assert.match(lines[2], /more lines in the log/);
  assert.match(excerpts[0], /RUST_BACKTRACE/);
});

test("vitest dot output yields the FAIL block up to the separator", () => {
  const { excerpts } = failureExcerpts("web-unit", fixture("vitest-fail.txt"));
  assert.equal(excerpts.length, 1);
  assert.match(excerpts[0], /^ FAIL {2}a\.test\.mjs > fails here/);
  assert.match(excerpts[0], /expected 1 to be 2/);
  assert.doesNotMatch(excerpts[0], /⎯|Test Files/);
  assert.deepEqual(summaryLines("web-unit", fixture("vitest-fail.txt")), [
    "Test Files  1 failed (1)",
    "Tests  1 failed | 1 passed (2)",
  ]);
});

test("playwright line output yields numbered failures, with or without a project", () => {
  const text = fixture("playwright-fail.txt");
  const { excerpts } = failureExcerpts("web-e2e", text);
  assert.equal(excerpts.length, 1);
  assert.match(excerpts[0], /1\) a\.spec\.mjs:3:1 › fails here/);
  assert.match(excerpts[0], /Received: 1/);
  assert.doesNotMatch(excerpts[0], /1 passed/);
  assert.deepEqual(summaryLines("web-e2e", text), ["1 failed", "1 passed (328ms)"]);
  const withProject = "  2) [chromium] › tests/x.spec.ts:9:3 › breaks ───\n\n    Error: boom\n\n  1 failed\n";
  assert.match(failureExcerpts("web-e2e", withProject).excerpts[0], /\[chromium\].*\n\n {4}Error: boom$/);
});

test("excerpt count is capped and the remainder is counted", () => {
  const text = Array.from({ length: 5 }, (_, i) => ` FAIL  f${i}.test.ts > t\nError ${i}\n⎯⎯⎯`).join("\n");
  const { excerpts, omitted } = failureExcerpts("migrator", text, { maxFailures: 2 });
  assert.deepEqual(excerpts, [" FAIL  f0.test.ts > t\nError 0", " FAIL  f1.test.ts > t\nError 1"]);
  assert.equal(omitted, 3);
});

test("ANSI colour codes are stripped before matching", () => {
  assert.equal(stripAnsi("\u001b[31mFAIL\u001b[39m"), "FAIL");
  const colored = "\u001b[1m\u001b[31m FAIL \u001b[39m\u001b[22m a.test.ts > t\nboom\n";
  assert.match(failureExcerpts("web-unit", colored).excerpts[0], /^ FAIL {2}a\.test\.ts > t\nboom$/);
});

test("argument parsing", () => {
  assert.deepEqual(parseArgs(["rust", "--", "-p", "hotsheet-cli"]), {
    tiers: ["rust"],
    maxFailures: 8,
    maxLines: 30,
    args: ["-p", "hotsheet-cli"],
  });
  assert.deepEqual(parseArgs(["rust,web-unit", "--max-lines", "5"]).tiers, ["rust", "web-unit"]);
  assert.equal(parseArgs(["web-e2e", "--max-failures", "3"]).maxFailures, 3);
  assert.throws(() => parseArgs([]), /name a tier/);
  assert.throws(() => parseArgs(["nope"]), /unknown tier/);
  assert.throws(() => parseArgs(["rust", "--max-lines", "0"]), /positive integer/);
  assert.throws(() => parseArgs(["rust", "--verbose"]), /unknown option/);
  assert.throws(() => parseArgs(["rust", "web-unit", "--", "-x"]), /exactly one tier/);
});

function tempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "hs2-test-quiet-"));
}

function node(script, cwd) {
  return { cwd, command: process.execPath, args: ["-e", script] };
}

test("a passing tier prints one line and writes the full output to the log", async () => {
  const logDir = tempDir();
  const step = node("console.log('noise'.repeat(3)); console.log('     Summary [   1.0s] 3 tests run: 3 passed')", logDir);
  const { code, log, report } = await runTier("rust", { logDir, steps: [step] });
  assert.equal(code, 0);
  assert.equal(report, `rust: PASS in ${report.match(/in ([\d.]+)s/)[1]}s | Summary [   1.0s] 3 tests run: 3 passed | log: ${log}`);
  assert.match(fs.readFileSync(log, "utf8"), /noisenoisenoise/);
});

test("a failing tier reports excerpts from real runner output and keeps the exit code", async () => {
  const logDir = tempDir();
  const fixturePath = path.join(here, "fixtures/test-quiet/vitest-fail.txt");
  const step = node(`process.stdout.write(require('fs').readFileSync(${JSON.stringify(fixturePath)}, 'utf8')); process.exit(1)`, logDir);
  const { code, report } = await runTier("web-unit", { logDir, steps: [step] });
  assert.equal(code, 1);
  assert.match(report, /^web-unit: FAIL in [\d.]+s \| Test Files {2}1 failed \(1\) \| Tests {2}1 failed \| 1 passed \(2\) \| log: /);
  assert.match(report, /FAIL {2}a\.test\.mjs > fails here\nAssertionError/);
  assert.doesNotMatch(report, /stdout \| a\.test\.mjs/, "test console output stays in the log");
});

test("a failed build step stops the tier and falls back to the output tail", async () => {
  const logDir = tempDir();
  const build = node("console.error('error[E0425]: cannot find value'); process.exit(101)", logDir);
  const tests = node("require('fs').writeFileSync('ran', '')", logDir);
  const { code, report } = await runTier("web-e2e", { logDir, steps: [build, tests] });
  assert.equal(code, 101);
  assert.match(report, /no test failure found; exit 101/);
  assert.match(report, /error\[E0425\]/);
  assert.equal(fs.existsSync(path.join(logDir, "ran")), false, "tests do not run after a failed build");
});

test("excerpt and fallback lines are clipped to a readable width", async () => {
  const long = "x".repeat(MAX_LINE_CHARS + 50);
  assert.equal(failureExcerpts("web-unit", ` FAIL  a.test.ts > t\n${long}\n`).excerpts[0].split("\n")[1], `${"x".repeat(MAX_LINE_CHARS)} …`);
  const logDir = tempDir();
  const step = node(`console.log(${JSON.stringify(long)}); process.exit(3)`, logDir);
  const { report, log } = await runTier("rust", { logDir, steps: [step] });
  assert.match(report, new RegExp(`^x{${MAX_LINE_CHARS}} …$`, "m"));
  assert.match(fs.readFileSync(log, "utf8"), new RegExp(`x{${MAX_LINE_CHARS + 50}}`), "the log keeps the full line");
});

test("a missing runner is reported, not thrown", async () => {
  const logDir = tempDir();
  const { code, report } = await runTier("migrator", {
    logDir,
    steps: [{ cwd: logDir, command: "hs2-no-such-runner", args: [] }],
  });
  assert.equal(code, 127);
  assert.match(report, /hs2-no-such-runner: spawn hs2-no-such-runner ENOENT/);
});

test("the command line rejects bad usage with exit 2", () => {
  const result = spawnSync(process.execPath, [path.join(here, "test-quiet.mjs"), "bogus"], { encoding: "utf8" });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /unknown tier: bogus\nusage:/);
});

test("HS2-SFVWN5 Rust and browser tiers build an isolated fixture before execution", () => {
  for (const tier of ["rust", "web-e2e"]) {
    const steps = TIERS[tier].steps([]);
    assert.equal(steps[0].command, "cargo");
    assert.equal(steps[0].args[0], "build");
    assert.ok(steps[0].args.includes("hotsheet-aitools/test-permission-vault"));
    assert.ok(steps.length > 1);
  }
});
