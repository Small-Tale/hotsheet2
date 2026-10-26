import { describe, expect, it, vi } from 'vitest';

import {
  createDraftFileRecovery,
  type DraftFileStore,
  unrestoredDraftFilesMessage,
  unsavedDraftFilesMessage,
} from './draft-file-recovery';

const file = (name: string, text = name) => new File([text], name, { type: 'text/plain' });

/** An in-memory recovery store whose saves, reads, and removals can each be made to fail. */
function memoryStore() {
  const saved = new Map<string, File>(),
    failing = { save: new Set<string>(), load: new Set<string>(), remove: false };
  const store: DraftFileStore = {
    save: vi.fn((scope: string, id: string, value: File) => {
      if (failing.save.has(id)) return Promise.reject(new Error('synthetic disk full'));
      saved.set(`${scope}:${id}`, value);
      return Promise.resolve();
    }),
    load: vi.fn((scope: string, items: readonly { id: string; name: string }[]) =>
      Promise.resolve({
        files: items.flatMap((item) => {
          const value = !failing.load.has(item.id) && saved.get(`${scope}:${item.id}`);
          return value ? [{ id: item.id, name: value.name, file: value }] : [];
        }),
        failed: items.filter((item) => failing.load.has(item.id)),
      }),
    ),
    remove: vi.fn((scope: string, ids: readonly string[]) => {
      if (failing.remove) return Promise.reject(new Error('synthetic removal failure'));
      for (const id of ids) saved.delete(`${scope}:${id}`);
      return Promise.resolve();
    }),
  };
  return { store, saved, failing };
}

describe('draft file recovery (HS2-791W19)', () => {
  it('keeps a failed save in memory, restores it from memory, and saves it on retry', async () => {
    const { store, saved, failing } = memoryStore(),
      recovery = createDraftFileRecovery(store),
      one = file('one.txt'),
      two = file('two.txt');
    failing.save.add('two');
    expect(
      await recovery.stage('composer:p', [
        { id: 'one', file: one },
        { id: 'two', file: two },
      ]),
    ).toEqual(['two']);
    expect(recovery.unsavedCount('composer:p')).toBe(1);
    expect(recovery.unsavedCount('composer:p', ['one'])).toBe(0);
    expect(recovery.unsavedCount('not-working:p')).toBe(0);
    // A project switch and return restores the unsaved file from memory, in staged order.
    const restored = await recovery.restore('composer:p', [
      { id: 'one', name: 'one.txt' },
      { id: 'two', name: 'two.txt' },
    ]);
    expect(restored.files.map((item) => item.file)).toEqual([one, two]);
    expect(restored.unrestored).toEqual([]);
    await recovery.retry();
    expect(recovery.unsavedCount('composer:p')).toBe(1);
    failing.save.clear();
    await recovery.retry();
    expect(recovery.unsavedCount('composer:p')).toBe(0);
    expect(saved.get('composer:p:two')).toBe(two);
    // Nothing unsaved: retry is a no-op that sends nothing.
    vi.mocked(store.save).mockClear();
    await recovery.retry();
    expect(store.save).not.toHaveBeenCalled();
  });

  it('never resurrects a file removed while its retry was pending or in flight', async () => {
    const { store, saved, failing } = memoryStore(),
      recovery = createDraftFileRecovery(store);
    failing.save.add('gone');
    failing.save.add('later');
    await recovery.stage('composer:p', [
      { id: 'gone', file: file('gone.txt') },
      { id: 'later', file: file('later.txt') },
    ]);
    failing.save.clear();
    await recovery.remove('composer:p', ['gone']);
    await recovery.retry();
    expect(saved.has('composer:p:gone')).toBe(false);
    expect(saved.has('composer:p:later')).toBe(true);
    // A removal that lands while a save is in flight still wins over that save's success.
    let finish = () => undefined as unknown;
    vi.mocked(store.save).mockImplementationOnce(
      (scope, id, value) =>
        new Promise<void>((resolve) => {
          finish = () => {
            saved.set(`${scope}:${id}`, value);
            resolve();
          };
        }),
    );
    const staging = recovery.stage('composer:p', [{ id: 'racing', file: file('racing.txt') }]);
    await recovery.remove('composer:p', ['racing']);
    finish();
    expect(await staging).toEqual([]);
    expect(recovery.unsavedCount('composer:p')).toBe(0);
  });

  it('HS2-791W19 blocks an in-flight copy without falsely reporting a failed save', async () => {
    const { store } = memoryStore(),
      recovery = createDraftFileRecovery(store);
    let reject!: (reason: Error) => void;
    vi.mocked(store.save).mockImplementationOnce(
      () =>
        new Promise<void>((_resolve, fail) => {
          reject = fail;
        }),
    );
    const items = [{ id: 'pending', name: 'pending.txt', file: file('pending.txt') }];
    const saving = recovery.stage('composer:p', items);
    expect(recovery.state('composer:p', items, [])).toMatchObject({ blocked: true, message: '' });
    reject(new Error('synthetic disk full'));
    await saving;
    expect(recovery.state('composer:p', items, []).message).toContain("Couldn't save");
  });

  it('coalesces concurrent retries into one pass', async () => {
    const { store, failing } = memoryStore(),
      recovery = createDraftFileRecovery(store);
    failing.save.add('one');
    await recovery.stage('composer:p', [{ id: 'one', file: file('one.txt') }]);
    failing.save.clear();
    vi.mocked(store.save).mockClear();
    const first = recovery.retry(),
      second = recovery.retry();
    expect(second).toBe(first);
    await first;
    expect(store.save).toHaveBeenCalledTimes(1);
  });

  it('keeps unreadable and missing references until explicit removal and survives a whole-store failure', async () => {
    const { store, failing } = memoryStore(),
      recovery = createDraftFileRecovery(store);
    await recovery.stage('composer:p', [
      { id: 'one', file: file('one.txt') },
      { id: 'two', file: file('two.txt') },
    ]);
    failing.load.add('two');
    const items = [
      { id: 'one', name: 'one.txt' },
      { id: 'two', name: 'two.txt' },
      { id: 'missing', name: 'missing.txt' },
    ];
    const partial = await recovery.restore('composer:p', items);
    expect(partial.files.map((item) => item.id)).toEqual(['one']);
    expect(partial.unrestored).toEqual([
      { id: 'two', name: 'two.txt' },
      { id: 'missing', name: 'missing.txt' },
    ]);
    expect(recovery.state('composer:p', partial.files, partial.unrestored).blocked).toBe(true);
    // The next reopen loads it once the store recovers.
    failing.load.clear();
    expect((await recovery.restore('composer:p', items)).files.map((item) => item.id)).toEqual(['one', 'two']);
    vi.mocked(store.load).mockRejectedValueOnce(new Error('synthetic database unavailable'));
    expect(await recovery.restore('composer:p', items)).toEqual({ files: [], unrestored: items });
  });

  it('reports a failed removal to its caller after forgetting the unsaved copy', async () => {
    const { store, failing } = memoryStore(),
      recovery = createDraftFileRecovery(store);
    failing.save.add('one');
    await recovery.stage('composer:p', [{ id: 'one', file: file('one.txt') }]);
    failing.remove = true;
    await expect(recovery.remove('composer:p', ['one'])).rejects.toThrow('synthetic removal failure');
    expect(recovery.unsavedCount('composer:p')).toBe(0);
  });
});

