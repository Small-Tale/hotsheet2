import { signal } from 'kerfjs';
import { describe, expect, it } from 'vitest';

import {
  type AttachmentLabelEditing,
  attachmentLabelEditingKey,
  attachmentLabelEditScope,
  beginAttachmentLabelEdit,
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
    expect(editing.value).toEqual({ scope: INSPECTOR_ATTACHMENT_LABEL_SCOPE, key: 'batch-1', original: 'Before' });
    // Repeating the gesture keeps one edit with the same edit-start label.
    inspector.input.value = 'Typed';
    beginAttachmentLabelEdit(inspector.title, editing);
    expect(editing.value?.original).toBe('Typed');
    inspector.input.value = 'Before';

    // The reader opens its own edit; the inspector's blur no longer closes it.
    beginAttachmentLabelEdit(reader.title, editing);
    expect(editing.value).toEqual({ scope: 'workspace-reader', key: '__legacy__', original: '' });
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
});
