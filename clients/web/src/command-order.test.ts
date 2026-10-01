import { describe, expect, it } from 'vitest';

import type { CommandDefinition } from './api';
import {
  commandGroupSections,
  keptCommandGroups,
  normalizeCommandGroups,
  reorderCommands,
  reorderCommandsMultiple,
} from './command-order';

const cmd = (id: string, group?: string): CommandDefinition => ({
  id,
  title: id,
  kind: 'shell',
  command: 'x',
  ...(group ? { group } : {}),
});

describe('reorderCommands', () => {
  it('moves a row before another and adopts its group', () => {
    const next = reorderCommands([cmd('a', 'Q'), cmd('b', 'Q'), cmd('c', 'R')], 'c', {
      kind: 'row',
      id: 'a',
      position: 'before',
    });
    expect(next.map((c) => `${c.id}:${c.group ?? ''}`)).toEqual(['c:Q', 'a:Q', 'b:Q']);
  });

  it('moves a row after another', () => {
    const next = reorderCommands([cmd('a', 'Q'), cmd('b', 'Q')], 'a', { kind: 'row', id: 'b', position: 'after' });
    expect(next.map((c) => c.id)).toEqual(['b', 'a']);
  });

  it('dropping onto an ungrouped row clears the moved command group', () => {
    const next = reorderCommands([cmd('a'), cmd('b', 'Q')], 'b', { kind: 'row', id: 'a', position: 'before' });
    expect(next.find((c) => c.id === 'b')?.group).toBeUndefined();
  });

  it('dropping into a group appends after that group and sets the group', () => {
    const next = reorderCommands([cmd('a', 'Q'), cmd('b', 'R'), cmd('c')], 'c', { kind: 'group', group: 'Q' });
    expect(next.map((c) => `${c.id}:${c.group ?? ''}`)).toEqual(['a:Q', 'c:Q', 'b:R']);
  });

  it('dropping into an empty group appends to the end with that group', () => {
    const next = reorderCommands([cmd('a', 'Q')], 'a', { kind: 'group', group: 'New' });
    expect(next.map((c) => `${c.id}:${c.group ?? ''}`)).toEqual(['a:New']);
  });

  it('is a no-op for an unknown source or self-drop', () => {
    const commands = [cmd('a', 'Q')];
    expect(reorderCommands(commands, 'zz', { kind: 'row', id: 'a', position: 'before' })).toEqual(commands);
    expect(reorderCommands(commands, 'a', { kind: 'row', id: 'a', position: 'after' })).toEqual(commands);
  });
});

describe('reorderCommandsMultiple', () => {
  it('moves a multi-selection as one block, preserving order and adopting the target group (HS2-VJYQHG)', () => {
    const next = reorderCommandsMultiple([cmd('a', 'Q'), cmd('b', 'R'), cmd('c', 'R'), cmd('d', 'Q')], ['a', 'd'], {
      kind: 'row',
      id: 'c',
      position: 'after',
    });
    // a and d move together after c and adopt group R, keeping their original a-before-d order.
    expect(next.map((c) => `${c.id}:${c.group ?? ''}`)).toEqual(['b:R', 'c:R', 'a:R', 'd:R']);
  });

  it('moves a multi-selection into a group area appended after that group', () => {
    const next = reorderCommandsMultiple([cmd('a'), cmd('b', 'Q'), cmd('c'), cmd('d', 'Q')], ['a', 'c'], {
      kind: 'group',
      group: 'Q',
    });
    expect(next.map((c) => `${c.id}:${c.group ?? ''}`)).toEqual(['b:Q', 'd:Q', 'a:Q', 'c:Q']);
  });

  it('is a no-op when the selection is dropped onto one of its own rows', () => {
    const commands = [cmd('a', 'Q'), cmd('b', 'Q'), cmd('c', 'R')];
    expect(reorderCommandsMultiple(commands, ['a', 'b'], { kind: 'row', id: 'a', position: 'before' })).toEqual(
      commands,
    );
  });

  it('delegates a single-id selection to reorderCommands', () => {
    const commands = [cmd('a', 'Q'), cmd('b', 'Q'), cmd('c', 'R')];
    expect(reorderCommandsMultiple(commands, ['c'], { kind: 'row', id: 'a', position: 'before' })).toEqual(
      reorderCommands(commands, 'c', { kind: 'row', id: 'a', position: 'before' }),
    );
  });
});

describe('commandGroupSections', () => {
  it('renders ungrouped first, then named groups in first-seen order, then empty extra groups', () => {
    const sections = commandGroupSections(
      [cmd('u'), cmd('a', 'Quality'), cmd('b', 'Release'), cmd('c', 'Quality')],
      ['Release', 'Ideas'],
    );
    expect(sections.map((s) => s.group)).toEqual(['', 'Quality', 'Release', 'Ideas']);
    expect(sections.find((s) => s.group === 'Quality')?.commands.map((c) => c.id)).toEqual(['a', 'c']);
    expect(sections.find((s) => s.group === 'Ideas')?.commands).toEqual([]);
  });

  it('omits the ungrouped section when there are no ungrouped commands', () => {
    expect(commandGroupSections([cmd('a', 'Q')]).map((s) => s.group)).toEqual(['Q']);
  });
});

describe('normalizeCommandGroups', () => {
  it('trims, drops blanks, and de-duplicates while keeping first-seen order', () => {
    expect(normalizeCommandGroups([' Ideas ', '', 'Later', 'Ideas', '  '])).toEqual(['Ideas', 'Later']);
  });
});

describe('keptCommandGroups (HS2-EZ5KMC)', () => {
  it('keeps explicitly added groups whether or not they hold commands', () => {
    expect(keptCommandGroups([cmd('a', 'Quality')], [cmd('a', 'Quality')], ['Ideas', 'Quality', 'Ideas'])).toEqual([
      'Ideas',
      'Quality',
    ]);
  });

  it('keeps a group whose last command was deleted or dragged away', () => {
    expect(keptCommandGroups([cmd('a', 'Quality'), cmd('b')], [cmd('b')], [])).toEqual(['Quality']);
    expect(keptCommandGroups([cmd('a', 'Quality')], [cmd('a', 'Release')], ['Ideas'])).toEqual(['Ideas', 'Quality']);
  });

  it('never keeps the ungrouped section and leaves still-populated groups alone', () => {
    expect(keptCommandGroups([cmd('a'), cmd('b', 'Q'), cmd('c', 'Q')], [cmd('b', 'Q')], [])).toEqual([]);
  });

  it('walks add group -> populate -> empty -> delete without losing the group early', () => {
    let kept = keptCommandGroups([], [], ['Ideas']);
    expect(commandGroupSections([], kept).map((s) => s.group)).toEqual(['Ideas']);
    const populated = [cmd('a', 'Ideas')];
    kept = keptCommandGroups([], populated, kept);
    expect(commandGroupSections(populated, kept)).toEqual([{ group: 'Ideas', commands: populated }]);
    kept = keptCommandGroups(populated, [], kept);
    expect(commandGroupSections([], kept)).toEqual([{ group: 'Ideas', commands: [] }]);
    kept = kept.filter((group) => group !== 'Ideas');
    expect(commandGroupSections([], kept)).toEqual([]);
  });
});
