import { signal } from 'kerfjs';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ApiHttpError, type CommandDefinition } from './api';
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
  it('keeps a conflicting tab draft until reload and then saves against the new revision (HS2-X2A3C1)', async () => {
    let revision = 'r0',
      persisted = [shell('initial')];
    const makeEditor = (draft: { value: CommandDefinition[] }, messages: string[]) =>
      new CommandSettingsSaves({
        projectPath: () => '/api/p',
        definitions: () => draft.value,
        groups: () => [],
        setMessage: (_project, message) => messages.push(message),
        commandSaved: (_project, definitions) => {
          draft.value = definitions;
        },
        groupsLoaded: () => undefined,
        saveCommands: async (_path, definitions, options) => {
          if (options.expectedRevision !== revision) throw new ApiHttpError('conflict', 409);
          persisted = definitions;
          revision = `r${Number(revision.slice(1)) + 1}`;
          return { definitions, revision };
        },
        saveGroups: async () => undefined,
      });
    const firstDraft = { value: [shell('first')] },
      secondDraft = { value: [shell('second')] },
      secondMessages: string[] = [],
      first = makeEditor(firstDraft, []),
      second = makeEditor(secondDraft, secondMessages);
    first.loadRevisions('p', 'r0', 'g0');
    second.loadRevisions('p', 'r0', 'g0');
    await first.save('p');
    await second.save('p');
    expect(persisted).toEqual([shell('first')]);
    expect(secondDraft.value).toEqual([shell('second')]);
    expect(second.hasConflict('p')).toBe(true);
    expect(secondMessages.at(-1)).toContain('Copy your draft');
    second.loadRevisions('p', 'r1', 'g1');
    await second.save('p');
    expect(persisted).toEqual([shell('first')]);
    const reloaded = makeEditor(secondDraft, []);
    reloaded.loadRevisions('p', 'r1', 'g1');
    await reloaded.save('p');
    expect(persisted).toEqual([shell('second')]);
  });

  it('holds a local group edit when another tab changes kept groups (HS2-X2A3C1)', async () => {
    let revision = 'g0',
      persisted: string[] = ['Remote'];
    const draft = { value: ['Local'] },
      messages: string[] = [],
      loaded = vi.fn(),
      saves = new CommandSettingsSaves({
        projectPath: () => '/api/p',
        definitions: () => [],
        groups: () => draft.value,
        setMessage: (_project, message) => messages.push(message),
        commandSaved: () => undefined,
        groupsLoaded: loaded,
        saveCommands: async () => [],
        saveGroups: async (_path, groups, expected) => {
          if (expected !== revision) throw new ApiHttpError('conflict', 409);
          persisted = groups;
          revision = 'g1';
          return { groups, revision };
        },
      });
    saves.loadRevisions('p', 'r0', 'g0');
    revision = 'g1';
    await saves.saveGroups('p');
    expect(saves.hasConflict('p')).toBe(true);
    expect(messages.at(-1)).toContain('Copy your draft');
    saves.loadGroups('p', persisted);
    expect(loaded).not.toHaveBeenCalled();
    expect(draft.value).toEqual(['Local']);
  });
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

  it('edits an AI command run action and clears it with the default or a type change (HS2-17Y2EC)', () => {
    const ai: CommandDefinition = { id: 'one', title: 'Review', kind: 'ai', prompt: 'Review' };
    const session = editCommandDefinition([ai], 'one', 'action', 'new_ai_session')[0];
    expect(session.action).toBe('new_ai_session');
    const reuse = editCommandDefinition([session], 'one', 'action', 'reuse_or_new_ai_session')[0];
    expect(reuse.action).toBe('reuse_or_new_ai_session');
    // The editor's default choice stores no action, so the command keeps the legacy click.
    const legacy = editCommandDefinition([reuse], 'one', 'action', 'default')[0];
    expect(legacy).not.toHaveProperty('action');
    expect(editCommandDefinition([legacy], 'one', 'action', 'create_ticket')[0].action).toBe('create_ticket');
    expect(editCommandDefinition([session], 'one', 'kind', 'shell')[0]).not.toHaveProperty('action');
    expect(editCommandDefinition([session], 'one', 'kind', 'program')[0]).not.toHaveProperty('action');
    expect(editCommandDefinition([session], 'one', 'kind', 'ai')[0].action).toBe('new_ai_session');
  });

  it('toggles the show-while-scheduled option, storing nothing when off (HS2-MA31ZN)', () => {
    const ai: CommandDefinition = { id: 'one', title: 'Review', kind: 'ai', prompt: 'Review' };
    const shown = editCommandDefinition([ai], 'one', 'show_when_scheduled', 'true')[0];
    expect(shown.show_when_scheduled).toBe(true);
    const hidden = editCommandDefinition([shown], 'one', 'show_when_scheduled', 'false')[0];
    expect(hidden).not.toHaveProperty('show_when_scheduled');
    // On again after off, then a type change away from AI drops it.
    const again = editCommandDefinition([hidden], 'one', 'show_when_scheduled', 'true')[0];
    expect(again.show_when_scheduled).toBe(true);
    expect(editCommandDefinition([again], 'one', 'kind', 'shell')[0]).not.toHaveProperty('show_when_scheduled');
    expect(editCommandDefinition([again], 'one', 'kind', 'program')[0]).not.toHaveProperty('show_when_scheduled');
    expect(editCommandDefinition([again], 'one', 'kind', 'ai')[0].show_when_scheduled).toBe(true);
  });

  it('edits auto-cleanup, storing nothing for None, and drops it off AI (HS2-27C161)', () => {
    const ai: CommandDefinition = { id: 'one', title: 'Review', kind: 'ai', prompt: 'Review' };
    const archive = editCommandDefinition([ai], 'one', 'cleanup', 'archive')[0];
    expect(archive.cleanup).toBe('archive');
    const remove = editCommandDefinition([archive], 'one', 'cleanup', 'delete')[0];
    expect(remove.cleanup).toBe('delete');
    const stamped = { ...remove, cleanup_since: '2026-11-26T08:00:00Z' };
    expect(editCommandDefinition([stamped], 'one', 'cleanup', 'archive')[0].cleanup_since).toBe('2026-11-26T08:00:00Z');
    const none = editCommandDefinition([stamped], 'one', 'cleanup', 'none')[0];
    expect(none).not.toHaveProperty('cleanup');
    expect(none).not.toHaveProperty('cleanup_since');
    expect(editCommandDefinition([none], 'one', 'cleanup', 'purge')[0]).not.toHaveProperty('cleanup');
    const again = editCommandDefinition([none], 'one', 'cleanup', 'archive')[0];
    expect(again.cleanup).toBe('archive');
    expect(editCommandDefinition([again], 'one', 'kind', 'shell')[0]).not.toHaveProperty('cleanup');
    expect(editCommandDefinition([again], 'one', 'kind', 'program')[0]).not.toHaveProperty('cleanup');
    expect(editCommandDefinition([again], 'one', 'kind', 'ai')[0].cleanup).toBe('archive');
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
      calls: Array<{
        path: string;
        options: { keepalive?: boolean; writeOrder?: { session: string; sequence: number }; expectedRevision: string };
      }> = [],
      messages: string[] = [],
      nextSequence = { value: 0 },
      saves = new CommandSettingsSaves({
        projectPath: (id) => `/api/${id}`,
        definitions: (id) => definitions.get(id)!,
        groups: () => [],
        setMessage: (_id, message) => {
          messages.push(message);
        },
        commandSaved: () => undefined,
        groupsLoaded: () => undefined,
        nextWriteOrder: () => ({ session: 'editor', sequence: ++nextSequence.value }),
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
    expect(calls).toEqual([
      { path: '/api/b', options: { expectedRevision: '', writeOrder: { session: 'editor', sequence: 1 } } },
    ]);
    saves.flush();
    await Promise.resolve();
    expect(calls).toEqual([
      { path: '/api/b', options: { expectedRevision: '', writeOrder: { session: 'editor', sequence: 1 } } },
      {
        path: '/api/a',
        options: { keepalive: true, expectedRevision: '', writeOrder: { session: 'editor', sequence: 2 } },
      },
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
  it('ignores a refresh that read the server before a save settled, then accepts a later one (HS2-A6K8D5)', async () => {
    let revision = 'g0';
    const groups = ['Ideas'],
      messages: string[] = [],
      loaded: string[][] = [],
      saves = new CommandSettingsSaves({
        projectPath: () => '/api/a',
        definitions: () => [],
        groups: () => [...groups],
        setMessage: (_project, message) => messages.push(message),
        commandSaved: () => undefined,
        groupsLoaded: (_id, values) => {
          loaded.push([...values]);
        },
        saveCommands: async () => [],
        saveGroups: async (_path, values, expected) => {
          if (expected !== revision) throw new ApiHttpError('conflict', 409);
          revision = `g${String(Number(revision.slice(1)) + 1)}`;
          return { groups: values, revision };
        },
      });
    expect(saves.hasRevisions('a')).toBe(false);
    saves.loadRevisions('a', 'r0', 'g0', saves.writeVersion('a'));
    expect(saves.hasRevisions('a')).toBe(true);
    // A refresh starts, the save completes, then the refresh's older snapshot arrives.
    const stale = saves.writeVersion('a');
    await saves.saveGroups('a');
    saves.loadRevisions('a', 'r0', 'g0', stale);
    saves.loadGroups('a', [], stale);
    expect(loaded).toEqual([]);
    // The next save names the revision its own write returned, not the stale one.
    groups.push('Second');
    await saves.saveGroups('a');
    expect(messages.filter(Boolean)).toEqual([]);
    expect(saves.hasConflict('a')).toBe(false);
    expect(revision).toBe('g2');
    // A refresh that started after both writes settled is current.
    const fresh = saves.writeVersion('a');
    saves.loadRevisions('a', 'r1', 'g2', fresh);
    saves.loadGroups('a', ['Ideas', 'Second'], fresh);
    expect(loaded).toEqual([['Ideas', 'Second']]);
    // A command save in flight makes only the command revision stale.
    const beforeCommand = saves.writeVersion('a');
    await saves.save('a');
    saves.loadRevisions('a', 'r-stale', 'g2', beforeCommand);
    saves.loadGroups('a', ['Ideas', 'Second'], beforeCommand);
    expect(loaded).toEqual([
      ['Ideas', 'Second'],
      ['Ideas', 'Second'],
    ]);
  });
});
