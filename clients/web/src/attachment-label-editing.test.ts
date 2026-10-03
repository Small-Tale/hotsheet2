import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  type AttachmentLabelEditing,
  attachmentLabelEditingDraft,
  attachmentLabelEditingKey,
  attachmentLabelEditScope,
  beginAttachmentLabelEdit,
  createAttachmentLabelEditor,
  endAttachmentLabelEdit,
  INSPECTOR_ATTACHMENT_LABEL_SCOPE,
  restoreAttachmentLabelEdit,
} from './attachment-label-editing';
import { groupAttachments, TicketAttachments } from './components/ticket-attachments';

interface FakeBatch {
  dataset: { attachmentBatchKey?: string };
  querySelector: (selector: string) => FakeInput | null;
}
interface FakeInput {
  value: string;
  disabled: boolean;
  closest: (selector: string) => unknown;
}

/** A batch title and its editor input inside an optional ticket reader frame. */
function batchIn(frameId: string | undefined, key: string | undefined, value = '', disabled = false) {
  const reader = frameId ? { dataset: { readerFrameId: frameId } } : null;
  const batch: FakeBatch = { dataset: { attachmentBatchKey: key }, querySelector: () => input };
  const closest = (selector: string) =>
    selector.includes('ticket-reader') ? reader : selector.includes('batch-key') ? batch : null;
  const input: FakeInput = { value, disabled, closest };
  const title = { closest };
  return { title: title as unknown as Element, input: input as unknown as HTMLInputElement };
}

describe('attachment label editing state (HS2-SG0AZY)', () => {
  it('scopes an edit to its ticket reader frame or the inspector', () => {
    expect(attachmentLabelEditScope(batchIn(undefined, 'a').title)).toBe(INSPECTOR_ATTACHMENT_LABEL_SCOPE);
    expect(attachmentLabelEditScope(batchIn('reader-2', 'a').title)).toBe('reader-2');
    const editing = { scope: 'reader-2', key: 'a', original: '' };
    expect(attachmentLabelEditingKey(editing, 'reader-2')).toBe('a');
    expect(attachmentLabelEditingKey(editing, INSPECTOR_ATTACHMENT_LABEL_SCOPE)).toBeUndefined();
    expect(attachmentLabelEditingKey(undefined, INSPECTOR_ATTACHMENT_LABEL_SCOPE)).toBeUndefined();
  });

  it('walks begin, replace, foreign end, restore, and end across surfaces', () => {
    const editing = signal<AttachmentLabelEditing | undefined>(undefined),
      inspector = batchIn(undefined, 'batch-1', 'Before'),
      reader = batchIn('workspace-reader', '__legacy__', '');
    // Ending or restoring with nothing open is a no-op.
    endAttachmentLabelEdit(inspector.input, editing);
    restoreAttachmentLabelEdit(inspector.input, editing);
    expect(editing.value).toBeUndefined();
    expect(inspector.input.value).toBe('Before');

    expect(beginAttachmentLabelEdit(inspector.title, editing)).toBe(inspector.input);
    expect(editing.value).toMatchObject({
      scope: INSPECTOR_ATTACHMENT_LABEL_SCOPE,
      key: 'batch-1',
      original: 'Before',
      draft: 'Before',
      base: 'Before',
    });
    // Repeating the gesture keeps the one open edit, its edit-start label, and its draft (HS2-0QQHSZ).
    const open = editing.value;
    inspector.input.value = 'Typed';
    expect(beginAttachmentLabelEdit(inspector.title, editing)).toBe(inspector.input);
    expect(editing.value).toBe(open);
    inspector.input.value = 'Before';

    // The reader opens its own edit; the inspector's blur no longer closes it.
    beginAttachmentLabelEdit(reader.title, editing);
    expect(editing.value).toMatchObject({ scope: 'workspace-reader', key: '__legacy__', original: '' });
    endAttachmentLabelEdit(inspector.input, editing);
    restoreAttachmentLabelEdit(inspector.input, editing);
    expect(editing.value?.scope).toBe('workspace-reader');
    expect(inspector.input.value).toBe('Before');

    // Escape restores the edit-start label; blur closes the edit.
    reader.input.value = 'Discard me';
    restoreAttachmentLabelEdit(reader.input, editing);
    expect(reader.input.value).toBe('');
    endAttachmentLabelEdit(reader.input, editing);
    expect(editing.value).toBeUndefined();

    // Empty, then refill: a fresh edit starts from the current label.
    reader.input.value = 'Renamed';
    beginAttachmentLabelEdit(reader.title, editing);
    expect(editing.value?.original).toBe('Renamed');
  });

  it('ignores read-only and unkeyed batches', () => {
    const editing = signal<AttachmentLabelEditing | undefined>(undefined);
    expect(beginAttachmentLabelEdit(batchIn(undefined, 'a', '', true).title, editing)).toBeUndefined();
    expect(beginAttachmentLabelEdit(batchIn(undefined, undefined).title, editing)).toBeUndefined();
    expect(beginAttachmentLabelEdit({ closest: () => null } as unknown as Element, editing)).toBeUndefined();
    expect(editing.value).toBeUndefined();
  });

  it('renders the editing flag from the editingLabelBatch prop, never for a read-only provider', () => {
    const attachments = [
        { id: 'a', name: 'a.png', batch_id: 'one', batch_label: 'One' },
        { id: 'b', name: 'b.png', batch_id: 'two', batch_label: 'Two' },
      ],
      [, second] = groupAttachments(attachments),
      markup = String(TicketAttachments({ attachments, editingLabelBatch: second.key }));
    expect(markup).toContain(`data-attachment-batch-key="${second.key}"`);
    expect(markup.match(/data-editing-label="true"/g)).toHaveLength(1);
    expect(markup).toMatch(/data-attachment-batch-key="two"[^>]*data-editing-label="true"/);
    expect(String(TicketAttachments({ attachments }))).not.toContain('data-editing-label');
    expect(String(TicketAttachments({ attachments, editable: false, editingLabelBatch: second.key }))).not.toContain(
      'data-editing-label',
    );
  });

  it('shows the controlled draft in the open editor only (HS2-0QQHSZ)', () => {
    const attachments = [
        { id: 'a', name: 'a.png', batch_id: 'one', batch_label: 'One' },
        { id: 'b', name: 'b.png', batch_id: 'two', batch_label: 'Two' },
      ],
      markup = String(TicketAttachments({ attachments, editingLabelBatch: 'two', editingLabelDraft: 'Two, typed' }));
    expect(markup).toContain('value="One"');
    expect(markup).toContain('value="Two, typed"');
    expect(markup).not.toContain('value="Two"');
    expect(String(TicketAttachments({ attachments, editingLabelDraft: 'Ignored' }))).not.toContain('Ignored');
  });
});

