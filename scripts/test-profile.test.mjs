import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { formatTable, nextestTimings, parseArgs, playwrightTimings, profile, slowest, vitestTimings } from "./test-profile.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
// Reports written by real nextest 0.9.114, Vitest 4.1.11, and Playwright 1.63 runs.
const fixture = (name) => path.join(here, "fixtures/test-profile", name);
const read = (name) => fs.readFileSync(fixture(name), "utf8");

test("reads nextest JUnit testcases", () => {
  assert.deepEqual(nextestTimings(read("nextest-junit.xml")), [
    { name: "hotsheet-plugins setup::tests::a_single_tool_in_a_clean_file_keeps_its_per_tool_section", seconds: 0.029 },
    { name: "hotsheet-plugins setup::tests::bundled_tool_skills_match_their_canonical_shared_workflows", seconds: 0.031 },
    { name: "hotsheet-plugins setup::tests::a_disabled_tool_never_removes_a_target_an_enabled_tool_declares", seconds: 0.055 },
  ]);
  assert.deepEqual(nextestTimings('<testcase name="a &amp; b" classname="x" time="1.5"/>'), [{ name: "x a & b", seconds: 1.5 }]);
});

test("reads Vitest per-test and per-file durations", () => {
  const { tests, files } = vitestTimings(JSON.parse(read("vitest.json")), "/root");
  assert.deepEqual(
    tests.map((entry) => entry.name),
    ["a.test.mjs > passes", "a.test.mjs > fails here"],
  );
  assert.ok(Math.abs(tests[1].seconds - 0.0034134) < 1e-6);
  assert.equal(files.length, 1);
  assert.equal(files[0].name, "a.test.mjs");
  assert.ok(files[0].seconds > 0);
});

test("reads Playwright tests with their attempts summed per file", () => {
  const { tests, files } = playwrightTimings(JSON.parse(read("playwright.json")));
  assert.deepEqual(tests, [
    { name: "a.spec.mjs:2 › passes", seconds: 0.001 },
    { name: "a.spec.mjs:3 › fails here", seconds: 0.008 },
  ]);
  assert.deepEqual(files, [{ name: "a.spec.mjs", seconds: 0.009 }]);
  const retried = {
    suites: [
      {
        title: "x.spec.ts",
        file: "x.spec.ts",
        suites: [
          {
            title: "group",
            file: "x.spec.ts",
            specs: [{ title: "flaky", file: "x.spec.ts", line: 4, tests: [{ projectName: "chromium", results: [{ duration: 1000 }, { duration: 500 }] }] }],
          },
        ],
      },
    ],
  };
  assert.deepEqual(playwrightTimings(retried).tests, [{ name: "[chromium] x.spec.ts:4 › group › flaky", seconds: 1.5 }]);
});

test("ranks the slowest entries and formats a summary table", () => {
  const entries = [
    { name: "b", seconds: 1 },
    { name: "a", seconds: 1 },
    { name: "c", seconds: 3 },
  ];
  assert.deepEqual(slowest(entries, 2).map((entry) => entry.name), ["c", "a"]);
  assert.equal(formatTable("tier", entries, 2), "tier: 3 entries, 5.0s total; slowest 2:\n     3.00s  c\n     1.00s  a");
});

test("argument parsing", () => {
  assert.deepEqual(parseArgs(["--nextest", "j.xml", "--top", "3"]), { top: 3, nextest: ["j.xml"], vitest: [], playwright: [] });
  assert.throws(() => parseArgs([]), /at least one report/);
  assert.throws(() => parseArgs(["--top", "0", "--vitest", "v"]), /positive integer/);
  assert.throws(() => parseArgs(["--vitest"]), /needs a value/);
  assert.throws(() => parseArgs(["--junit", "x"]), /unknown option/);
});

test("profiles every named report", () => {
  const text = profile({ top: 1, nextest: [fixture("nextest-junit.xml")], vitest: [], playwright: [fixture("playwright.json")] });
  assert.match(text, /^nextest tests .*: 3 entries, 0\.1s total; slowest 1:\n {5}0\.06s {2}hotsheet-plugins setup::tests::a_disabled_tool/);
  assert.match(text, /Playwright files .*\n {5}0\.01s {2}a\.spec\.mjs/);
});

test("the command line prints the profile and rejects bad usage", () => {
  const script = path.join(here, "test-profile.mjs");
  const ok = spawnSync(process.execPath, [script, "--vitest", fixture("vitest.json")], { encoding: "utf8" });
  assert.equal(ok.status, 0, ok.stderr);
  assert.match(ok.stdout, /^Vitest files .*: 1 entries/);
  const bad = spawnSync(process.execPath, [script], { encoding: "utf8" });
  assert.equal(bad.status, 2);
  assert.match(bad.stderr, /test-profile: name at least one report/);
});
