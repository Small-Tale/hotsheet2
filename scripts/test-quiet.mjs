#!/usr/bin/env node
// Quiet test wrapper (HS2-CP63FG): runs a test tier with quiet diagnostic reporters, writes the
// complete output to a log file, and prints one summary line per tier plus capped failure
// excerpts. Agents read the summary instead of the full runner output.
//
//   node scripts/test-quiet.mjs <tier>[,<tier>...] [--max-failures N] [--max-lines N] [-- runner args]
//
// Tiers: rust (cargo nextest), web-unit (clients/web Vitest), web-e2e (clients/web binary build +
// Playwright), migrator (migrator Vitest). Runner args after `--` go to the single named tier,
// for example `node scripts/test-quiet.mjs rust -- -p hotsheet-cli -E 'test(claim)'`.
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export const DEFAULT_MAX_FAILURES = 8;
export const DEFAULT_MAX_LINES = 30;
const FALLBACK_TAIL_LINES = 30;
/** Compiler command lines can run to thousands of characters; excerpts clip each line. */
export const MAX_LINE_CHARS = 240;

function clip(line) {
  return line.length <= MAX_LINE_CHARS ? line : `${line.slice(0, MAX_LINE_CHARS)} …`;
}

/** Runner definitions: commands, failure-block boundaries, and summary-line patterns. */
export const TIERS = {
  rust: {
    steps: (args) => [
      {
        cwd: REPO,
        command: "cargo",
        args: ["build", "--quiet", "-p", "hotsheet-cli", "-p", "hotsheet-server", "-p", "hotsheet-terminals", "--bins", "--features", "hotsheet-aitools/test-permission-vault"],
      },
      {
        cwd: REPO,
        command: "cargo",
        args: [
          "nextest",
          "run",
          "--hide-progress-bar",
          "--status-level",
          "leak",
          "--final-status-level",
          "fail",
          "--failure-output",
          "final",
          "--success-output",
          "never",
          ...args,
        ],
      },
    ],
    failureStart: /^\s*(FAIL|TIMEOUT|SIGSEGV|SIGABRT|SIGKILL|LEAK-FAIL|ABORT)\s+\[/,
    failureEnd: /^\s*(LEAK\s+\[|Summary \[|error: )/,
    // libtest prints captured stdout before stderr, so the panic message sits at the end.
    keep: "head-tail",
    // HS2-ET5FF1: a nonfailing leak is named diagnostic metadata, never a failure body.
    summary: [/^\s*LEAK\s+\[.*$/, /^\s*Summary \[.*$/],
  },
  "web-unit": {
    steps: (args) => [
      { cwd: path.join(REPO, "clients/web"), command: "npx", args: ["vitest", "run", "--reporter=dot", ...args] },
    ],
    failureStart: /^\s*FAIL\s+\S/,
    failureEnd: /^\s*(⎯{3,}|Test Files\s)/,
    summary: [/^\s*Test Files\s.*$/, /^\s*Tests\s+\d.*$/],
  },
  "web-e2e": {
    steps: (args) => [
      {
        cwd: REPO,
        command: "cargo",
        args: ["build", "--quiet", "-p", "hotsheet-cli", "-p", "hotsheet-server", "--bins", "--features", "hotsheet-aitools/test-permission-vault"],
      },
      { cwd: path.join(REPO, "clients/web"), command: "npx", args: ["playwright", "test", "--reporter=line", ...args] },
    ],
    failureStart: /^\s+\d+\) \S/,
    failureEnd: /^\s+\d+ (failed|flaky|passed|skipped|interrupted|did not run)\b/,
    summary: [/^\s+\d+ (failed|flaky|passed|skipped|interrupted|did not run)\b.*$/],
  },
  migrator: {
    steps: (args) => [
      { cwd: path.join(REPO, "migrator"), command: "npx", args: ["vitest", "run", "--reporter=dot", ...args] },
    ],
    failureStart: /^\s*FAIL\s+\S/,
    failureEnd: /^\s*(⎯{3,}|Test Files\s)/,
    summary: [/^\s*Test Files\s.*$/, /^\s*Tests\s+\d.*$/],
  },
};

// eslint-disable-next-line no-control-regex
const ANSI = /\u001b\[[0-9;?]*[ -/]*[@-~]/g;

export function stripAnsi(text) {
  return text.replace(ANSI, "");
}

/**
 * Split runner output into failure excerpts. Each excerpt starts at a `failureStart` line and
 * runs to the next start or `failureEnd` line, capped at `maxLines`; at most `maxFailures`
 * distinct excerpts are returned, with `omitted` counting the rest. Over-long excerpts keep their
 * head, or for `keep: "head-tail"` tiers their first third and the end.
 */
export function failureExcerpts(tier, text, { maxFailures = DEFAULT_MAX_FAILURES, maxLines = DEFAULT_MAX_LINES } = {}) {
  const spec = TIERS[tier];
  const lines = stripAnsi(text).split(/\r?\n/).map(clip);
  const blocks = [];
  let current = null;
  for (const line of lines) {
    if (spec.failureStart.test(line)) {
      current = [line];
      blocks.push(current);
    } else if (current && spec.failureEnd.test(line)) {
      current = null;
    } else if (current) {
      current.push(line);
    }
  }
  // nextest names a failure twice (as it happens and in the final list); keep the fuller block.
  const unique = new Map();
  for (const block of blocks) {
    while (block.length > 1 && block.at(-1).trim() === "") block.pop();
    const key = block[0].trim();
    if ((unique.get(key)?.length ?? 0) < block.length) unique.set(key, block);
  }
  const distinct = [...unique.values()];
  const excerpts = distinct.slice(0, maxFailures).map((block) => {
    if (block.length <= maxLines) return block.join("\n");
    const cut = `  … ${block.length - maxLines} more lines in the log`;
    if (spec.keep !== "head-tail") return [...block.slice(0, maxLines), cut].join("\n");
    const head = Math.ceil(maxLines / 3);
    return [...block.slice(0, head), cut, ...block.slice(block.length - (maxLines - head))].join("\n");
  });
  return { excerpts, omitted: Math.max(0, distinct.length - maxFailures) };
}

/** Runner count lines and named nonfailing leak statuses, whitespace-trimmed. */
export function summaryLines(tier, text) {
  const found = [];
  for (const line of stripAnsi(text).split(/\r?\n/)) {
    if (TIERS[tier].summary.some((pattern) => pattern.test(line))) found.push(line.trim());
  }
  return found;
}

function tail(text, count) {
  const lines = stripAnsi(text).trimEnd().split(/\r?\n/);
  return lines.slice(-count).map(clip).join("\n");
}

function runStep(step, log, env) {
  return new Promise((resolve) => {
    fs.appendFileSync(log, `$ (cd ${step.cwd} && ${step.command} ${step.args.join(" ")})\n`);
    const child = spawn(step.command, step.args, { cwd: step.cwd, env, stdio: ["ignore", "pipe", "pipe"] });
    let output = "";
    const record = (chunk) => {
      output += chunk;
      fs.appendFileSync(log, chunk);
    };
    child.stdout.on("data", record);
    child.stderr.on("data", record);
    child.on("error", (error) => {
      record(`\n${step.command}: ${error.message}\n`);
      resolve({ code: 127, output });
    });
    child.on("close", (code, signal) => resolve({ code: code ?? (signal ? 128 : 1), output }));
  });
}

/**
 * Run one tier: every step's output goes to `log`; the returned report is the compact text
 * printed to the terminal. Steps stop at the first failing one (a failed build skips its tests).
 */
export async function runTier(tier, { args = [], logDir, maxFailures, maxLines, steps, now = Date.now } = {}) {
  const spec = TIERS[tier];
  if (!spec) throw new Error(`unknown tier: ${tier}`);
  fs.mkdirSync(logDir, { recursive: true });
  const stamp = new Date(now()).toISOString().replace(/[:.]/g, "-");
  const log = path.join(logDir, `${tier}-${stamp}.log`);
  fs.writeFileSync(log, "");
  const env = { ...process.env, NO_COLOR: "1", CARGO_TERM_COLOR: "never" };
  delete env.FORCE_COLOR;
  const started = Date.now();
  let code = 0;
  let output = "";
  for (const step of steps ?? spec.steps(args)) {
    const result = await runStep(step, log, env);
    output += result.output;
    code = result.code;
    if (code !== 0) break;
  }
  const seconds = ((Date.now() - started) / 1000).toFixed(1);
  const counts = summaryLines(tier, output);
  const status = code === 0 ? "PASS" : "FAIL";
  const lines = [`${tier}: ${status} in ${seconds}s${counts.length ? ` | ${counts.join(" | ")}` : ""} | log: ${log}`];
  if (code !== 0) {
    const { excerpts, omitted } = failureExcerpts(tier, output, { maxFailures, maxLines });
    if (excerpts.length === 0) {
      lines.push(`  (no test failure found; exit ${code}; last ${FALLBACK_TAIL_LINES} lines)`, tail(output, FALLBACK_TAIL_LINES));
    } else {
      lines.push(...excerpts);
      if (omitted > 0) lines.push(`  … ${omitted} more failures in the log`);
    }
  }
  return { code, log, report: lines.join("\n") };
}

export function parseArgs(argv) {
  const split = argv.indexOf("--");
  const own = split === -1 ? argv : argv.slice(0, split);
  const runnerArgs = split === -1 ? [] : argv.slice(split + 1);
  const options = { tiers: [], maxFailures: DEFAULT_MAX_FAILURES, maxLines: DEFAULT_MAX_LINES, args: runnerArgs };
  for (let i = 0; i < own.length; i += 1) {
    const arg = own[i];
    if (arg === "--max-failures" || arg === "--max-lines") {
      const value = Number.parseInt(own[i + 1] ?? "", 10);
      if (!Number.isInteger(value) || value < 1) throw new Error(`${arg} needs a positive integer`);
      options[arg === "--max-failures" ? "maxFailures" : "maxLines"] = value;
      i += 1;
    } else if (arg.startsWith("-")) {
      throw new Error(`unknown option: ${arg}`);
    } else {
      options.tiers.push(...arg.split(",").filter(Boolean));
    }
  }
  if (options.tiers.length === 0) throw new Error(`name a tier: ${Object.keys(TIERS).join(", ")}`);
  for (const tier of options.tiers) if (!TIERS[tier]) throw new Error(`unknown tier: ${tier}`);
  if (runnerArgs.length > 0 && options.tiers.length > 1) {
    throw new Error("runner args after -- apply to exactly one tier");
  }
  return options;
}

async function main() {
  let options;
  try {
    options = parseArgs(process.argv.slice(2));
  } catch (error) {
    console.error(`test-quiet: ${error.message}`);
    console.error("usage: node scripts/test-quiet.mjs <tier>[,<tier>...] [--max-failures N] [--max-lines N] [-- runner args]");
    process.exit(2);
  }
  const logDir = process.env.HOTSHEET_TEST_QUIET_LOG_DIR ?? path.join(REPO, "target/test-quiet");
  let exit = 0;
  for (const tier of options.tiers) {
    const { code, report } = await runTier(tier, { ...options, logDir });
    console.log(report);
    if (code !== 0) exit = code;
  }
  process.exit(exit);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  await main();
}