describe('draft file recovery messages (HS2-791W19)', () => {
  it('names one unsaved file and counts several recovery failures', () => {
    expect(unsavedDraftFilesMessage(['a.txt'])).toBe(
      "Couldn't save “a.txt” for recovery if Hot Sheet closes. It stays attached here, and saving retries with your next change. Recover or remove the affected files before submitting.",
    );
    expect(unsavedDraftFilesMessage(['a', 'b'])).toContain("Couldn't save 2 files for recovery");
    expect(unrestoredDraftFilesMessage(1)).toContain('1 staged file.');
    expect(unrestoredDraftFilesMessage(2)).toContain('2 staged files.');
  });
  it('HS2-791W19 derives all outstanding failures across healthy edits, scope changes, retry and explicit removal', async () => {
    const { store, failing } = memoryStore(),
      recovery = createDraftFileRecovery(store),
      staged = [{ id: 'a', name: 'a.txt', file: file('a.txt') }],
      unresolved = [{ id: 'b', name: 'b.txt' }];
    failing.save.add('a');
    await recovery.stage('composer:one', staged);
    expect(recovery.state('composer:one', staged, unresolved)).toMatchObject({
      blocked: true,
      attachments: [...staged, ...unresolved],
    });
    expect(recovery.state('composer:one', staged, unresolved).message).toContain("Couldn't save");
    expect(recovery.state('composer:one', staged, unresolved).message).toContain("Couldn't restore");
    expect(recovery.state('composer:one', staged, unresolved).message).toContain('Affected: b.txt.');
    const healthy = { id: 'c', name: 'c.txt', file: file('c.txt') };
    await recovery.stage('composer:one', [healthy]);
    expect(recovery.state('composer:one', [...staged, healthy], unresolved).message).toBe(
      recovery.state('composer:one', staged, unresolved).message,
    );
    expect(recovery.state('composer:two', [], []).blocked).toBe(false);
    const restored = await recovery.restore('composer:one', staged);
    expect(recovery.state('composer:one', restored.files, unresolved).message).toContain("Couldn't save");
    failing.save.clear();
    await recovery.retry();
    expect(recovery.state('composer:one', restored.files, unresolved).message).not.toContain("Couldn't save");
    expect(recovery.state('composer:one', restored.files, unresolved).blocked).toBe(true);
    await recovery.remove('composer:one', ['b']);
    expect(recovery.state('composer:one', restored.files, []).blocked).toBe(false);
  });
});
