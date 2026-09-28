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

describe('mergeText within a line (HS2-R8TYCG)', () => {
  it('merges edits to different words of one line, such as a title', () => {
    expect(mergeText('Fix the parser bug', 'Fix the parser crash', 'Fix the lexer bug')).toEqual({
      clean: true,
      merged: 'Fix the lexer crash',
    });
    expect(mergeText('Ship it', 'Ship it now', 'Please ship it')).toEqual({
      clean: true,
      merged: 'Please ship it now',
    });
  });

  it('merges word edits inside a multi-line region both sides touched', () => {
    const base = text('Intro', 'alpha beta gamma', 'delta epsilon', 'End'),
      mine = text('Intro', 'alpha BETA gamma', 'delta epsilon', 'End'),
      theirs = text('Intro', 'alpha beta gamma', 'delta EPSILON!', 'End');
    // Line-level, both sides changed adjacent lines of one region; word-level they are disjoint.
    expect(mergeText(base, mine, theirs)).toEqual({
      clean: true,
      merged: text('Intro', 'alpha BETA gamma', 'delta EPSILON!', 'End'),
    });
  });

  it('keeps punctuation and whitespace exact and still conflicts on the same word', () => {
    expect(mergeText('a, b; c.', 'a, B; c.', 'a, b; c!')).toEqual({ clean: true, merged: 'a, B; c!' });
    expect(mergeText('Fix the parser bug', 'Fix the lexer bug', 'Fix the tokenizer bug')).toEqual({
      clean: false,
    });
    // Both sides inserting different words at the same point is ambiguous.
    expect(mergeText('one two', 'one new two', 'one other two')).toEqual({ clean: false });
    // A word inserted right next to a word the other side replaced has no unambiguous reading.
    expect(mergeText('My local wording', 'My revised local wording', 'Their newer wording')).toEqual({
      clean: false,
    });
    expect(mergeText('Fix the bug', 'Fix the bug quickly', 'Fix the defect')).toEqual({ clean: false });
    // Lines stay looser: editing a line while the other side appends lines after it merges.
    expect(mergeText('# Notes\nDraft', '# Notes\nDraft, expanded', '# Notes\nDraft\n\nAppended')).toEqual({
      clean: true,
      merged: '# Notes\nDraft, expanded\n\nAppended',
    });
  });

  it('merges any two edits to different words of a line, symmetrically (seeded)', () => {
    for (let seed = 1; seed <= 200; seed += 1) {
      let state = seed;
      const next = (limit: number) => {
          state = (state * 1_103_515_245 + 12_345) % 2_147_483_648;
          return state % limit;
        },
        size = 3 + next(8),
        base = Array.from({ length: size }, (_, index) => `w${index}`),
        first = next(size - 1),
        second = first + 1 + next(size - first - 1),
        mine = base.map((word, index) => (index === first ? `${word}-mine` : word)),
        theirs = base.map((word, index) => (index === second ? `${word}-theirs` : word)),
        merged = mergeText(base.join(' '), mine.join(' '), theirs.join(' '));
      const expected = base.map((word, index) =>
        index === first ? `${word}-mine` : index === second ? `${word}-theirs` : word,
      );
      expect(merged, `seed ${seed}`).toEqual({ clean: true, merged: expected.join(' ') });
      expect(mergeText(base.join(' '), theirs.join(' '), mine.join(' ')), `seed ${seed}`).toEqual(merged);
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
