import { describe, expect, it } from 'vitest';

import {
  activeTerminalVisibilityGroup,
  addTerminalVisibilityGroup,
  availableTerminalVisibilityTypes,
  DEFAULT_TERMINAL_VISIBILITY_GROUP_ID,
  effectiveTerminalVisibleTypes,
  hideNewTerminalInNamedGroups,
  initialTerminalVisibilityState,
  parseTerminalVisibilityState,
  projectVisibilityToggleTarget,
  removeTerminalVisibilityGroup,
  renameTerminalVisibilityGroup,
  scopeTerminalVisibilityTypes,
  selectTerminalVisibilityGroup,
  setAllTerminalsVisibleInGroup,
  setTerminalTypeVisibleInGroup,
  setTerminalVisibleInGroup,
  TERMINAL_VISIBILITY_TYPES,
  terminalVisibilityItems,
  terminalVisibilityItemsNoun,
  type TerminalVisibilityState,
  type TerminalVisibilityType,
  terminalVisibilityTypes,
  toggleEffectiveTerminalTypeInGroup,
  visibleTerminalTypes,
} from './terminal-visibility';

describe('terminal visibility groups', () => {
  it('parses persisted groups tolerantly and restores invalid scopes to Default', () => {
    const parsed = parseTerminalVisibilityState(
      JSON.stringify({
        groups: [
          {
            id: 'focus',
            name: ' Focus ',
            hiddenKeys: ['project:a', 'project:a', 3],
            hiddenTypes: ['chat', 'chat', 'browser', 3],
          },
        ],
        activeByScope: { dashboard: 'missing', 'project:p': 'focus' },
      }),
    );
    expect(parsed.groups.map((group) => group.id)).toEqual(['default', 'focus']);
    expect(parsed.groups[1]).toMatchObject({ name: 'Focus', hiddenKeys: ['project:a'], hiddenTypes: ['chat'] });
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
  it('keeps type visibility per group through repeated toggles, scope switches, and persistence', () => {
    let state = addTerminalVisibilityGroup(initialTerminalVisibilityState(), 'focus').state;
    state = selectTerminalVisibilityGroup(state, 'dashboard', 'focus');
    for (const type of TERMINAL_VISIBILITY_TYPES) {
      state = setTerminalTypeVisibleInGroup(state, 'focus', type, false);
      expect(visibleTerminalTypes(activeTerminalVisibilityGroup(state, 'dashboard'))).not.toContain(type);
    }
    expect(visibleTerminalTypes(activeTerminalVisibilityGroup(state, 'dashboard'))).toEqual([]);
    expect(visibleTerminalTypes(activeTerminalVisibilityGroup(state, 'project:p'))).toEqual(TERMINAL_VISIBILITY_TYPES);
    state = setTerminalTypeVisibleInGroup(state, 'focus', 'chat', true);
    state = setTerminalTypeVisibleInGroup(state, 'focus', 'chat', true);
    expect(visibleTerminalTypes(activeTerminalVisibilityGroup(state, 'dashboard'))).toEqual(['chat']);
    state = parseTerminalVisibilityState(JSON.stringify(state));
    expect(visibleTerminalTypes(activeTerminalVisibilityGroup(state, 'dashboard'))).toEqual(['chat']);
    state = selectTerminalVisibilityGroup(state, 'dashboard', 'default');
    expect(visibleTerminalTypes(activeTerminalVisibilityGroup(state, 'dashboard'))).toEqual(TERMINAL_VISIBILITY_TYPES);
    state = selectTerminalVisibilityGroup(state, 'dashboard', 'focus');
    expect(visibleTerminalTypes(activeTerminalVisibilityGroup(state, 'dashboard'))).toEqual(['chat']);
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
          title: 'Plain Terminal',
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

  it('normalizes duplicates, unsupported future types, and stale bulk-action sentinels', () => {
    expect(terminalVisibilityTypes(['ai', 'ai', 'browser', 'unknown'])).toEqual(['ai']);
    expect(terminalVisibilityTypes(['ai', 'deselect-all'])).toEqual(['ai']);
    expect(terminalVisibilityTypes(['select-all'])).toEqual([]);
    expect(terminalVisibilityTypes([...TERMINAL_VISIBILITY_TYPES])).toEqual(TERMINAL_VISIBILITY_TYPES);
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
    state = setAllTerminalsVisibleInGroup(state, 'focus', keys(TERMINAL_VISIBILITY_TYPES), true);
    state = setAllTerminalsVisibleInGroup(state, 'focus', keys(['ai']), false);
    expect(state.groups[1].hiddenKeys).toEqual(['one:agent']);
  });
});

describe('workspace visibility segment availability (HS2-4TKASN)', () => {
  const group = (hiddenTypes?: TerminalVisibilityType[]) => ({
    id: 'default',
    name: 'Default',
    hiddenKeys: [],
    hiddenTypes,
  });
  const session = { id: 's', projectId: 'p', projectName: 'P', alive: true, busy: false, scrollback: '' };
  const chat = { id: 'ai-chat:c', projectId: 'p', projectName: 'P', name: 'Chat', tool: 'Codex' };

  it('offers AI Chats only while the feature is on or a chat is listed', () => {
    expect(availableTerminalVisibilityTypes(true, false)).toEqual(['shell', 'ai', 'chat']);
    expect(availableTerminalVisibilityTypes(true, true)).toEqual(['shell', 'ai', 'chat']);
    expect(availableTerminalVisibilityTypes(false, true)).toEqual(['shell', 'ai', 'chat']);
    expect(availableTerminalVisibilityTypes(false, false)).toEqual(['shell', 'ai']);
  });

  it('reads chats from the listed scope only', () => {
    const groups = [
      { projectId: 'p', projectName: 'P', sessions: [session] },
      { projectId: 'q', projectName: 'Q', sessions: [], chats: [{ ...chat, projectId: 'q', projectName: 'Q' }] },
    ];
    expect(scopeTerminalVisibilityTypes(groups, 'dashboard', false)).toEqual(['shell', 'ai', 'chat']);
    expect(scopeTerminalVisibilityTypes(groups, 'project:p', false)).toEqual(['shell', 'ai']);
    expect(scopeTerminalVisibilityTypes(groups, 'project:q', false)).toEqual(['shell', 'ai', 'chat']);
    expect(scopeTerminalVisibilityTypes([], 'dashboard', false)).toEqual(['shell', 'ai']);
    expect(scopeTerminalVisibilityTypes([], 'dashboard', true)).toEqual(['shell', 'ai', 'chat']);
  });

  it('never applies an unavailable type and falls back to the first available one', () => {
    const available: TerminalVisibilityType[] = ['shell', 'ai'];
    expect(effectiveTerminalVisibleTypes(group(), available)).toEqual(['shell', 'ai']);
    expect(effectiveTerminalVisibleTypes(group(['shell']), available)).toEqual(['ai']);
    // A chat-only choice falls back to Terminals instead of showing nothing.
    expect(effectiveTerminalVisibleTypes(group(['shell', 'ai']), available)).toEqual(['shell']);
    // An explicit "nothing shown" stays empty.
    expect(effectiveTerminalVisibleTypes(group(['shell', 'ai', 'chat']), available)).toEqual([]);
    expect(effectiveTerminalVisibleTypes(group(['shell', 'ai']), [])).toEqual([]);
    expect(effectiveTerminalVisibleTypes(group(['shell', 'ai']))).toEqual(['chat']);
  });

  it('toggles from the applied types without trapping a fallback or losing the stored chat choice', () => {
    const available: TerminalVisibilityType[] = ['shell', 'ai'];
    let state: TerminalVisibilityState = { groups: [group(['shell', 'ai'])], activeByScope: {} };
    const applied = () => effectiveTerminalVisibleTypes(state.groups[0], available);
    expect(applied()).toEqual(['shell']);
    // Releasing the fallback stores an explicit nothing-shown choice.
    state = toggleEffectiveTerminalTypeInGroup(state, 'default', 'shell', available);
    expect(applied()).toEqual([]);
    expect(state.groups[0].hiddenTypes).toEqual(['shell', 'ai', 'chat']);
    state = toggleEffectiveTerminalTypeInGroup(state, 'default', 'ai', available);
    expect(applied()).toEqual(['ai']);
    expect(state.groups[0].hiddenTypes).toEqual(['shell', 'chat']);
    state = toggleEffectiveTerminalTypeInGroup(state, 'default', 'shell', available);
    expect(applied()).toEqual(['shell', 'ai']);
    // Chat keeps its stored choice while unavailable, and returns with it.
    expect(state.groups[0].hiddenTypes).toEqual(['chat']);
    state = toggleEffectiveTerminalTypeInGroup(state, 'default', 'chat', available);
    expect(state.groups[0].hiddenTypes).toEqual(['chat']);
    state = toggleEffectiveTerminalTypeInGroup(state, 'missing', 'shell', available);
    expect(applied()).toEqual(['shell', 'ai']);
    state = toggleEffectiveTerminalTypeInGroup(state, 'default', 'chat', TERMINAL_VISIBILITY_TYPES);
    expect(effectiveTerminalVisibleTypes(state.groups[0])).toEqual(['shell', 'ai', 'chat']);
  });
});

describe('project heading toggle (HS2-HC67SP)', () => {
  it('shows all when any listed item is hidden and hides all when all are visible', () => {
    // Mixed: one hidden shows all.
    expect(projectVisibilityToggleTarget(['p:a', 'p:b'], ['p:b'])).toBe(true);
    // All hidden shows all.
    expect(projectVisibilityToggleTarget(['p:a', 'p:b'], ['p:a', 'p:b'])).toBe(true);
    // All visible hides all; hidden keys outside the listed set do not count.
    expect(projectVisibilityToggleTarget(['p:a', 'p:b'], [])).toBe(false);
    expect(projectVisibilityToggleTarget(['p:a', 'p:b'], ['q:c'])).toBe(false);
    expect(projectVisibilityToggleTarget([], ['p:a'])).toBe(false);
  });

  it('applies the rule through repeated, interleaved, and refill sequences', () => {
    const keys = ['p:a', 'p:b', 'p:c'];
    let state = addTerminalVisibilityGroup(initialTerminalVisibilityState(), 'focus').state;
    const toggle = () =>
      (state = setAllTerminalsVisibleInGroup(
        state,
        'focus',
        keys,
        projectVisibilityToggleTarget(keys, state.groups[1].hiddenKeys),
      ));
    toggle();
    expect(state.groups[1].hiddenKeys).toEqual(keys);
    toggle();
    expect(state.groups[1].hiddenKeys).toEqual([]);
    state = setTerminalVisibleInGroup(state, 'focus', 'p:b', false);
    state = setTerminalVisibleInGroup(state, 'focus', 'q:other', false);
    toggle();
    expect(state.groups[1].hiddenKeys).toEqual(['q:other']);
    toggle();
    expect(state.groups[1].hiddenKeys).toEqual(['q:other', ...keys]);
    expect(state.groups[0].hiddenKeys).toEqual([]);
  });

  it('names the listed types in title style', () => {
    expect(terminalVisibilityItemsNoun(['ai', 'ai'])).toBe('AI Shells');
    expect(terminalVisibilityItemsNoun(['shell'])).toBe('Terminals');
    expect(terminalVisibilityItemsNoun(['chat'])).toBe('AI Chats');
    expect(terminalVisibilityItemsNoun(['shell', 'ai'])).toBe('Items');
    expect(terminalVisibilityItemsNoun([])).toBe('Items');
  });
});
