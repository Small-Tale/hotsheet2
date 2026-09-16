import { describe, expect, it } from 'vitest';

import type { CommandDefinition } from './api';
import { commandGroupSections, emptyExtraGroups, reorderCommands } from './command-order';

const cmd = (id: string, group?: string): CommandDefinition => ({ id, title: id, kind: 'shell', command: 'x', ...(group ? { group } : {}) });

describe('reorderCommands', () => {
  it('moves a row before another and adopts its group', () => {
    const next = reorderCommands([cmd('a', 'Q'), cmd('b', 'Q'), cmd('c', 'R')], 'c', { kind: 'row', id: 'a', position: 'before' });
    expect(next.map(c => `${c.id}:${c.group ?? ''}`)).toEqual(['c:Q', 'a:Q', 'b:Q']);
  });

  it('moves a row after another', () => {
    const next = reorderCommands([cmd('a', 'Q'), cmd('b', 'Q')], 'a', { kind: 'row', id: 'b', position: 'after' });
    expect(next.map(c => c.id)).toEqual(['b', 'a']);
  });

  it('dropping onto an ungrouped row clears the moved command group', () => {
    const next = reorderCommands([cmd('a'), cmd('b', 'Q')], 'b', { kind: 'row', id: 'a', position: 'before' });
    expect(next.find(c => c.id === 'b')?.group).toBeUndefined();
  });

  it('dropping into a group appends after that group and sets the group', () => {
    const next = reorderCommands([cmd('a', 'Q'), cmd('b', 'R'), cmd('c')], 'c', { kind: 'group', group: 'Q' });
    expect(next.map(c => `${c.id}:${c.group ?? ''}`)).toEqual(['a:Q', 'c:Q', 'b:R']);
  });

  it('dropping into an empty group appends to the end with that group', () => {
    const next = reorderCommands([cmd('a', 'Q')], 'a', { kind: 'group', group: 'New' });
    expect(next.map(c => `${c.id}:${c.group ?? ''}`)).toEqual(['a:New']);
  });

  it('is a no-op for an unknown source or self-drop', () => {
    const commands = [cmd('a', 'Q')];
    expect(reorderCommands(commands, 'zz', { kind: 'row', id: 'a', position: 'before' })).toEqual(commands);
    expect(reorderCommands(commands, 'a', { kind: 'row', id: 'a', position: 'after' })).toEqual(commands);
  });
});

describe('commandGroupSections', () => {
  it('renders ungrouped first, then named groups in first-seen order, then empty extra groups', () => {
    const sections = commandGroupSections([cmd('u'), cmd('a', 'Quality'), cmd('b', 'Release'), cmd('c', 'Quality')], ['Release', 'Ideas']);
    expect(sections.map(s => s.group)).toEqual(['', 'Quality', 'Release', 'Ideas']);
    expect(sections.find(s => s.group === 'Quality')?.commands.map(c => c.id)).toEqual(['a', 'c']);
    expect(sections.find(s => s.group === 'Ideas')?.commands).toEqual([]);
  });

  it('omits the ungrouped section when there are no ungrouped commands', () => {
    expect(commandGroupSections([cmd('a', 'Q')]).map(s => s.group)).toEqual(['Q']);
  });
});

describe('emptyExtraGroups', () => {
  it('returns only extra groups that have no commands, de-duplicated and ordered', () => {
    expect(emptyExtraGroups([cmd('a', 'Quality')], ['Ideas', 'Quality', 'Ideas', 'Later'])).toEqual(['Ideas', 'Later']);
  });
});
