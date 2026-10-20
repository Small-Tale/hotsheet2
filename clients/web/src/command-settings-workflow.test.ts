import { signal } from 'kerfjs';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { CommandDefinition } from './api';
import {
  addCommandGroupName,
  commandDefinitionsFromDraft,
  commandGroupsAfterEdit,
  CommandSettingsSaves,
  editCommandAiSelection,
  editCommandDefinition,
  nextCommandDefinition,
  reorderCommandSettings,
  validateCommandDefinitions,
} from './command-settings-workflow';
import { createCommandsController } from './features/commands';
import type { Project } from './interactions/types';

const shell = (id: string, group?: string): CommandDefinition => ({
  id,
  title: id,
  kind: 'shell',
  command: `echo ${id}`,
  group,
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

afterEach(() => {
  vi.useRealTimers();
});

describe('command settings workflow (HS2-1M05KF)', () => {
  it('keeps the shipped controller wired to project-scoped drafts and selections', () => {
    vi.useFakeTimers();
    const project = (id: string): Project => ({
        id,
        name: id,
        root: `/work/${id}`,
        apiPath: `/api/${id}`,
        stores: [],
        compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
      }),
      selectedProjectId = signal('a'),
      owner = createCommandsController({
        projects: signal([project('a'), project('b')]),
        selectedProjectId,
        storedWorkspacePreferences: { commandGroupExpanded: true, commandGroupsCollapsed: {} },
      });
    owner.commandDefinitions.value = [shell('one')];
    owner.updateCommandSetting('a', 'one', 'title', 'First project');
    owner.selectCommandSetting('a', 'one');
    selectedProjectId.value = 'b';
    owner.commandDefinitions.value = [shell('two')];
    owner.updateCommandSetting('b', 'two', 'title', 'Second project');
    expect(owner.commandSettingsDefinitions('a')[0].title).toBe('First project');
    expect(owner.commandSettingsDefinitions('b')[0].title).toBe('Second project');
    expect(owner.commandSelection('a')).toEqual(['one']);
    expect(owner.commandSelection('b')).toEqual([]);
  });

  it('keeps partial JSON safe and validates each command kind before save', () => {
    const loaded = [shell('one')];
    expect(commandDefinitionsFromDraft('{', loaded)).toEqual(loaded);
    expect(commandDefinitionsFromDraft('{}', loaded)).toEqual(loaded);
    expect(commandDefinitionsFromDraft('[{"id":"two","title":"Two"},{"id":3,"title":"bad"}]', loaded)).toEqual([
      { id: 'two', title: 'Two' },
    ]);
    expect(nextCommandDefinition([shell('command-1'), shell('command-2')]).id).toBe('command-3');
    const program = editCommandDefinition([shell('one')], 'one', 'kind', 'program')[0];
    expect(program.command).toBeUndefined();
    expect(validateCommandDefinitions([program])).toBe('one needs a program.');
    const complete = editCommandDefinition([program], 'one', 'program', 'git')[0];
    expect(validateCommandDefinitions([complete])).toBeUndefined();
    const ai = editCommandDefinition([complete], 'one', 'kind', 'ai')[0];
    expect(ai.program).toBeUndefined();
    expect(validateCommandDefinitions([ai])).toBe('one needs an AI prompt.');
    const selected = editCommandAiSelection([ai], 'one', { tool: 'codex', model: 'model', effort: 'high' });
    expect(selected[0]).toMatchObject({ tool: 'codex', model: 'model', effort: 'high' });
    expect(editCommandAiSelection(selected, 'one', {})[0].tool).toBeUndefined();
    expect(validateCommandDefinitions([shell('duplicate'), shell('duplicate')])).toBe(
      'Command identifiers must be unique: duplicate.',
    );
  });

  it('retains emptied groups through reorder, deletion, and later reuse', () => {
    const before = [shell('a', 'Keep'), shell('b', 'Other')],
      moved = reorderCommandSettings(before, [], ['a'], { kind: 'group', group: 'Other' });
    expect(moved.definitions.map((item) => [item.id, item.group])).toEqual([
      ['b', 'Other'],
      ['a', 'Other'],
    ]);
    expect(moved.groups).toEqual(['Keep']);
    expect(commandGroupsAfterEdit(moved.definitions, [moved.definitions[0]], moved.groups)).toEqual(['Keep']);
    expect(addCommandGroupName(' New ', moved.definitions, moved.groups).groups).toEqual(['Keep', 'New']);
    expect(addCommandGroupName('Other', moved.definitions, moved.groups).message).toBe(
      'A group named "Other" already exists.',
    );
    expect(addCommandGroupName('   ', moved.definitions, moved.groups)).toEqual({});
  });

  it('debounces separately by project and flushes pending edits with keepalive', async () => {
    vi.useFakeTimers();
    const definitions = new Map([
        ['a', [shell('a')]],
        ['b', [shell('b')]],
      ]),
      calls: Array<{ path: string; options: { keepalive?: boolean } }> = [],
      messages: string[] = [],
      saves = new CommandSettingsSaves({
        projectPath: (id) => `/api/${id}`,
        definitions: (id) => definitions.get(id)!,
        groups: () => [],
        setMessage: (_id, message) => {
          messages.push(message);
        },
        commandSaved: () => undefined,
        groupsLoaded: () => undefined,
        saveCommands: async (path, _items, options) => {
          calls.push({ path, options });
          return definitions.get(path.slice(5))!;
        },
        saveGroups: async () => undefined,
      });
    saves.schedule('a');
    saves.schedule('b');
    await vi.advanceTimersByTimeAsync(300);
    saves.schedule('a');
    await vi.advanceTimersByTimeAsync(300);
    expect(calls).toEqual([{ path: '/api/b', options: {} }]);
    saves.flush();
    await Promise.resolve();
    expect(calls).toEqual([
      { path: '/api/b', options: {} },
      { path: '/api/a', options: { keepalive: true } },
    ]);
    await vi.advanceTimersByTimeAsync(600);
    expect(calls).toHaveLength(2);
    expect(messages).toEqual(['', '']);
  });

  it('ignores stale save success/errors after a newer edit or project replacement', async () => {
    vi.useFakeTimers();
    const paths = new Map([['a', '/api/a']]),
      definitions = [shell('a')],
      requests: Array<ReturnType<typeof deferred<CommandDefinition[]>>> = [],
      saved: string[] = [],
      messages: string[] = [],
      saves = new CommandSettingsSaves({
        projectPath: (id) => paths.get(id),
        definitions: () => definitions,
        groups: () => [],
        setMessage: (_id, message) => {
          messages.push(message);
        },
        commandSaved: (_id, items) => {
          saved.push(items[0].title);
        },
        groupsLoaded: () => undefined,
        saveCommands: () => {
          const request = deferred<CommandDefinition[]>();
          requests.push(request);
          return request.promise;
        },
        saveGroups: async () => undefined,
      });
    const first = saves.save('a');
    saves.schedule('a'); // A new draft invalidates the first response before the debounce fires.
    requests[0].resolve([{ ...shell('a'), title: 'stale' }]);
    await first;
    expect(saved).toEqual([]);
    const second = saves.save('a'),
      third = saves.save('a');
    requests[2].resolve([{ ...shell('a'), title: 'latest' }]);
    await third;
    requests[1].reject(new Error('old error'));
    await second;
    expect(saved).toEqual(['latest']);
    expect(messages.at(-1)).toBe('');
    const fourth = saves.save('a');
    paths.set('a', '/api/reopened');
    requests[3].resolve([{ ...shell('a'), title: 'old project' }]);
    await fourth;
    expect(saved).toEqual(['latest']);
  });

  it('serializes group writes and ignores refreshes until the latest group save settles', async () => {
    const first = deferred<undefined>(),
      second = deferred<undefined>(),
      groups = ['Keep'],
      calls: string[][] = [],
      loaded: string[][] = [],
      saves = new CommandSettingsSaves({
        projectPath: () => '/api/a',
        definitions: () => [],
        groups: () => [...groups],
        setMessage: () => undefined,
        commandSaved: () => undefined,
        groupsLoaded: (_id, values) => {
          loaded.push([...values]);
        },
        saveCommands: async () => [],
        saveGroups: (_path, values) => {
          calls.push(values);
          return calls.length === 1 ? first.promise : second.promise;
        },
      });
    const one = saves.saveGroups('a');
    await vi.waitFor(() => {
      expect(calls).toEqual([['Keep']]);
    });
    groups.push('Later');
    const two = saves.saveGroups('a');
    saves.loadGroups('a', ['stale']);
    expect(loaded).toEqual([]);
    expect(calls).toEqual([['Keep']]);
    first.resolve(undefined);
    await one;
    await vi.waitFor(() => {
      expect(calls).toEqual([['Keep'], ['Keep', 'Later']]);
    });
    expect(calls).toEqual([['Keep'], ['Keep', 'Later']]);
    saves.loadGroups('a', ['stale']);
    expect(loaded).toEqual([]);
    second.resolve(undefined);
    await two;
    saves.loadGroups('a', ['Keep', 'Later']);
    expect(loaded).toEqual([['Keep', 'Later']]);
  });
});