/** An in-memory localStorage stand-in. */
function memoryStorage() {
  const items = new Map<string, string>();
  return {
    items,
    getItem: (key: string) => items.get(key) ?? null,
    setItem: (key: string, value: string) => {
      items.set(key, value);
    },
    removeItem: (key: string) => {
      items.delete(key);
    },
  };
}

/** A relabellable batch (title button, editor input) in the inspector or a reader frame. */
function editableBatch(key: string, label: string, ids = ['a', 'b'], frameId?: string) {
  const reader = frameId ? { dataset: { readerFrameId: frameId } } : null;
  const batch = {
    dataset: { attachmentBatchKey: key, attachmentIds: ids.join(','), attachmentBatch: 'batch-id' },
    querySelector: () => input,
  };
  const closest = (selector: string) =>
    selector.includes('ticket-reader') ? reader : selector.includes('batch-key') ? batch : null;
  const input = { value: label, disabled: false, closest } as unknown as HTMLInputElement;
  return { title: { closest } as unknown as Element, input };
}

describe('attachment label autosave contract (HS2-0QQHSZ)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  function harness(stored = '') {
    const editing = signal<AttachmentLabelEditing | undefined>(undefined),
      storage = memoryStorage(),
      saves: Array<{ label: string; ids: readonly string[] | undefined; metadata: unknown }> = [],
      notes: string[] = [];
    let label = stored,
      fail = false;
    const editor = createAttachmentLabelEditor({
      editing,
      storage: () => storage,
      draftKey: (batchKey) => `draft:${batchKey}`,
      currentLabel: () => label,
      describe: (batch) => ({ batch_id: batch.dataset.attachmentBatch }),
      save: (edit, next) => {
        saves.push({ label: next, ids: edit.ids, metadata: edit.metadata });
        if (fail) return Promise.resolve(false);
        label = next;
        return Promise.resolve(true);
      },
      notify: (message) => notes.push(message),
    });
    return {
      editing,
      storage,
      saves,
      notes,
      editor,
      setRemote: (next: string) => {
        label = next;
      },
      setFail: (next: boolean) => {
        fail = next;
      },
      type: (input: HTMLInputElement, value: string) => {
        input.value = value;
        editor.input(input);
      },
    };
  }

  it('coalesces rapid typing into one debounced recovery copy and one write on blur', async () => {
    const h = harness('Before'),
      batch = editableBatch('k1', 'Before');
    expect(h.editor.begin(batch.title)).toBe(batch.input);
    for (const value of ['B', 'Be', 'Bet', 'Better']) h.type(batch.input, value);
    // Keystrokes update the controlled draft at once but never write to the server.
    expect(h.editing.value?.draft).toBe('Better');
    expect(attachmentLabelEditingDraft(h.editing.value, INSPECTOR_ATTACHMENT_LABEL_SCOPE)).toBe('Better');
    expect(attachmentLabelEditingDraft(h.editing.value, 'reader-1')).toBeUndefined();
    expect(h.storage.items.size).toBe(0);
    vi.advanceTimersByTime(149);
    expect(h.storage.items.size).toBe(0);
    vi.advanceTimersByTime(1);
    expect(JSON.parse(h.storage.items.get('draft:k1')!)).toMatchObject({ base: 'Before', draft: 'Better' });
    expect(h.saves).toHaveLength(0);

    // Blur writes once with the batch's ids and captured metadata, clears the copy, and closes.
    await expect(h.editor.finish(batch.input)).resolves.toBe(true);
    expect(h.saves).toEqual([{ label: 'Better', ids: ['a', 'b'], metadata: { batch_id: 'batch-id' } }]);
    expect(h.storage.items.size).toBe(0);
    expect(h.editing.value).toBeUndefined();

    // Post-save editing: a new edit starts from the saved label and writes again.
    const again = editableBatch('k1', 'Better');
    h.editor.begin(again.title);
    expect(h.editing.value).toMatchObject({ original: 'Better', draft: 'Better', base: 'Better' });
    h.type(again.input, 'Best');
    await h.editor.finish(again.input);
    expect(h.saves.map((save) => save.label)).toEqual(['Better', 'Best']);
  });

  it('writes nothing for an unchanged edit, and Escape restores the label and drops the copy', async () => {
    const h = harness('Same'),
      batch = editableBatch('k1', 'Same');
    h.editor.begin(batch.title);
    await h.editor.finish(batch.input);
    expect(h.saves).toHaveLength(0);
    expect(h.editing.value).toBeUndefined();

    h.editor.begin(batch.title);
    h.type(batch.input, 'Typed then abandoned');
    vi.advanceTimersByTime(150);
    expect(h.storage.items.size).toBe(1);
    h.editor.escape(batch.input);
    expect(batch.input.value).toBe('Same');
    expect(h.editing.value?.draft).toBe('Same');
    expect(h.storage.items.size).toBe(0);
    await h.editor.finish(batch.input);
    vi.advanceTimersByTime(500);
    expect(h.saves).toHaveLength(0);
    expect(h.storage.items.size).toBe(0);
  });

  it('flushes on page hide, keeping the editor open, and keeps a failed write open with its copy', async () => {
    const h = harness('Old'),
      batch = editableBatch('k1', 'Old');
    h.editor.begin(batch.title);
    h.type(batch.input, 'Hidden mid-edit');
    await expect(h.editor.flush()).resolves.toBe(true);
    expect(h.saves.map((save) => save.label)).toEqual(['Hidden mid-edit']);
    expect(h.editing.value?.draft).toBe('Hidden mid-edit');

    h.setFail(true);
    h.type(batch.input, 'Will fail');
    await expect(h.editor.finish(batch.input)).resolves.toBe(false);
    // The draft stays visible in the open editor and recoverable from the local copy.
    expect(h.editing.value?.draft).toBe('Will fail');
    expect(JSON.parse(h.storage.items.get('draft:k1')!)).toMatchObject({ draft: 'Will fail' });
    h.setFail(false);
    h.type(batch.input, 'Will fail, then succeed');
    await expect(h.editor.finish(batch.input)).resolves.toBe(true);
    expect(h.saves.at(-1)?.label).toBe('Will fail, then succeed');
    expect(h.editing.value).toBeUndefined();
    expect(h.storage.items.size).toBe(0);
  });

  it('merges against the edit-start label so a concurrent rename is kept', async () => {
    const h = harness('alpha beta gamma'),
      batch = editableBatch('k1', 'alpha beta gamma');
    h.editor.begin(batch.title);
    // The user edits the start; another collaborator edits the end meanwhile.
    h.type(batch.input, 'ALPHA beta gamma');
    h.setRemote('alpha beta GAMMA');
    await h.editor.finish(batch.input);
    expect(h.saves.at(-1)?.label).toBe('ALPHA beta GAMMA');

    // When the remote already holds the draft, or the user changed nothing, no write happens.
    const converged = editableBatch('k2', 'A');
    h.setRemote('A');
    h.editor.begin(converged.title);
    h.type(converged.input, 'B');
    h.setRemote('B');
    await h.editor.finish(converged.input);
    const adopted = editableBatch('k3', 'C');
    h.setRemote('C');
    h.editor.begin(adopted.title);
    h.setRemote('D');
    await h.editor.finish(adopted.input);
    expect(h.saves).toHaveLength(1);
  });

  it('restores an unsaved copy after a reload, rebased onto the current label', async () => {
    const h = harness('Base');
    h.storage.setItem('draft:k1', JSON.stringify({ base: 'Base', draft: 'Base plus typing', at: 1 }));
    const batch = editableBatch('k1', 'Base');
    expect(h.editor.begin(batch.title)).toBe(batch.input);
    expect(batch.input.value).toBe('Base plus typing');
    expect(h.editing.value).toMatchObject({ original: 'Base', draft: 'Base plus typing', base: 'Base' });
    expect(h.notes).toEqual(['Restored an unsaved label edit.']);
    await h.editor.finish(batch.input);
    expect(h.saves.map((save) => save.label)).toEqual(['Base plus typing']);
    expect(h.storage.items.size).toBe(0);

    // A copy whose label changed elsewhere in a disjoint way rebases onto the new label.
    h.storage.setItem('draft:k2', JSON.stringify({ base: 'alpha beta gamma', draft: 'ALPHA beta gamma', at: 1 }));
    const rebased = editableBatch('k2', 'alpha beta GAMMA');
    h.setRemote('alpha beta GAMMA');
    h.editor.begin(rebased.title);
    expect(h.editing.value).toMatchObject({ draft: 'ALPHA beta GAMMA', base: 'alpha beta GAMMA' });

    // A conflicting copy is discarded with a notice; a stale copy equal to the label is dropped quietly.
    h.editing.value = undefined;
    h.storage.setItem('draft:k3', JSON.stringify({ base: 'one', draft: 'two', at: 1 }));
    const conflicted = editableBatch('k3', 'three');
    h.editor.begin(conflicted.title);
    expect(conflicted.input.value).toBe('three');
    expect(h.storage.items.has('draft:k3')).toBe(false);
    expect(h.notes.at(-1)).toMatch(/could not be merged/);
    h.editing.value = undefined;
    h.storage.setItem('draft:k4', JSON.stringify({ base: 'x', draft: 'y', at: 1 }));
    const stale = editableBatch('k4', 'y');
    h.editor.begin(stale.title);
    expect(h.storage.items.has('draft:k4')).toBe(false);
    expect(h.editing.value).toMatchObject({ draft: 'y' });
  });

  it('ignores input, escape, and blur from another surface than the open edit', async () => {
    const h = harness('Inspector label'),
      inspector = editableBatch('k1', 'Inspector label'),
      reader = editableBatch('k1', 'Inspector label', ['a', 'b'], 'reader-1');
    h.editor.begin(reader.title);
    h.type(inspector.input, 'Stray inspector typing');
    h.editor.escape(inspector.input);
    await expect(h.editor.finish(inspector.input)).resolves.toBe(true);
    expect(h.editing.value).toMatchObject({ scope: 'reader-1', draft: 'Inspector label' });
    h.type(reader.input, 'Reader label');
    await h.editor.finish(reader.input);
    expect(h.saves.map((save) => save.label)).toEqual(['Reader label']);
  });
});
