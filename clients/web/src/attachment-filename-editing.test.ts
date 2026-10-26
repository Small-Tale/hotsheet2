import { signal } from 'kerfjs';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { type AttachmentFilenameEditing, createAttachmentFilenameEditor } from './attachment-filename-editing';
import { AttachmentFilenameEditor } from './components/attachment-filename-editor';

function setup() {
  const stored = new Map<string, string>();
  const storage = {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => {
      stored.set(key, value);
    },
    removeItem: (key: string) => {
      stored.delete(key);
    },
  } as Storage;
  const editing = signal<AttachmentFilenameEditing | undefined>(undefined);
  const editor = createAttachmentFilenameEditor(editing, () => storage);
  const edit = { scope: 'reader-a', ticketKey: 'git:a', id: 'file-a', original: 'original.txt' };
  return { editor, editing, stored, edit };
}
afterEach(() => vi.useRealTimers());

describe('inline attachment filename editing (HS2-WMZ6Z5)', () => {
  it('retains conflicting recovery and a rapid switch draft without issuing a server write', async () => {
    vi.useFakeTimers();
    const h = setup(),
      save = vi.fn<() => Promise<void>>();
    h.stored.set('first', JSON.stringify({ base: 'before.txt', draft: 'my-edit.txt' }));
    h.editor.begin(h.edit, 'first', save);
    expect(h.editing.value).toMatchObject({ draft: 'my-edit.txt' });
    expect(h.editing.value?.message).toContain('conflicts');
    h.editor.input('rapid-edit.txt');
    h.editor.begin({ ...h.edit, ticketKey: 'other:b' }, 'second', save);
    expect(JSON.parse(h.stored.get('first') ?? '{}')).toMatchObject({ draft: 'rapid-edit.txt' });
    await vi.advanceTimersByTimeAsync(150);
    expect(save).not.toHaveBeenCalled();
    h.editor.cancel();
    h.editor.dispose();
  });

  it('fences an in-flight owner and coalesces repeated finish requests before a later edit', async () => {
    const h = setup();
    let resolve!: () => void;
    const pending = new Promise<void>((done) => {
      resolve = done;
    });
    const save = vi.fn(() => pending);
    h.editor.begin(h.edit, 'first', save);
    h.editor.input('pending.txt');
    const first = h.editor.finish();
    const repeated = h.editor.finish();
    expect(h.editing.value?.saving).toBe(true);
    h.editor.input('cannot-retarget.txt');
    h.editor.cancel();
    expect(h.editor.begin({ ...h.edit, id: 'different' }, 'second', vi.fn())).toBe(false);
    expect(h.editing.value?.draft).toBe('pending.txt');
    resolve();
    expect(await first).toBe(true);
    expect(await repeated).toBe(true);
    expect(save).toHaveBeenCalledExactlyOnceWith('pending.txt');
    h.editor.begin(h.edit, 'third', () => Promise.resolve());
    h.editor.cancel();
    h.editor.dispose();
  });

  it('does not write while typing, captures the destination and retains a failed draft for retry', async () => {
    vi.useFakeTimers();
    const h = setup(),
      save = vi
        .fn<(name: string) => Promise<void>>()
        .mockRejectedValueOnce(new Error('offline'))
        .mockResolvedValue(undefined);
    h.editor.begin(h.edit, 'recovery-a', save);
    h.editor.input(' renamed.txt ');
    await vi.advanceTimersByTimeAsync(150);
    expect(save).not.toHaveBeenCalled();
    expect(JSON.parse(h.stored.get('recovery-a') ?? '{}')).toMatchObject({
      base: 'original.txt',
      draft: ' renamed.txt ',
    });
    expect(await h.editor.finish()).toBe(false);
    expect(h.editing.value).toMatchObject({ draft: ' renamed.txt ', message: 'offline', saving: false });
    expect(String(AttachmentFilenameEditor({ edit: h.editing.value, name: h.edit.original }))).toContain('offline');
    expect(await h.editor.finish()).toBe(true);
    expect(save.mock.calls).toEqual([['renamed.txt'], ['renamed.txt']]);
    expect(h.stored.size).toBe(0);
    expect(h.editing.value).toBeUndefined();
    h.editor.dispose();
  });

  it('restores local recovery, cancels, skips blank/unchanged writes, and supports a later edit', async () => {
    vi.useFakeTimers();
    const h = setup(),
      save = vi.fn<(name: string) => Promise<void>>().mockResolvedValue(undefined);
    h.stored.set('key', JSON.stringify({ base: 'original.txt', draft: 'recovered.txt' }));
    h.editor.begin(h.edit, 'key', save);
    expect(h.editing.value?.draft).toBe('recovered.txt');
    h.editor.cancel();
    await vi.advanceTimersByTimeAsync(500);
    expect(h.stored.size).toBe(0);
    expect(save).not.toHaveBeenCalled();
    for (const value of ['', '   ', 'original.txt']) {
      h.editor.begin(h.edit, 'key', save);
      h.editor.input(value);
      expect(await h.editor.finish()).toBe(true);
      expect(h.editing.value).toBeUndefined();
    }
    expect(save).not.toHaveBeenCalled();
    h.editor.begin({ ...h.edit, scope: 'inspector', ticketKey: 'other:b' }, 'other', save);
    h.editor.input('later.txt');
    await h.editor.finish();
    expect(save).toHaveBeenCalledExactlyOnceWith('later.txt');
    h.editor.dispose();
  });
});
