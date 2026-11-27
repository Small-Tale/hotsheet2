import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  type AttachmentLabelConflict,
  attachmentLabelConflictFor,
  type AttachmentLabelEditing,
  attachmentLabelEditingDraft,
  attachmentLabelEditingKey,
  attachmentLabelEditScope,
  beginAttachmentLabelEdit,
  createAttachmentLabelEditor,
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

  it('walks begin, replace, and restore across surfaces', () => {
    const editing = signal<AttachmentLabelEditing | undefined>(undefined),
      inspector = batchIn(undefined, 'batch-1', 'Before'),
      reader = batchIn('workspace-reader', '__legacy__', '');
    // Restoring with nothing open is a no-op.
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

    // The reader opens its own edit; restoring through the inspector leaves it alone.
    beginAttachmentLabelEdit(reader.title, editing);
    expect(editing.value).toMatchObject({ scope: 'workspace-reader', key: '__legacy__', original: '' });
    restoreAttachmentLabelEdit(inspector.input, editing);
    expect(editing.value?.scope).toBe('workspace-reader');
    expect(inspector.input.value).toBe('Before');

    // Escape restores the edit-start label.
    reader.input.value = 'Discard me';
    restoreAttachmentLabelEdit(reader.input, editing);
    expect(reader.input.value).toBe('');
    expect(editing.value?.draft).toBe('');
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

  it('renders the label conflict card on its own batch only, never for a read-only provider (HS2-9DQXGY)', () => {
    const attachments = [
        { id: 'a', name: 'a.png', batch_id: 'one', batch_label: 'One' },
        { id: 'b', name: 'b.png', batch_id: 'two', batch_label: 'Two' },
      ],
      labelConflict = attachmentLabelConflictFor(
        {
          scope: INSPECTOR_ATTACHMENT_LABEL_SCOPE,
          key: 'two',
          draftKey: 'draft:two',
          base: 'Base',
          mine: 'Mine',
          theirs: 'Two',
          resolution: 'Mine and Two',
        },
        INSPECTOR_ATTACHMENT_LABEL_SCOPE,
        undefined,
      ),
      markup = String(TicketAttachments({ attachments, labelConflict }));
    expect(markup.match(/data-component="ticket-field-conflict"/g)).toHaveLength(1);
    expect(markup.indexOf('data-component="ticket-field-conflict"')).toBeGreaterThan(
      markup.indexOf('data-attachment-batch-key="two"'),
    );
    expect(markup).toContain('Mine and Two');
    expect(markup).toContain('data-action="apply-attachment-label-merge"');
    expect(String(TicketAttachments({ attachments, editable: false, labelConflict }))).not.toContain(
      'ticket-field-conflict',
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

  it('HS2-E0H7S0 old label completion clears its captured destination rather than a replacement owner', async () => {
    const editing = signal<AttachmentLabelEditing | undefined>(undefined),
      storage = memoryStorage();
    let owner = 'project-a:git:1',
      release!: (saved: boolean) => void;
    const editor = createAttachmentLabelEditor({
        editing,
        storage: () => storage,
        draftKey: (batch) => `${owner}:${batch}`,
        currentLabel: () => 'base',
        describe: () => ({}),
        save: () =>
          new Promise((resolve) => {
            release = resolve;
          }),
      }),
      batch = editableBatch('same-batch', 'base');
    editor.begin(batch.title);
    batch.input.value = 'submitted';
    editor.input(batch.input);
    const pending = editor.flush();
    owner = 'project-b:git:1';
    storage.setItem(`${owner}:same-batch`, JSON.stringify({ base: 'different base', draft: 'submitted' }));
    release(true);
    await pending;
    expect(storage.getItem('project-a:git:1:same-batch')).toBeNull();
    expect(JSON.parse(storage.getItem(`${owner}:same-batch`) ?? '{}')).toMatchObject({ draft: 'submitted' });
  });

  function harness(stored = '') {
    const editing = signal<AttachmentLabelEditing | undefined>(undefined),
      conflict = signal<AttachmentLabelConflict | undefined>(undefined),
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
      conflict,
      ticketKey: (scope) => `ticket-of-${scope}`,
    });
    return {
      editing,
      conflict,
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

  it('HS2-E0H7S0 captures a latest label synchronously and exposes quota until retry', () => {
    const h = harness('Before'),
      batch = editableBatch('owned', 'Before');
    h.editor.begin(batch.title);
    h.type(batch.input, 'Latest label');
    const original = h.storage.setItem,
      failure = vi.spyOn(h.storage, 'setItem').mockImplementation(() => {
        throw new Error('Synthetic quota');
      });
    expect(() => {
      h.editor.captureRecovery();
    }).toThrow('Synthetic quota');
    expect(h.editing.value?.draft).toBe('Latest label');
    expect(h.saves).toEqual([]);
    failure.mockImplementation(original);
    h.editor.captureRecovery();
    expect(JSON.parse(h.storage.getItem('draft:owned') ?? '{}')).toMatchObject({
      base: 'Before',
      draft: 'Latest label',
    });
    h.editor.escape(batch.input);
    h.editor.captureRecovery();
    expect(h.storage.getItem('draft:owned')).toBeNull();
    h.type(batch.input, 'After reset');
    h.editor.captureRecovery();
    expect(JSON.parse(h.storage.getItem('draft:owned') ?? '{}')).toMatchObject({ draft: 'After reset' });
    h.editor.escape(batch.input);
  });

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
    expect(h.storage.items.size).toBe(0);
    // The combined save is announced once it commits, as ticket fields do (HS2-ESHNH3, HS2-JJ3R1G).
    expect(h.notes).toEqual(['Merged your edit with a newer change to Attachment Label.']);

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

    // A conflicting copy is kept for the conflict card (HS2-9DQXGY); a stale copy equal to the label is
    // dropped quietly.
    h.editing.value = undefined;
    h.storage.setItem('draft:k3', JSON.stringify({ base: 'one', draft: 'two', at: 1 }));
    const conflicted = editableBatch('k3', 'three');
    expect(h.editor.begin(conflicted.title)).toBeUndefined();
    expect(conflicted.input.value).toBe('three');
    expect(h.storage.items.has('draft:k3')).toBe(true);
    expect(h.conflict.value).toMatchObject({ key: 'k3', base: 'one', mine: 'two', theirs: 'three' });
    expect(h.notes.join('\n')).not.toMatch(/discarded/);
    h.editor.acceptTheirs();
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

describe('attachment label conflicts keep the recovery copy (HS2-9DQXGY)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  function setup(remote: string) {
    const editing = signal<AttachmentLabelEditing | undefined>(undefined),
      conflict = signal<AttachmentLabelConflict | undefined>(undefined),
      storage = memoryStorage(),
      saves: string[] = [],
      notes: string[] = [];
    let label = remote,
      fail = false;
    const editor = createAttachmentLabelEditor({
      editing,
      conflict,
      storage: () => storage,
      draftKey: (batchKey) => `draft:${batchKey}`,
      currentLabel: () => label,
      describe: (batch) => ({ batch_id: batch.dataset.attachmentBatch }),
      ticketKey: (scope) => `ticket-of-${scope}`,
      save: (_edit, next) => {
        saves.push(next);
        if (fail) return Promise.resolve(false);
        label = next;
        return Promise.resolve(true);
      },
      notify: (message) => notes.push(message),
    });
    return {
      editing,
      conflict,
      storage,
      saves,
      notes,
      editor,
      setFail: (next: boolean) => {
        fail = next;
      },
      setRemote: (next: string) => {
        label = next;
      },
      field: (value: string) => ({ value }) as unknown as HTMLTextAreaElement,
    };
  }

  it('merges a disjoint copy on reopen and says so with the merge toast', () => {
    const h = setup('alpha beta GAMMA');
    h.storage.setItem('draft:k1', JSON.stringify({ base: 'alpha beta gamma', draft: 'ALPHA beta gamma', at: 1 }));
    const batch = editableBatch('k1', 'alpha beta GAMMA');
    expect(h.editor.begin(batch.title)).toBe(batch.input);
    expect(batch.input.value).toBe('ALPHA beta GAMMA');
    expect(h.notes).toEqual(['Restored an unsaved edit and merged it with a newer change to Attachment Label.']);
    expect(h.conflict.value).toBeUndefined();
  });

  it('clears a copy whose merge the server already holds instead of restoring a no-op', () => {
    const h = setup('ALPHA beta GAMMA');
    h.storage.setItem('draft:k1', JSON.stringify({ base: 'alpha beta GAMMA', draft: 'ALPHA beta GAMMA', at: 1 }));
    const batch = editableBatch('k1', 'ALPHA beta GAMMA');
    h.editor.begin(batch.title);
    expect(h.storage.items.size).toBe(0);
    expect(h.notes).toEqual([]);
  });

  it('keeps an overlapping copy, offers both versions, and applies the merged value once', async () => {
    const h = setup('Repro on WebKit');
    h.storage.setItem('draft:k1', JSON.stringify({ base: 'Repro', draft: 'Repro on Safari 17', at: 1 }));
    const batch = editableBatch('k1', 'Repro on WebKit', ['a', 'b'], 'reader-1');
    expect(h.editor.begin(batch.title)).toBeUndefined();
    expect(h.editing.value).toBeUndefined();
    expect(h.storage.items.has('draft:k1')).toBe(true);
    expect(h.conflict.value).toMatchObject({
      scope: 'reader-1',
      ticketKey: 'ticket-of-reader-1',
      key: 'k1',
      draftKey: 'draft:k1',
      ids: ['a', 'b'],
      metadata: { batch_id: 'batch-id' },
      mine: 'Repro on Safari 17',
      theirs: 'Repro on WebKit',
      resolution: 'Repro on Safari 17',
    });
    // The card is scoped to its surface and ticket.
    expect(attachmentLabelConflictFor(h.conflict.value, 'reader-1', 'ticket-of-reader-1')).toMatchObject({
      key: 'k1',
      resolution: 'Repro on Safari 17',
      conflict: { key: 'attachment_label:k1', field: 'attachment_label', label: 'Attachment Label' },
    });
    expect(attachmentLabelConflictFor(h.conflict.value, INSPECTOR_ATTACHMENT_LABEL_SCOPE, 'ticket-of-reader-1')).toBe(
      undefined,
    );
    expect(attachmentLabelConflictFor(h.conflict.value, 'reader-1', 'another-ticket')).toBeUndefined();

    // Typing the merged value is coalesced: no write, and state catches up after the pause.
    h.editor.conflictInput(h.field('Repro on Safari'));
    h.editor.conflictInput(h.field('Repro on Safari 17 and WebKit'));
    expect(h.conflict.value?.resolution).toBe('Repro on Safari 17');
    // Reopening while the card shows keeps the merged value being shaped.
    h.editor.flushConflictInput();
    expect(h.editor.begin(batch.title)).toBeUndefined();
    expect(h.conflict.value?.resolution).toBe('Repro on Safari 17 and WebKit');
    expect(h.saves).toEqual([]);

    // A failed write keeps the card and the copy; a retry commits once and settles both.
    h.setFail(true);
    await expect(h.editor.applyMerge()).resolves.toBe(false);
    expect(h.conflict.value).toBeDefined();
    expect(h.storage.items.has('draft:k1')).toBe(true);
    h.setFail(false);
    await expect(h.editor.applyMerge()).resolves.toBe(true);
    expect(h.saves).toEqual(['Repro on Safari 17 and WebKit', 'Repro on Safari 17 and WebKit']);
    expect(h.conflict.value).toBeUndefined();
    expect(h.storage.items.size).toBe(0);
    // Nothing waits any more: reopening is clean and a second apply is a no-op.
    await expect(h.editor.applyMerge()).resolves.toBe(false);
    expect(h.editor.begin(batch.title)).toBe(batch.input);
  });

  it('Use Theirs drops the copy without a write; applying their label unchanged writes nothing', async () => {
    const h = setup('three');
    h.storage.setItem('draft:k1', JSON.stringify({ base: 'one', draft: 'two', at: 1 }));
    h.editor.begin(editableBatch('k1', 'three').title);
    h.editor.conflictInput(h.field('pending'));
    h.editor.acceptTheirs();
    expect(h.conflict.value).toBeUndefined();
    expect(h.storage.items.size).toBe(0);
    vi.runAllTimers();
    expect(h.conflict.value).toBeUndefined();

    h.storage.setItem('draft:k2', JSON.stringify({ base: 'one', draft: 'two', at: 1 }));
    h.editor.begin(editableBatch('k2', 'three').title);
    h.editor.conflictInput(h.field(' three '));
    await expect(h.editor.applyMerge()).resolves.toBe(true);
    expect(h.saves).toEqual([]);
    expect(h.storage.items.size).toBe(0);
    // Input or Use Theirs with nothing waiting is a no-op.
    h.editor.conflictInput(h.field('stray'));
    h.editor.acceptTheirs();
    expect(h.conflict.value).toBeUndefined();
  });

  it('offers a copy without a merge base instead of ignoring it, and drops one equal to the label', () => {
    const h = setup('Current');
    h.storage.setItem('draft:k1', JSON.stringify({ draft: 'Legacy text', at: 1 }));
    h.editor.begin(editableBatch('k1', 'Current').title);
    expect(h.conflict.value).toMatchObject({ base: '', mine: 'Legacy text', theirs: 'Current' });
    h.editor.acceptTheirs();
    h.storage.setItem('draft:k2', JSON.stringify({ draft: 'Current', at: 1 }));
    const same = editableBatch('k2', 'Current');
    expect(h.editor.begin(same.title)).toBe(same.input);
    expect(h.storage.items.size).toBe(0);
    expect(h.conflict.value).toBeUndefined();
  });

  it('a newer reopen that no longer conflicts settles the shown card', () => {
    const h = setup('three');
    h.storage.setItem('draft:k1', JSON.stringify({ base: 'one', draft: 'two', at: 1 }));
    h.editor.begin(editableBatch('k1', 'three').title);
    expect(h.conflict.value).toBeDefined();
    // The label changed elsewhere to the copy's text: nothing is left to choose.
    const converged = editableBatch('k1', 'two');
    h.editor.begin(converged.title);
    expect(h.conflict.value).toBeUndefined();
    expect(h.storage.items.size).toBe(0);
  });
  it('a finished edit that overlaps a newer label keeps the draft in the card and writes nothing (HS2-ESHNH3)', async () => {
    const h = setup('Repro');
    const batch = editableBatch('k1', 'Repro', ['a', 'b'], 'reader-1');
    expect(h.editor.begin(batch.title)).toBe(batch.input);
    batch.input.value = 'Repro on Safari 17';
    h.editor.input(batch.input);
    // Another collaborator rewrote the same words while the editor was open.
    h.setRemote('Repro on WebKit');
    await expect(h.editor.finish(batch.input)).resolves.toBe(true);
    expect(h.saves).toEqual([]);
    expect(h.notes).toEqual([]);
    expect(h.editing.value).toBeUndefined();
    expect(JSON.parse(h.storage.getItem('draft:k1') ?? '{}')).toMatchObject({
      base: 'Repro',
      draft: 'Repro on Safari 17',
    });
    expect(h.conflict.value).toMatchObject({
      scope: 'reader-1',
      ticketKey: 'ticket-of-reader-1',
      key: 'k1',
      draftKey: 'draft:k1',
      ids: ['a', 'b'],
      metadata: { batch_id: 'batch-id' },
      base: 'Repro',
      mine: 'Repro on Safari 17',
      theirs: 'Repro on WebKit',
      resolution: 'Repro on Safari 17',
    });
    // The debounce timer fires later without writing either.
    vi.runAllTimers();
    expect(h.saves).toEqual([]);
    // Reopening the rerendered batch finds the same kept copy and keeps the card instead of the editor.
    expect(h.editor.begin(editableBatch('k1', 'Repro on WebKit', ['a', 'b'], 'reader-1').title)).toBeUndefined();
    expect(h.conflict.value?.mine).toBe('Repro on Safari 17');
    // Apply Merged Value writes once and drops the copy; editing afterwards starts from the result.
    h.editor.conflictInput(h.field('Repro on Safari 17 and WebKit'));
    await expect(h.editor.applyMerge()).resolves.toBe(true);
    expect(h.saves).toEqual(['Repro on Safari 17 and WebKit']);
    expect(h.storage.items.size).toBe(0);
    expect(h.conflict.value).toBeUndefined();
    const again = editableBatch('k1', 'Repro on Safari 17 and WebKit', ['a', 'b'], 'reader-1');
    expect(h.editor.begin(again.title)).toBe(again.input);
    again.input.value = 'Repro everywhere';
    h.editor.input(again.input);
    await h.editor.finish(again.input);
    expect(h.saves.at(-1)).toBe('Repro everywhere');
  });

  it('an overlap found by page hide keeps the copy, and Use Theirs then drops it without a write', async () => {
    const h = setup('one');
    const batch = editableBatch('k1', 'one');
    h.editor.begin(batch.title);
    batch.input.value = 'two';
    h.editor.input(batch.input);
    h.setRemote('three');
    await expect(h.editor.flush()).resolves.toBe(true);
    expect(h.saves).toEqual([]);
    expect(h.conflict.value).toMatchObject({ base: 'one', mine: 'two', theirs: 'three' });
    expect(h.storage.items.has('draft:k1')).toBe(true);
    h.editor.acceptTheirs();
    expect(h.conflict.value).toBeUndefined();
    expect(h.storage.items.size).toBe(0);
    expect(h.saves).toEqual([]);
  });

  it('an editor left open after a page-hide save continues from the saved label (HS2-ESHNH3)', async () => {
    const h = setup('one two three four five');
    const batch = editableBatch('k1', 'one two three four five');
    h.editor.begin(batch.title);
    batch.input.value = 'ONE two three four five';
    h.editor.input(batch.input);
    h.setRemote('one two three four FIVE');
    await expect(h.editor.flush()).resolves.toBe(true);
    expect(h.saves).toEqual(['ONE two three four FIVE']);
    // The unchanged draft adopts the merged label, so blurring afterwards writes nothing.
    expect(h.editing.value).toMatchObject({ draft: 'ONE two three four FIVE', base: 'ONE two three four FIVE' });
    await h.editor.finish(batch.input);
    expect(h.saves).toHaveLength(1);

    // Text typed while a merged write is in flight merges against the sent draft, so the newer
    // label's change is never reverted and nothing reads as a conflict.
    const next = editableBatch('k2', 'one two three four five');
    h.setRemote('one two three four five');
    h.editor.begin(next.title);
    next.input.value = 'ONE two three four five';
    h.editor.input(next.input);
    h.setRemote('one two three four FIVE');
    const pending = h.editor.flush();
    next.input.value = 'ONE two THREE four five';
    h.editor.input(next.input);
    await expect(pending).resolves.toBe(true);
    expect(h.editing.value?.base).toBe('ONE two three four five');
    await expect(h.editor.finish(next.input)).resolves.toBe(true);
    expect(h.saves.slice(1)).toEqual(['ONE two three four FIVE', 'ONE two THREE four FIVE']);
    expect(h.conflict.value).toBeUndefined();
  });

  it('a save-time merge is announced only after its write commits (HS2-ESHNH3)', async () => {
    const h = setup('alpha beta gamma');
    const batch = editableBatch('k1', 'alpha beta gamma');
    h.editor.begin(batch.title);
    batch.input.value = 'ALPHA beta gamma';
    h.editor.input(batch.input);
    h.setRemote('alpha beta GAMMA');
    h.setFail(true);
    await expect(h.editor.finish(batch.input)).resolves.toBe(false);
    expect(h.saves).toEqual(['ALPHA beta GAMMA']);
    expect(h.notes).toEqual([]);
    expect(h.conflict.value).toBeUndefined();
    h.setFail(false);
    h.editor.input(batch.input);
    await expect(h.editor.finish(batch.input)).resolves.toBe(true);
    expect(h.saves).toEqual(['ALPHA beta GAMMA', 'ALPHA beta GAMMA']);
    expect(h.notes).toEqual(['Merged your edit with a newer change to Attachment Label.']);
    // A plain save with no newer label says nothing beyond the save's own acknowledgement.
    const plain = editableBatch('k2', 'x');
    h.setRemote('x');
    h.editor.begin(plain.title);
    plain.input.value = 'y';
    h.editor.input(plain.input);
    await h.editor.finish(plain.input);
    expect(h.notes).toHaveLength(1);
  });
});
