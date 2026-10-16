#!/usr/bin/env node
// Slowest-test profile (HS2-3PX1E4): lists the slowest tests (and Vitest/Playwright files) from
// the reports each runner already writes, so slow tests are found from data, not guesses.
//
//   node scripts/test-profile.mjs [--top N] [--nextest <junit.xml>] [--vitest <report.json>] [--playwright <report.json>]
//
// Reports: nextest writes target/nextest/default/junit.xml on every run (.config/nextest.toml);
// Vitest with `--reporter=json --outputFile.json=<path>`; Playwright with `--reporter=json`
// (`PLAYWRIGHT_JSON_OUTPUT_NAME=<path>`).
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const decode = (text) =>
  text.replace(/&(lt|gt|quot|apos|amp);/g, (_, name) => ({ lt: "<", gt: ">", quot: '"', apos: "'", amp: "&" })[name]);

const attribute = (tag, name) => {
  const match = tag.match(new RegExp(`\\s${name}="([^"]*)"`));
  return match ? decode(match[1]) : undefined;
};

/** nextest JUnit: one entry per `<testcase>`, named `binary::test`, in seconds. */
export function nextestTimings(xml) {
  return [...xml.matchAll(/<testcase\b[^>]*>/g)].map(([tag]) => ({
    name: `${attribute(tag, "classname")} ${attribute(tag, "name")}`,
    seconds: Number(attribute(tag, "time") ?? 0),
  }));
}

/** Vitest JSON (Jest-compatible): per-test durations plus per-file wall time. */
export function vitestTimings(report, root = "") {
  const tests = [];
  const files = [];
  for (const file of report.testResults ?? []) {
    const name = root ? path.relative(root, file.name) : file.name;
    files.push({ name, seconds: Math.max(0, (file.endTime - file.startTime) / 1000) });
    for (const result of file.assertionResults ?? []) {
      tests.push({ name: `${name} > ${result.fullName ?? result.title}`, seconds: (result.duration ?? 0) / 1000 });
    }
  }
  return { tests, files };
}

/** Playwright JSON: per-test duration (the sum of its attempts) and per-file totals. */
export function playwrightTimings(report) {
  const tests = [];
  const perFile = new Map();
  const visit = (suite, titles) => {
    const nextTitles = suite.title && !suite.file?.endsWith(suite.title) ? [...titles, suite.title] : titles;
    for (const spec of suite.specs ?? []) {
      for (const test of spec.tests ?? []) {
        const ms = (test.results ?? []).reduce((sum, result) => sum + (result.duration ?? 0), 0);
        const project = test.projectName ? `[${test.projectName}] ` : "";
        tests.push({ name: `${project}${spec.file}:${spec.line} › ${[...nextTitles, spec.title].join(" › ")}`, seconds: ms / 1000 });
        perFile.set(spec.file, (perFile.get(spec.file) ?? 0) + ms);
      }
    }
    for (const child of suite.suites ?? []) visit(child, nextTitles);
  };
  for (const suite of report.suites ?? []) visit(suite, []);
  return { tests, files: [...perFile].map(([name, ms]) => ({ name, seconds: ms / 1000 })) };
}

export function slowest(entries, top) {
  return [...entries].sort((a, b) => b.seconds - a.seconds || a.name.localeCompare(b.name)).slice(0, top);
}

export function formatTable(title, entries, top) {
  const total = entries.reduce((sum, entry) => sum + entry.seconds, 0);
  const rows = slowest(entries, top).map((entry) => `${entry.seconds.toFixed(2).padStart(9)}s  ${entry.name}`);
  return [`${title}: ${entries.length} entries, ${total.toFixed(1)}s total; slowest ${rows.length}:`, ...rows].join("\n");
}

export function parseArgs(argv) {
  const options = { top: 15, nextest: [], vitest: [], playwright: [] };
  for (let i = 0; i < argv.length; i += 2) {
    const [flag, value] = [argv[i], argv[i + 1]];
    if (value === undefined) throw new Error(`${flag} needs a value`);
    if (flag === "--top") {
      options.top = Number.parseInt(value, 10);
      if (!Number.isInteger(options.top) || options.top < 1) throw new Error("--top needs a positive integer");
    } else if (["--nextest", "--vitest", "--playwright"].includes(flag)) {
      options[flag.slice(2)].push(value);
    } else {
      throw new Error(`unknown option: ${flag}`);
    }
  }
  if (!options.nextest.length && !options.vitest.length && !options.playwright.length) {
    throw new Error("name at least one report: --nextest, --vitest, or --playwright");
  }
  return options;
}

export function profile(options, read = (file) => fs.readFileSync(file, "utf8")) {
  const sections = [];
  for (const file of options.nextest) sections.push(formatTable(`nextest tests (${file})`, nextestTimings(read(file)), options.top));
  for (const file of options.vitest) {
    const { tests, files } = vitestTimings(JSON.parse(read(file)), process.cwd());
    sections.push(formatTable(`Vitest files (${file})`, files, options.top), formatTable(`Vitest tests (${file})`, tests, options.top));
  }
  for (const file of options.playwright) {
    const { tests, files } = playwrightTimings(JSON.parse(read(file)));
    sections.push(formatTable(`Playwright files (${file})`, files, options.top), formatTable(`Playwright tests (${file})`, tests, options.top));
  }
  return sections.join("\n\n");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    console.log(profile(parseArgs(process.argv.slice(2))));
  } catch (error) {
    console.error(`test-profile: ${error.message}`);
    process.exit(2);
  }
}
