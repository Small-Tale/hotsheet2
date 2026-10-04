import { describe, expect, it } from 'vitest';

import {
  createTerminalNameWriteQueue,
  defaultTerminalName,
  defaultTerminalNames,
  parseTerminalNames,
  reconcileLocalTerminalNames,
  restoreDefaultTerminalTitle,
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
  it('resets a renamed terminal to its default and back again across every transition (HS2-2Q7KTX)', () => {
    const groups = [
      {
        projectId: 'p',
        sessions: [
          { id: 'a', title: 'Claude 1', defaultTitle: 'Claude 1', named: false },
          { id: 'b', title: 'Terminal 1', named: false },
        ],
      },
      { projectId: 'q', sessions: [{ id: 'a', title: 'Claude 1', defaultTitle: 'Claude 1', named: false }] },
    ];
    // Unnamed → reset is a no-op that keeps identity.
    expect(restoreDefaultTerminalTitle(groups, 'p', 'a')).toBe(groups);
    // Rename marks the session named.
    const renamed = retitleTerminal(groups, 'p', 'a', 'Review');
    expect(renamed[0].sessions[0]).toMatchObject({ title: 'Review', named: true, defaultTitle: 'Claude 1' });
    // Reset restores the default title and clears the named flag, touching only that project.
    const reset = restoreDefaultTerminalTitle(renamed, 'p', 'a');
    expect(reset[0].sessions[0]).toMatchObject({ title: 'Claude 1', named: false });
    expect(reset[0].sessions[1]).toBe(renamed[0].sessions[1]);
    expect(reset[1]).toBe(groups[1]);
    // Repeating the reset changes nothing.
    expect(restoreDefaultTerminalTitle(reset, 'p', 'a')).toBe(reset);
    // A rename to exactly the default text still counts as a rename the user can reset.
    const sameText = retitleTerminal(reset, 'p', 'a', 'Claude 1');
    expect(sameText).not.toBe(reset);
    expect(sameText[0].sessions[0]).toMatchObject({ title: 'Claude 1', named: true });
    expect(restoreDefaultTerminalTitle(sameText, 'p', 'a')[0].sessions[0]).toMatchObject({ named: false });
    // Rename again after the reset (empty-then-refill).
    expect(retitleTerminal(reset, 'p', 'a', 'Again')[0].sessions[0]).toMatchObject({ title: 'Again', named: true });
    // Without a recorded default, or for an unknown terminal/project, nothing changes.
    const named = retitleTerminal(groups, 'p', 'b', 'Logs');
    expect(restoreDefaultTerminalTitle(named, 'p', 'b')).toBe(named);
    expect(restoreDefaultTerminalTitle(named, 'p', 'missing')).toBe(named);
    expect(restoreDefaultTerminalTitle(named, 'missing', 'a')).toBe(named);
  });
});

describe('terminal name write queue (HS2-0E7Q6E)', () => {
  /** A write whose completion the test controls, recording when it starts. */
  function deferredWrites() {
    const started: string[] = [],
      finish = new Map<string, (ok: boolean) => void>();
    const write = (label: string) => () =>
      new Promise<void>((resolve, reject) => {
        started.push(label);
        finish.set(label, (ok) => {
          if (ok) resolve();
          else reject(new Error(label));
        });
      });
    const settle = async (label: string, ok = true) => {
      finish.get(label)!(ok);
      for (let tick = 0; tick < 5; tick += 1) await Promise.resolve();
    };
    return { started, write, settle };
  }

  it('runs one write per terminal at a time, in user order, and drains to not pending', async () => {
    const queue = createTerminalNameWriteQueue(),
      { started, write, settle } = deferredWrites();
    expect(queue.has('p:t1')).toBe(false);
    const first = queue.enqueue('p:t1', write('rename A'));
    expect(started).toEqual(['rename A']);
    expect(queue.has('p:t1')).toBe(true);
    void queue.enqueue('p:t1', write('reset'));
    // The reset waits for the rename instead of racing it to the server.
    expect(started).toEqual(['rename A']);
    await settle('rename A');
    await first;
    expect(started).toEqual(['rename A', 'reset']);
    expect(queue.has('p:t1')).toBe(true);
    await settle('reset');
    expect(queue.has('p:t1')).toBe(false);
  });

  it('coalesces intents queued behind a running write to the latest one', async () => {
    const queue = createTerminalNameWriteQueue(),
      { started, write, settle } = deferredWrites(),
      settled: string[] = [];
    void queue.enqueue('p:t1', write('rename A'));
    void queue.enqueue('p:t1', write('rename B')).then(() => settled.push('B'));
    void queue.enqueue('p:t1', write('reset')).then(() => settled.push('reset'));
    await settle('rename A');
    // "rename B" never runs: the reset superseded it before it started; both callers settle together.
    expect(started).toEqual(['rename A', 'reset']);
    expect(settled).toEqual([]);
    await settle('reset');
    expect(settled).toEqual(['B', 'reset']);
    expect(queue.has('p:t1')).toBe(false);
  });

  it('continues after a failed write, keeps terminals independent, and refills after draining', async () => {
    const queue = createTerminalNameWriteQueue(),
      { started, write, settle } = deferredWrites();
    void queue.enqueue('p:t1', write('t1 rename'));
    void queue.enqueue('p:t2', write('t2 rename'));
    // Another terminal's write never waits on this one.
    expect(started).toEqual(['t1 rename', 't2 rename']);
    void queue.enqueue('p:t1', write('t1 reset'));
    await settle('t1 rename', false);
    expect(started).toContain('t1 reset');
    await settle('t1 reset');
    await settle('t2 rename');
    expect(queue.has('p:t1')).toBe(false);
    expect(queue.has('p:t2')).toBe(false);
    // Empty-then-refill: a new write after draining runs immediately.
    void queue.enqueue('p:t1', write('t1 again'));
    expect(started.at(-1)).toBe('t1 again');
    expect(queue.has('p:t1')).toBe(true);
    await settle('t1 again');
    expect(queue.has('p:t1')).toBe(false);
  });

  it('serves as the pending set reconciliation consults', () => {
    const queue = createTerminalNameWriteQueue();
    void queue.enqueue(terminalNameKey('p', 't1'), () => new Promise(() => undefined));
    const { upload } = reconcileLocalTerminalNames(
      'p',
      [{ id: 't1' }],
      { [terminalNameKey('p', 't1')]: 'Local' },
      queue,
    );
    // A terminal with a write in flight is not re-uploaded.
    expect(upload).toEqual([]);
  });
});
