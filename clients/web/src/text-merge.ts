/**
 * Three-way merges for ticket fields edited concurrently (HS2-A4XCXE).
 *
 * `mergeText` is a diff3: each side's changes against the common base become hunks, and hunks from
 * different sides merge cleanly unless they overlap (or both insert at the same point) with different
 * results. It merges by line first; a line region both sides changed is retried word by word, so
 * edits to different words of the same line (a title, say) still combine (HS2-R8TYCG). Only an
 * overlap that survives the word-level pass is a real conflict worth asking the user about.
 */

export type TextMerge = { clean: true; merged: string } | { clean: false };

interface Hunk {
  /** Base token range `[start, end)` this hunk replaces. */
  start: number;
  end: number;
  lines: string[];
}

/** Token-by-token LCS guard: larger inputs fall back to "changed as a whole" hunks. */
const MAX_LCS_CELLS = 4_000_000;

function lines(text: string): string[] {
  return text.split('\n');
}

/** Words, whitespace runs, and single punctuation marks: every character belongs to exactly one token. */
function words(text: string): string[] {
  return text.match(/\s+|[\p{L}\p{N}_]+|[^\s\p{L}\p{N}_]/gu) ?? [];
}

/** Hunks turning `base` into `side`, from a longest-common-subsequence token match. */
function diffHunks(base: readonly string[], side: readonly string[]): Hunk[] {
  let prefix = 0;
  while (prefix < base.length && prefix < side.length && base[prefix] === side[prefix]) prefix += 1;
  let suffix = 0;
  while (
    suffix < base.length - prefix &&
    suffix < side.length - prefix &&
    base[base.length - 1 - suffix] === side[side.length - 1 - suffix]
  )
    suffix += 1;
  const a = base.slice(prefix, base.length - suffix),
    b = side.slice(prefix, side.length - suffix);
  if (a.length === 0 && b.length === 0) return [];
  if (a.length === 0 || b.length === 0 || a.length * b.length > MAX_LCS_CELLS)
    return [{ start: prefix, end: prefix + a.length, lines: [...b] }];
  const width = b.length + 1,
    table = new Uint32Array((a.length + 1) * width);
  for (let i = a.length - 1; i >= 0; i -= 1)
    for (let j = b.length - 1; j >= 0; j -= 1)
      table[i * width + j] =
        a[i] === b[j]
          ? table[(i + 1) * width + j + 1] + 1
          : Math.max(table[(i + 1) * width + j], table[i * width + j + 1]);
  const hunks: Hunk[] = [];
  let i = 0,
    j = 0,
    open: Hunk | undefined;
  const close = () => {
    if (open) hunks.push(open);
    open = undefined;
  };
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) {
      close();
      i += 1;
      j += 1;
    } else if (j < b.length && (i === a.length || table[i * width + j + 1] >= table[(i + 1) * width + j])) {
      open ??= { start: prefix + i, end: prefix + i, lines: [] };
      open.lines.push(b[j]);
      j += 1;
    } else {
      open ??= { start: prefix + i, end: prefix + i, lines: [] };
      open.end = prefix + i + 1;
      i += 1;
    }
  }
  close();
  return hunks;
}

/** The text of `base[start, end)` after applying one side's hunks that fall inside that range. */
function applyWithin(base: readonly string[], start: number, end: number, hunks: readonly Hunk[]): string[] {
  const result: string[] = [];
  let position = start;
  for (const hunk of hunks) {
    result.push(...base.slice(position, hunk.start), ...hunk.lines);
    position = hunk.end;
  }
  result.push(...base.slice(position, end));
  return result;
}

function sameLines(left: readonly string[], right: readonly string[]) {
  return left.length === right.length && left.every((line, index) => line === right[index]);
}

/**
 * diff3 over token sequences. A region both sides changed differently goes to `resolve`, which may
 * merge it at a finer grain; `undefined` means a real conflict. With `strictAdjacency`, an insertion
 * touching the other side's change at either boundary also counts as overlapping.
 */
