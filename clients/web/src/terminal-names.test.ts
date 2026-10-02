import { describe, expect, it } from 'vitest';

import {
  defaultTerminalName,
  defaultTerminalNames,
  parseTerminalNames,
  reconcileLocalTerminalNames,
  retitleTerminal,
  terminalNameKey,
  terminalTitle,
  withoutTerminalName,
} from './terminal-names';

describe('terminal names', () => {
  it('replaces generated terminal ids with a stable human sequence and tidies explicit ids', () => {
    expect(defaultTerminalName('01JEDZC1ATK1BH4KGAZC3Z6W4D', 0)).toBe('Terminal 1');
    expect(defaultTerminalName('codex-main', 1)).toBe('Codex Main');
    expect(defaultTerminalName('CI_shell', 2)).toBe('CI Shell');
  });
  it('names AI shells after their provider and numbers each provider and plain shells separately (HS2-HZK0NK)', () => {
    const ulid = (n: number) => `01JEDZC1ATK1BH4KGAZC3Z6W${String(n).padStart(2, '0')}`,
      label = (tool: string) => ({ claude: 'Claude', codex: 'Codex' })[tool] ?? tool;
    expect(
      defaultTerminalNames(
        [
          { id: ulid(1), tool: 'claude' },
          { id: ulid(2) },
          { id: ulid(3), tool: 'codex' },
          { id: ulid(4), tool: 'claude' },
          { id: 'codex-main', tool: 'codex' },
          { id: ulid(5) },
          { id: ulid(6), tool: 'codex' },
        ],
        label,
      ),
    ).toEqual(['Claude 1', 'Terminal 1', 'Codex 1', 'Claude 2', 'Codex Main', 'Terminal 2', 'Codex 2']);
    // A provider the label lookup does not know still gets a readable, numbered default.
    expect(defaultTerminalNames([{ id: ulid(7), tool: 'opencode' }], (tool) => tool.toUpperCase())).toEqual([
      'OPENCODE 1',
    ]);
    expect(defaultTerminalNames([], label)).toEqual([]);
  });
  it('loads only non-empty string overrides and scopes their keys to a project', () => {
    expect(terminalNameKey('project', 'terminal')).toBe('project:terminal');
    expect(parseTerminalNames('{"one":"  Build shell  ","bad":2,"empty":" "}')).toEqual({ one: 'Build shell' });
    expect(parseTerminalNames('bad json')).toEqual({});
  });
  it('titles a tab from an unsent local rename, then the shared server name, then the default (HS2-89FPV1)', () => {
    expect(terminalTitle('Local', 'Server', 'Terminal 1')).toBe('Local');
    expect(terminalTitle(undefined, 'Server', 'Terminal 1')).toBe('Server');
    expect(terminalTitle('  ', ' ', 'Terminal 1')).toBe('Terminal 1');
    expect(terminalTitle(undefined, undefined, 'Terminal 1')).toBe('Terminal 1');
  });
  it('removes one local name without disturbing the rest', () => {
    const names = { 'p:a': 'A', 'p:b': 'B' };
    expect(withoutTerminalName(names, 'p:a')).toEqual({ 'p:b': 'B' });
    expect(withoutTerminalName(names, 'p:missing')).toBe(names);
  });
  it('uploads settled local names the server lacks, drops superseded ones, and leaves in-flight renames alone', () => {
    const local = { 'p:a': 'Alpha', 'p:b': 'Beta', 'p:c': 'Gamma', 'other:a': 'Elsewhere' };
    expect(
      reconcileLocalTerminalNames(
        'p',
        [{ id: 'a' }, { id: 'b', name: 'Server beta' }, { id: 'c' }, { id: 'd' }],
        local,
        new Set(['p:c']),
      ),
    ).toEqual({ upload: [{ terminalId: 'a', name: 'Alpha' }], drop: ['p:b'] });
    expect(reconcileLocalTerminalNames('p', [], local, new Set())).toEqual({ upload: [], drop: [] });
  });
  it('retitles only the matching project terminal and keeps identity when nothing changes', () => {
    const groups = [
      {
        projectId: 'p',
        sessions: [
          { id: 'a', title: 'Terminal 1' },
          { id: 'b', title: 'Terminal 2' },
        ],
      },
      { projectId: 'q', sessions: [{ id: 'a', title: 'Terminal 1' }] },
    ];
    const renamed = retitleTerminal(groups, 'p', 'a', 'Build');
    expect(renamed[0].sessions.map((session) => session.title)).toEqual(['Build', 'Terminal 2']);
    expect(renamed[1]).toBe(groups[1]);
    expect(retitleTerminal(renamed, 'p', 'a', 'Build')).toBe(renamed);
    expect(retitleTerminal(groups, 'p', 'missing', 'Build')).toBe(groups);
  });
});
