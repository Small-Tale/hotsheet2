import { describe, expect, it } from 'vitest';

import {
  activeTerminalVisibilityGroup,
  addTerminalVisibilityGroup,
  DEFAULT_TERMINAL_VISIBILITY_GROUP_ID,
  hideNewTerminalInNamedGroups,
  initialTerminalVisibilityState,
  parseTerminalVisibilityState,
  removeTerminalVisibilityGroup,
  renameTerminalVisibilityGroup,
  selectTerminalVisibilityGroup,
  setAllTerminalsVisibleInGroup,
  setTerminalVisibleInGroup,
  TERMINAL_VISIBILITY_TYPES,
  terminalVisibilityItems,
  terminalVisibilityTypes,
} from './terminal-visibility';

describe('terminal visibility groups', () => {
  it('parses persisted groups tolerantly and restores invalid scopes to Default', () => {
    const parsed = parseTerminalVisibilityState(
      JSON.stringify({
        groups: [{ id: 'focus', name: ' Focus ', hiddenKeys: ['project:a', 'project:a', 3] }],
        activeByScope: { dashboard: 'missing', 'project:p': 'focus' },
      }),
    );
    expect(parsed.groups.map((group) => group.id)).toEqual(['default', 'focus']);
    expect(parsed.groups[1]).toMatchObject({ name: 'Focus', hiddenKeys: ['project:a'] });
    expect(activeTerminalVisibilityGroup(parsed, 'dashboard').id).toBe(DEFAULT_TERMINAL_VISIBILITY_GROUP_ID);
    expect(activeTerminalVisibilityGroup(parsed, 'project:p').id).toBe('focus');
    expect(parseTerminalVisibilityState('{')).toEqual(initialTerminalVisibilityState());
  });
  it('creates, selects, renames, and removes named groups without mutating Default', () => {
    let state = initialTerminalVisibilityState();
    const added = addTerminalVisibilityGroup(state, 'focus', ' Focus ');
    state = selectTerminalVisibilityGroup(added.state, 'dashboard', added.group.id);
    state = renameTerminalVisibilityGroup(state, 'focus', 'Builds');
    expect(activeTerminalVisibilityGroup(state, 'dashboard').name).toBe('Builds');
    expect(renameTerminalVisibilityGroup(state, 'default', 'Changed')).toBe(state);
    state = removeTerminalVisibilityGroup(state, 'focus');
    expect(activeTerminalVisibilityGroup(state, 'dashboard').id).toBe('default');
    expect(removeTerminalVisibilityGroup(state, 'default')).toBe(state);
  });
  it('keeps independent hidden sets and active choices across scopes', () => {
    let state = addTerminalVisibilityGroup(initialTerminalVisibilityState(), 'focus').state;
    state = selectTerminalVisibilityGroup(state, 'dashboard', 'focus');
    state = setTerminalVisibleInGroup(state, 'focus', 'p:a', false);
    state = setAllTerminalsVisibleInGroup(state, 'focus', ['p:a', 'p:b'], false);
    expect(activeTerminalVisibilityGroup(state, 'dashboard').hiddenKeys).toEqual(['p:a', 'p:b']);
    state = setTerminalVisibleInGroup(state, 'focus', 'p:a', true);
    expect(activeTerminalVisibilityGroup(state, 'dashboard').hiddenKeys).toEqual(['p:b']);
    expect(activeTerminalVisibilityGroup(state, 'project:p').hiddenKeys).toEqual([]);
  });
  it('keeps newly created terminals visible in Default and hidden in named views', () => {
    const state = hideNewTerminalInNamedGroups(
      addTerminalVisibilityGroup(initialTerminalVisibilityState(), 'focus').state,
      'p:new',
    );
    expect(state.groups.find((group) => group.id === 'default')?.hiddenKeys).toEqual([]);
    expect(state.groups.find((group) => group.id === 'focus')?.hiddenKeys).toEqual(['p:new']);
  });
});

describe('workspace visibility type filtering', () => {
  const groups = [
    {
      projectId: 'one',
      projectName: 'One',
      sessions: [
        {
          id: 'shell',
          projectId: 'one',
          projectName: 'One',
          title: 'Codex-looking shell',
          alive: true,
          busy: true,
          scrollback: '',
          link: 'https://ai.example',
        },
        {
          id: 'agent',
          projectId: 'one',
          projectName: 'One',
          kind: 'ai' as const,
          title: 'Plain terminal',
          alive: false,
          busy: false,
          scrollback: '',
        },
      ],
      chats: [{ id: 'ai-chat:session', projectId: 'one', projectName: 'One', name: 'Agent chat', tool: 'Codex' }],
    },
    {
      projectId: 'two',
      projectName: 'Two',
      sessions: [],
      chats: [{ id: 'ai-chat:session', projectId: 'two', projectName: 'Two', name: 'Other chat', tool: 'Claude' }],
    },
  ];
  const keys = (types: readonly string[], scope = 'dashboard') =>
    terminalVisibilityItems(groups, scope, terminalVisibilityTypes(types)).flatMap((group) =>
      group.items.map((item) => item.key),
    );

  it('normalizes special actions, duplicates and unsupported future types', () => {
    expect(terminalVisibilityTypes(['ai', 'ai', 'browser', 'unknown'])).toEqual(['ai']);
    expect(terminalVisibilityTypes(['ai', 'deselect-all'])).toEqual([]);
    expect(terminalVisibilityTypes(['select-all'])).toEqual(TERMINAL_VISIBILITY_TYPES);
    expect(terminalVisibilityTypes([])).toEqual([]);
  });

  it('covers every type subset, including chat-only projects and legacy shell metadata', () => {
    const byType = { shell: ['one:shell'], ai: ['one:agent'], chat: ['one:ai-chat:session', 'two:ai-chat:session'] };
    for (let mask = 0; mask < 8; mask++) {
      const types = TERMINAL_VISIBILITY_TYPES.filter((_, index) => mask & (1 << index));
      expect(keys(types)).toEqual(types.flatMap((type) => byType[type]));
    }
    expect(keys(['chat'], 'project:two')).toEqual(['two:ai-chat:session']);
    expect(keys(['shell'], 'project:two')).toEqual([]);
    expect(keys(['ai'], 'project:missing')).toEqual([]);
  });

  it('limits bulk edits to listed keys through empty, refill, scope and group transitions', () => {
    let state = addTerminalVisibilityGroup(initialTerminalVisibilityState(), 'focus').state;
    state = setAllTerminalsVisibleInGroup(state, 'focus', keys(['chat'], 'project:one'), false);
    state = setAllTerminalsVisibleInGroup(state, 'focus', keys([]), true);
    expect(state.groups[1].hiddenKeys).toEqual(['one:ai-chat:session']);
    state = setAllTerminalsVisibleInGroup(state, 'focus', keys(['shell']), false);
    state = setAllTerminalsVisibleInGroup(state, 'focus', keys(['chat']), true);
    expect(state.groups[1].hiddenKeys).toEqual(['one:shell']);
    expect(state.groups[0].hiddenKeys).toEqual([]);
    state = setAllTerminalsVisibleInGroup(state, 'focus', keys(['select-all']), true);
    state = setAllTerminalsVisibleInGroup(state, 'focus', keys(['ai']), false);
    expect(state.groups[1].hiddenKeys).toEqual(['one:agent']);
  });
});
