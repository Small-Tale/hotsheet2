// HS2-A7X8F5: name the Rust changes the local gate cannot type-check on this host.
//
// The macOS gate compiles only the macOS `cfg` branches. A change inside, or to, a platform `cfg`
// gate can leave another platform's build broken (for example code only a macOS branch used
// becomes `dead_code` on Linux, CI run 42115994261). A local Linux `cargo check` cannot run on
// macOS: the `ring`, `aws-lc-sys` and `libwebp-sys2` build scripts need a Linux C sysroot. So the
// gate reports these changes as deferred to CI instead of implying they were verified. This is a
// text scan of the diff, never a build.
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';

/** A `cfg` predicate that depends on the target platform. */
const PLATFORM_PREDICATE = /\b(?:target_os|target_family|target_env|target_vendor)\s*=|\b(?:windows|unix)\b/;
/** `#[cfg(...)]`, `#![cfg(...)]`, `#[cfg_attr(...)]`, and `cfg!(...)`, up to the first `(`. */
const CFG_START = /(#!?\[\s*cfg(?:_attr)?\s*\(|\bcfg!\s*\()/g;

/** The balanced argument text of a `cfg` whose `(` is at `open`. */
function argumentText(text, open) {
  let depth = 0;
  for (let index = open; index < text.length; index += 1) {
    if (text[index] === '(') depth += 1;
    else if (text[index] === ')') {
      depth -= 1;
      if (depth === 0) return text.slice(open + 1, index);
    }
  }
  return text.slice(open + 1);
}

function lineStarts(text) {
  const starts = [0];
  for (let index = 0; index < text.length; index += 1) if (text[index] === '\n') starts.push(index + 1);
  return starts;
}

function lineAt(starts, index) {
  let low = 0;
  let high = starts.length - 1;
  while (low < high) {
    const middle = Math.ceil((low + high) / 2);
    if (starts[middle] <= index) low = middle;
    else high = middle - 1;
  }
  return low + 1;
}

/**
 * The 1-based inclusive line ranges of `text` that a platform `cfg` gates, each with its
 * predicate. An attribute gates the item after it: through the matching `}` of its first block,
 * or its first `;` when that comes before any `{`. An inner `#![cfg]` gates the whole file, and a
 * `cfg!` gates its own line. Comments and strings are not parsed, so the scan errs toward
 * reporting a change.
 */
export function platformCfgRanges(text) {
  const starts = lineStarts(text);
  const lastLine = starts.length;
  const ranges = [];
  for (const match of text.matchAll(CFG_START)) {
    const open = match.index + match[0].length - 1;
    const raw = argumentText(text, open);
    const predicate = raw.replace(/\s+/g, ' ').trim();
    if (!PLATFORM_PREDICATE.test(predicate)) continue;
    const start = lineAt(starts, match.index);
    if (match[0].startsWith('#![')) {
      ranges.push({ start: 1, end: lastLine, predicate });
      continue;
    }
    if (match[0].startsWith('cfg!')) {
      ranges.push({ start, end: start, predicate });
      continue;
    }
    let depth = 0;
    let end = lastLine;
    for (let index = open + raw.length + 2; index < text.length; index += 1) {
      const char = text[index];
      if (char === '{') depth += 1;
      else if (char === '}') {
        depth -= 1;
        if (depth <= 0) {
          end = lineAt(starts, index);
          break;
        }
      } else if (char === ';' && depth === 0) {
        end = lineAt(starts, index);
        break;
      }
    }
    ranges.push({ start, end, predicate });
  }
  return ranges;
}

/** The changed old-side and new-side line numbers of a unified `-U0` diff. */
export function changedLines(diff) {
  const removed = [];
  const added = [];
  for (const match of diff.matchAll(/^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/gm)) {
    const [oldStart, oldCount, newStart, newCount] = [match[1], match[2] ?? '1', match[3], match[4] ?? '1'].map(Number);
    for (let line = 0; line < oldCount; line += 1) removed.push(oldStart + line);
    for (let line = 0; line < newCount; line += 1) added.push(newStart + line);
  }
  return { removed, added };
}

/** The files that may declare module `path` (`a/b.rs` or `a/b/mod.rs` → `a/mod.rs`, `a/lib.rs`, …). */
export function parentModuleFiles(path) {
  const name = basename(path) === 'mod.rs' ? basename(dirname(path)) : basename(path, '.rs');
  const directory = basename(path) === 'mod.rs' ? dirname(dirname(path)) : dirname(path);
  return {
    name,
    parents: ['mod.rs', 'lib.rs', 'main.rs'].map((file) => join(directory, file)).concat(`${directory}.rs`),
  };
}

/** The platform predicate under which a parent declares `path`'s module, if any. */
function moduleGate(read, side, path) {
  const { name, parents } = parentModuleFiles(path);
  const declaration = new RegExp(`^\\s*(?:pub(?:\\([^)]*\\))?\\s+)?mod\\s+${name}\\s*;`);
  for (const parent of parents) {
    const text = read(side, parent);
    if (text === null) continue;
    const lines = text.split('\n');
    const ranges = platformCfgRanges(text);
    for (const [index, line] of lines.entries()) {
      if (!declaration.test(line)) continue;
      const hit = ranges.find((range) => range.start <= index + 1 && index + 1 <= range.end);
      if (hit) return hit.predicate;
    }
  }
  return null;
}

/**
 * The changed Rust files whose changed lines touch platform-gated code. `read(side, path)`
 * returns the file text on the `old` (base) or `new` (working tree) side, or `null` when the
 * file does not exist there; `diff(path)` returns its `-U0` diff against the base.
 */
export function platformCfgChanges({ changes, read: uncached, diff }) {
  // Sibling modules share parents; read each side of each file once.
  const cache = new Map();
  const read = (side, path) => {
    const key = `${side}\0${path}`;
    if (!cache.has(key)) cache.set(key, uncached(side, path));
    return cache.get(key);
  };
  const files = [];
  for (const path of changes.filter((path) => path.endsWith('.rs'))) {
    const { removed, added } = changedLines(diff(path));
    const predicates = new Set();
    const lines = new Set();
    for (const [side, numbers] of [
      ['old', removed],
      ['new', added],
    ]) {
      const text = read(side, path);
      if (text === null || numbers.length === 0) continue;
      const whole = moduleGate(read, side, path);
      const ranges = platformCfgRanges(text);
      for (const line of numbers) {
        const hits = ranges.filter((range) => range.start <= line && line <= range.end);
        if (whole) hits.push({ predicate: whole });
        if (!hits.length) continue;
        if (side === 'new') lines.add(line);
        for (const hit of hits) predicates.add(hit.predicate);
      }
    }
    if (predicates.size)
      files.push({ path, lines: [...lines].sort((a, b) => a - b), predicates: [...predicates].sort() });
  }
  return files;
}

const HOST_NAMES = { darwin: 'macOS', linux: 'Linux', win32: 'Windows' };

/** The gate's platform evidence: what this host compiled and what CI must still type-check. */
export function platformCfgEvidence(files, host = process.platform) {
  const hostName = HOST_NAMES[host] ?? host;
  const where = files.map((file) => (file.lines.length ? `${file.path}:${file.lines[0]}` : file.path)).join(', ');
  const summary = files.length
    ? `${files.length} Rust file${files.length === 1 ? '' : 's'} change platform cfg code (${where}); only the ` +
      `${hostName} branches were compiled locally. CI clippy type-checks Linux; the Platforms workflow covers ` +
      'Windows and macOS.'
    : 'none';
  return { host: hostName, files, summary };
}

function git(repo, args, input) {
  const result = spawnSync('git', args, {
    cwd: repo,
    env: withoutGitRepositoryEnv(process.env),
    encoding: 'utf8',
    input,
    maxBuffer: 256 * 1024 * 1024,
  });
  return result.status === 0 ? result.stdout : null;
}

/** Read `base:path` for every path in one `git cat-file --batch`; `null` marks a missing path. */
export function readBaseFiles(repo, base, paths) {
  const texts = new Map(paths.map((path) => [path, null]));
  if (!paths.length) return texts;
  const out = spawnSync('git', ['cat-file', '--batch'], {
    cwd: repo,
    env: withoutGitRepositoryEnv(process.env),
    input: paths.map((path) => `${base}:${path}`).join('\n') + '\n',
    maxBuffer: 256 * 1024 * 1024,
  });
  if (out.status !== 0) return texts;
  const bytes = out.stdout;
  let offset = 0;
  for (const path of paths) {
    const newline = bytes.indexOf(10, offset);
    if (newline < 0) break;
    const header = bytes.subarray(offset, newline).toString('utf8');
    offset = newline + 1;
    const match = /^[0-9a-f]+ (\w+) (\d+)$/.exec(header);
    if (!match) continue; // `<spec> missing`
    const size = Number(match[2]);
    if (match[1] === 'blob') texts.set(path, bytes.subarray(offset, offset + size).toString('utf8'));
    offset += size + 1;
  }
  return texts;
}

/** Split one multi-file `git diff` into each file's text, keyed by its new-side path. */
export function splitDiff(diff) {
  const files = new Map();
  for (const part of diff.split(/^(?=diff --git )/m)) {
    const match = /^diff --git a\/(.+?) b\/(.+)$/m.exec(part);
    if (match) files.set(match[2], part);
  }
  return files;
}

/** [`platformCfgChanges`] for the working tree of `repo` against the commit `base`. */
export function platformCfgForRepo(repo, base, changes, host = process.platform) {
  const rust = changes.filter((path) => path.endsWith('.rs'));
  if (!base || !rust.length) return platformCfgEvidence([], host);
  const wanted = [...new Set(rust.flatMap((path) => [path, ...parentModuleFiles(path).parents]))];
  const old = readBaseFiles(repo, base, wanted);
  const diffs = splitDiff(
    git(repo, ['diff', '-U0', '--no-color', '--no-ext-diff', '--no-renames', base, '--', ...rust]) ?? '',
  );
  const files = platformCfgChanges({
    changes: rust,
    read: (side, path) => {
      if (side === 'old') return old.has(path) ? old.get(path) : readBaseFiles(repo, base, [path]).get(path);
      const file = join(repo, path);
      return existsSync(file) ? readFileSync(file, 'utf8') : null;
    },
    diff: (path) => {
      if (diffs.has(path)) return diffs.get(path);
      const file = join(repo, path);
      // An untracked new file has no diff against the base: every line of it is new.
      if (old.get(path) !== null || !existsSync(file)) return '';
      const text = readFileSync(file, 'utf8');
      const count = text.split('\n').length - (text.endsWith('\n') ? 1 : 0);
      return count ? `@@ -0,0 +1,${count} @@` : '';
    },
  });
  return platformCfgEvidence(files, host);
}
