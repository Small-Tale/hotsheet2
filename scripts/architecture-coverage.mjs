#!/usr/bin/env node
// HS2-H0T69J: mechanical checks for the deep architecture baseline in
// docs/architecture. It lists every source module or directory above the
// depth-standard threshold per area, reports which ones the area coverage
// files do not name, and verifies every `path:line` citation in the
// architecture Markdown against the working tree (which is the audited SHA on
// the review branch).
//
//   node scripts/architecture-coverage.mjs inventory [--area <key>]
//   node scripts/architecture-coverage.mjs check [--area <key>] [--citations-only]
//
// `check` exits non-zero when a required path is unmapped or a citation does
// not resolve. Pure helpers are exported for scripts/architecture-coverage.test.mjs.

import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const ARCH = join(ROOT, 'docs', 'architecture');

/** Convert a platform path to the repository's slash spelling. */
export function slash(path) {
  return path.split(sep).join('/');
}

/** True when the repository-relative path is a non-test source file. */
export function isSource(path, config) {
  if (!config.source_extensions.some((ext) => path.endsWith(ext))) return false;
  const probe = `/${path}`;
  return !config.test_patterns.some((pattern) => probe.includes(pattern));
}

/** First area whose root prefix matches; areas are ordered most specific first. */
export function areaFor(path, config) {
  return config.areas.find((area) => area.roots.some((root) => path === root || path.startsWith(root))) ?? null;
}

/**
 * Given `{path: lines}` for non-test source files, return the required entries:
 * every file at or above the threshold, every directory whose non-test source
 * total reaches it, and every direct entry of a `list_every_entry` directory.
 */
export function requiredEntries(files, area, threshold) {
  const required = new Map();
  const dirTotals = new Map();
  for (const [path, lines] of Object.entries(files)) {
    if (lines >= threshold) required.set(path, { kind: 'file', lines });
    const root = area.roots.find((r) => path.startsWith(r));
    const parts = path.split('/');
    for (let i = parts.length - 1; i > 0; i -= 1) {
      const dir = parts.slice(0, i).join('/');
      if (!root || !dir.startsWith(root) || dir === root) break;
      dirTotals.set(dir, (dirTotals.get(dir) ?? 0) + lines);
    }
    for (const listed of area.list_every_entry ?? []) {
      if (!path.startsWith(`${listed}/`)) continue;
      const entry = `${listed}/${path.slice(listed.length + 1).split('/')[0]}`;
      if (!required.has(entry)) required.set(entry, { kind: 'listed', lines: 0 });
    }
  }
  for (const [dir, lines] of dirTotals) {
    if (lines >= threshold) required.set(dir, { kind: 'dir', lines });
  }
  return required;
}

/** True when coverage text names the path in backticks, optionally with a `:line` suffix. */
export function isMapped(path, coverageText) {
  const bare = path.replace(/\.(rs|ts|tsx|mjs|js|swift)$/, '');
  const escaped = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const pattern = new RegExp('`(' + escaped(path) + '|' + escaped(bare) + ')(/)?(:\\d+(-\\d+)?)?`');
  return pattern.test(coverageText);
}

const CITATION = /`((?:[\w@.-]+\/)*[\w@.-]+\.[A-Za-z]+):(\d+)(?:-(\d+))?`/g;

/** Extract `path:line` or `path:start-end` citations from Markdown or DSL text. */
export function citations(text) {
  const found = [];
  for (const match of text.matchAll(CITATION)) {
    found.push({ path: match[1], start: Number(match[2]), end: Number(match[3] ?? match[2]) });
  }
  return found;
}

/** Return a reason string when a citation does not resolve, else null. */
export function citationProblem(citation, lineCount) {
  if (lineCount === null) return 'file not found';
  if (citation.start < 1 || citation.end < citation.start) return 'invalid range';
  if (citation.end > lineCount) return `line ${citation.end} beyond end (${lineCount} lines)`;
  return null;
}

function walk(dir, out) {
  for (const name of readdirSync(dir)) {
    if (name === 'node_modules' || name === 'target' || name.startsWith('.')) continue;
    const full = join(dir, name);
    const st = statSync(full);
    if (st.isDirectory()) walk(full, out);
    else out.push(slash(relative(ROOT, full)));
  }
}

function countLines(path) {
  const text = readFileSync(join(ROOT, path), 'utf8');
  return text.length === 0 ? 0 : text.split('\n').length - (text.endsWith('\n') ? 1 : 0);
}

function loadConfig() {
  return JSON.parse(readFileSync(join(ARCH, 'areas.json'), 'utf8'));
}

function areaFiles(config) {
  const byArea = new Map(config.areas.map((a) => [a.key, {}]));
  const seenRoots = new Set();
  for (const area of config.areas) {
    for (const root of area.roots) {
      const base = root.replace(/\/[^/]*$/, '');
      const dir = existsSync(join(ROOT, root)) && statSync(join(ROOT, root)).isDirectory() ? root : base;
      if (seenRoots.has(dir)) continue;
      seenRoots.add(dir);
      const files = [];
      walk(join(ROOT, dir), files);
      for (const file of files) {
        if (!isSource(file, config)) continue;
        const owner = areaFor(file, config);
        if (owner) byArea.get(owner.key)[file] = countLines(file);
      }
    }
  }
  return byArea;
}

function coverageText(area) {
  return area.coverage
    .map((file) => join(ARCH, file))
    .filter((file) => existsSync(file))
    .map((file) => readFileSync(file, 'utf8'))
    .join('\n');
}

function markdownFiles() {
  const out = [];
  walk(ARCH, out);
  return out.filter((f) => f.endsWith('.md') || f.endsWith('.dsl'));
}

function main(argv) {
  const [command = 'check', ...rest] = argv;
  const areaFlag = rest.indexOf('--area');
  const only = areaFlag >= 0 ? rest[areaFlag + 1] : null;
  const config = loadConfig();
  const byArea = areaFiles(config);
  let failures = 0;

  if (!rest.includes('--citations-only')) {
    for (const area of config.areas) {
      if (only && area.key !== only) continue;
      const required = requiredEntries(byArea.get(area.key), area, config.threshold_lines);
      const text = coverageText(area);
      const rows = [...required].sort(([a], [b]) => a.localeCompare(b));
      const unmapped = rows.filter(([path]) => !isMapped(path, text));
      if (command === 'inventory') {
        console.log(`## ${area.key} (${area.ticket}): ${rows.length} required entries`);
        for (const [path, info] of rows)
          console.log(
            `${isMapped(path, text) ? 'mapped  ' : 'UNMAPPED'} ${info.kind.padEnd(6)} ${String(info.lines).padStart(6)}  ${path}`,
          );
      } else {
        console.log(`${area.key}: ${rows.length - unmapped.length}/${rows.length} required entries mapped`);
        for (const [path] of unmapped) console.log(`  unmapped: ${path}`);
        failures += unmapped.length;
      }
    }
  }

  if (command === 'check') {
    let total = 0;
    for (const doc of markdownFiles()) {
      const text = readFileSync(join(ROOT, doc), 'utf8');
      for (const citation of citations(text)) {
        total += 1;
        const exists = existsSync(join(ROOT, citation.path)) && statSync(join(ROOT, citation.path)).isFile();
        const problem = citationProblem(citation, exists ? countLines(citation.path) : null);
        if (problem) {
          failures += 1;
          console.log(`  bad citation ${doc}: ${citation.path}:${citation.start} (${problem})`);
        }
      }
    }
    console.log(`citations: ${total} checked`);
  }
  if (failures > 0) {
    console.log(`${failures} problem(s)`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main(process.argv.slice(2));
