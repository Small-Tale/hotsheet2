import { describe, expect, it } from 'vitest';

import { mergeSet, mergeText } from './text-merge';

const text = (...rows: string[]) => rows.join('\n');

describe('mergeText (HS2-A4XCXE)', () => {
  it('takes the only side that changed, or either side when both agree', () => {
    expect(mergeText('a', 'a', 'b')).toEqual({ clean: true, merged: 'b' });
    expect(mergeText('a', 'b', 'a')).toEqual({ clean: true, merged: 'b' });
    expect(mergeText('a', 'c', 'c')).toEqual({ clean: true, merged: 'c' });
  });

  it('merges edits to different lines from both sides', () => {
    const base = text('one', 'two', 'three', 'four', 'five'),
      mine = text('one', 'TWO', 'three', 'four', 'five'),
      theirs = text('one', 'two', 'three', 'four', 'FIVE', 'six');
    expect(mergeText(base, mine, theirs)).toEqual({
      clean: true,
      merged: text('one', 'TWO', 'three', 'four', 'FIVE', 'six'),
    });
  });

  it('merges an insertion on one side with a deletion elsewhere on the other', () => {
    const base = text('a', 'b', 'c', 'd'),
      mine = text('a', 'new', 'b', 'c', 'd'),
      theirs = text('a', 'b', 'c');
    expect(mergeText(base, mine, theirs)).toEqual({ clean: true, merged: text('a', 'new', 'b', 'c') });
  });

  it('keeps identical overlapping edits once', () => {
    const base = text('a', 'b', 'c'),
      mine = text('a', 'B', 'c', 'mine'),
      theirs = text('a', 'B', 'c');
    expect(mergeText(base, mine, theirs)).toEqual({ clean: true, merged: text('a', 'B', 'c', 'mine') });
  });

  it('reports a conflict only when both sides change the same lines differently', () => {
    expect(mergeText(text('a', 'b', 'c'), text('a', 'mine', 'c'), text('a', 'theirs', 'c'))).toEqual({
      clean: false,
    });
    expect(mergeText('title', 'my title', 'their title')).toEqual({ clean: false });
  });

  it('conflicts when both sides insert different lines at the same point', () => {
    expect(mergeText(text('a', 'b'), text('a', 'x', 'b'), text('a', 'y', 'b'))).toEqual({ clean: false });
  });

  it('merges appends from one side with edits from the other, including an empty base', () => {
    expect(mergeText('', 'mine', 'theirs')).toEqual({ clean: false });
    const base = text('# Notes', 'Draft'),
      mine = text('# Notes', 'Draft, expanded by me'),
      theirs = text('# Notes', 'Draft', '', 'Appended by the AI');
    expect(mergeText(base, mine, theirs)).toEqual({
      clean: true,
      merged: text('# Notes', 'Draft, expanded by me', '', 'Appended by the AI'),
    });
  });

  it('is symmetric for clean merges', () => {
    const base = text('1', '2', '3', '4', '5', '6'),
      left = text('1', 'two', '3', '4', '5', '6'),
      right = text('1', '2', '3', '4', 'five', '6', '7');
    expect(mergeText(base, left, right)).toEqual(mergeText(base, right, left));
  });
});

describe('mergeText adversarial sequences', () => {
  // Deterministic pseudo-random walk over line edits (seeded LCG), so failures reproduce.
  function random(seed: number) {
    let state = seed;
    return (limit: number) => {
      state = (state * 1_103_515_245 + 12_345) % 2_147_483_648;
      return state % limit;
    };
  }
  function edit(lines: string[], index: number, kind: number, tag: string) {
    const next = [...lines];
    if (kind === 0) next[index] = `${next[index]} ${tag}`;
    else if (kind === 1) next.splice(index, 0, `inserted ${tag}`);
    else next.splice(index, 1);
    return next;
  }

  it('merges any two edits separated by an unchanged line, symmetrically and with both changes', () => {
    for (let seed = 1; seed <= 300; seed += 1) {
      const next = random(seed),
        size = 4 + next(8),
        base = Array.from({ length: size }, (_, index) => `line ${index}`),
        first = next(size - 2),
        second = first + 2 + next(size - first - 2),
        mine = edit(base, first, next(3), 'mine'),
        theirs = edit(base, second, next(3), 'theirs'),
        merged = mergeText(base.join('\n'), mine.join('\n'), theirs.join('\n'));
      expect(merged.clean, `seed ${seed}`).toBe(true);
      if (!merged.clean) continue;
      expect(mergeText(base.join('\n'), theirs.join('\n'), mine.join('\n'))).toEqual(merged);
      // Re-merging the result against either side is idempotent: it already contains that side's change.
      expect(mergeText(mine.join('\n'), merged.merged, merged.merged)).toEqual(merged);
      for (const side of [mine, theirs])
        for (const line of side.filter((value) => !base.includes(value)))
          expect(merged.merged.split('\n'), `seed ${seed}`).toContain(line);
    }
  });

  it('never reports a conflict for an identical overlapping edit, and always does for a divergent one', () => {
    for (let seed = 1; seed <= 100; seed += 1) {
      const next = random(seed),
        base = Array.from({ length: 3 + next(6) }, (_, index) => `line ${index}`),
        index = next(base.length),
        same = edit(base, index, 0, 'both');
      expect(mergeText(base.join('\n'), same.join('\n'), same.join('\n'))).toEqual({
        clean: true,
        merged: same.join('\n'),
      });
      expect(
        mergeText(base.join('\n'), edit(base, index, 0, 'mine').join('\n'), edit(base, index, 0, 'theirs').join('\n')),
      ).toEqual({ clean: false });
    }
  });
});

describe('mergeSet (HS2-A4XCXE)', () => {
  it('applies my additions and removals on top of theirs', () => {
    expect(mergeSet(['a', 'b'], ['a', 'c'], ['a', 'b', 'd'])).toEqual(['a', 'd', 'c']);
    expect(mergeSet(['a'], ['a', 'b'], [])).toEqual(['b']);
    expect(mergeSet([], ['x'], ['x'])).toEqual(['x']);
  });
});