function mergeTokens(
  base: readonly string[],
  mine: readonly string[],
  theirs: readonly string[],
  {
    resolve,
    strictAdjacency = false,
  }: {
    resolve?: (base: string[], mine: string[], theirs: string[]) => string[] | undefined;
    strictAdjacency?: boolean;
  } = {},
): string[] | undefined {
  const tagged = [
    ...diffHunks(base, mine).map((hunk) => ({ hunk, side: 0 })),
    ...diffHunks(base, theirs).map((hunk) => ({ hunk, side: 1 })),
  ].sort((left, right) => left.hunk.start - right.hunk.start || left.hunk.end - right.hunk.end);
  const output: string[] = [];
  let position = 0,
    index = 0;
  while (index < tagged.length) {
    const cluster = [tagged[index]];
    let start = tagged[index].hunk.start,
      end = tagged[index].hunk.end;
    index += 1;
    // A hunk joins the cluster when it overlaps it, or when both are insertions at the same point.
    // Lines keep "edit this line, append after it" apart; words are tighter, so with
    // `strictAdjacency` an insertion touching the other change at either boundary joins too:
    // "insert a word here" next to "replace that word" has no unambiguous reading, so it asks.
    const joins = (hunk: Hunk) =>
      hunk.start < end ||
      (hunk.start === end && hunk.start === hunk.end && (start === end || strictAdjacency)) ||
      (strictAdjacency && hunk.start === end && start === end);
    while (index < tagged.length && joins(tagged[index].hunk)) {
      end = Math.max(end, tagged[index].hunk.end);
      start = Math.min(start, tagged[index].hunk.start);
      cluster.push(tagged[index]);
      index += 1;
    }
    output.push(...base.slice(position, start));
    const sides = [0, 1].map((side) => cluster.filter((item) => item.side === side).map((item) => item.hunk));
    if (sides[0].length === 0 || sides[1].length === 0) {
      output.push(...applyWithin(base, start, end, sides[0].length ? sides[0] : sides[1]));
    } else {
      const ours = applyWithin(base, start, end, sides[0]),
        theirsTokens = applyWithin(base, start, end, sides[1]);
      if (sameLines(ours, theirsTokens)) output.push(...ours);
      else {
        const resolved = resolve?.(base.slice(start, end), ours, theirsTokens);
        if (!resolved) return undefined;
        output.push(...resolved);
      }
    }
    position = end;
  }
  output.push(...base.slice(position));
  return output;
}

/** Retry a line region both sides changed word by word; `undefined` when the words themselves overlap. */
function mergeLineRegion(base: string[], mine: string[], theirs: string[]): string[] | undefined {
  const merged = mergeTokens(words(base.join('\n')), words(mine.join('\n')), words(theirs.join('\n')), {
    strictAdjacency: true,
  });
  return merged?.join('').split('\n');
}

/** Merge two concurrent edits of `base`. Identical or one-sided edits always merge. */
export function mergeText(base: string, mine: string, theirs: string): TextMerge {
  if (mine === theirs) return { clean: true, merged: mine };
  if (mine === base) return { clean: true, merged: theirs };
  if (theirs === base) return { clean: true, merged: mine };
  const merged = mergeTokens(lines(base), lines(mine), lines(theirs), { resolve: mergeLineRegion });
  return merged ? { clean: true, merged: merged.join('\n') } : { clean: false };
}

/**
 * Merge concurrent edits of a set-like list (tags): start from `theirs`, then apply what `mine`
 * added and removed relative to `base`. Never conflicts.
 */
export function mergeSet(base: readonly string[], mine: readonly string[], theirs: readonly string[]): string[] {
  const baseSet = new Set(base),
    mineSet = new Set(mine),
    removed = new Set(base.filter((item) => !mineSet.has(item))),
    result = theirs.filter((item) => !removed.has(item));
  for (const item of mine) if (!baseSet.has(item) && !result.includes(item)) result.push(item);
  return result;
}
