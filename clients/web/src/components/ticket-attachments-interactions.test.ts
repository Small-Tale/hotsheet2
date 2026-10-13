import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';

import { type TicketAttachmentActionBindings, wireTicketAttachmentActions } from './ticket-attachments-interactions';

interface Registration {
  event: string;
  selector: string;
  handle: (event: unknown, target: unknown) => unknown;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => ({
  ...(await original<typeof import('kerfjs')>()),
  delegate: (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    registrations.push({ event, selector, handle });
    return () => undefined;
  },
  delegateCapture: (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    registrations.push({ event: `capture:${event}`, selector, handle });
    return () => undefined;
  },
}));
const handler = (event: string, selector: string) =>
  registrations.find((item) => item.event === event && item.selector.includes(selector))!.handle;
const cancelable = (extra: Record<string, unknown> = {}) => ({
  preventDefault: vi.fn(),
  stopPropagation: vi.fn(),
  ...extra,
});

describe('wireTicketAttachmentActions (HS2-1KM6MB)', () => {
  const focused = { focus: vi.fn() };
  const root = { querySelector: () => focused } as unknown as HTMLElement;
  const editorInput = { focus: vi.fn(), select: vi.fn() };
  let finishResult: Promise<boolean> = Promise.resolve(true);
  const labelEditor = {
    begin: vi.fn((target: Element) => ((target as unknown as { open?: boolean }).open ? editorInput : undefined)),
    input: vi.fn(),
    escape: vi.fn(),
    finish: vi.fn(() => finishResult),
    flush: vi.fn(() => Promise.resolve(true)),
  };
  const bindings = {
    addFiles: vi.fn(),
    extraDropTargets: '#rail',
    labelEditor: labelEditor as unknown as TicketAttachmentActionBindings['labelEditor'],
    changePurpose: vi.fn(),
    regroup: vi.fn(),
  };
  wireTicketAttachmentActions(root, bindings);
  beforeAll(() => {
    vi.stubGlobal('Node', function Node() {});
    vi.stubGlobal('CSS', { escape: (value: string) => value });
    vi.stubGlobal('requestAnimationFrame', (callback: () => void) => {
      callback();
      return 0;
    });
  });
  afterAll(() => {
    vi.unstubAllGlobals();
  });

  it('adds picked files once and clears the input, ignoring an empty pick', () => {
    const files = { length: 1 } as FileList,
      input = { files, value: 'a.png' };
    handler('change', 'ticket-attachments')({}, input);
    handler('change', 'ticket-attachments')({}, { files: { length: 0 }, value: 'x' });
    expect(bindings.addFiles.mock.calls).toEqual([[files, input]]);
    expect(input.value).toBe('');
  });

  it('highlights only the attachment drop target and adds dropped files on either target', () => {
    const dropzone = { dataset: {} as Record<string, string>, matches: () => true };
    const rail = { dataset: {} as Record<string, string>, matches: () => false };
    expect(registrations.find((item) => item.event === 'dragover')!.selector).toContain('#rail');
    handler('dragover', 'drop-target="true"')(cancelable(), dropzone);
    handler('dragover', 'drop-target="true"')(cancelable(), rail);
    expect(dropzone.dataset.draggingAttachment).toBe('true');
    expect(rail.dataset.draggingAttachment).toBeUndefined();
    handler('dragleave', 'drop-target="true"')({}, dropzone);
    expect(dropzone.dataset.draggingAttachment).toBeUndefined();
    const files = { length: 2 } as FileList;
    bindings.addFiles.mockClear();
    handler('drop', 'drop-target="true"')(cancelable({ dataTransfer: { files } }), rail);
    handler('drop', 'drop-target="true"')(cancelable({ dataTransfer: { files: { length: 0 } } }), dropzone);
    expect(bindings.addFiles.mock.calls).toEqual([[files, rail]]);
  });

  it('reports purpose changes for a batch with ids only', () => {
    const batch = { dataset: { attachmentIds: 'a,,b' } };
    handler('change', 'attachment-batch-purpose')({}, { closest: () => batch });
    handler('change', 'attachment-batch-purpose')({}, { closest: () => ({ dataset: { attachmentIds: '' } }) });
    expect(bindings.changePurpose.mock.calls).toEqual([[batch, ['a', 'b']]]);
  });

  it('edits a batch label: begin focuses, input drafts, Enter blurs and refocuses after the save settles', async () => {
    handler('dblclick', 'edit-attachment-batch-label')({}, { open: true });
    handler('dblclick', 'edit-attachment-batch-label')({}, { open: false });
    await Promise.resolve();
    expect(editorInput.focus).toHaveBeenCalledTimes(1);
    expect(editorInput.select).toHaveBeenCalledTimes(1);
    const field = { blur: vi.fn(), closest: () => ({ dataset: { attachmentIds: 'a,b' } }) };
    handler('input', 'attachment-batch-label')({}, field);
    expect(labelEditor.input).toHaveBeenCalledWith(field);
    let settle!: (saved: boolean) => void;
    finishResult = new Promise((resolve) => (settle = resolve));
    handler('capture:blur', 'attachment-batch-label')({}, field);
    handler('keydown', 'attachment-batch-label')({ key: 'Enter' }, field);
    expect(field.blur).toHaveBeenCalledTimes(1);
    await Promise.resolve();
    expect(focused.focus).not.toHaveBeenCalled();
    settle(true);
    await finishResult;
    await Promise.resolve();
    expect(focused.focus).toHaveBeenCalledTimes(1);
    handler('keydown', 'attachment-batch-label')({ key: 'Escape' }, field);
    handler('keydown', 'attachment-batch-label')({ key: 'a' }, field);
    expect(labelEditor.escape).toHaveBeenCalledWith(field);
    expect(field.blur).toHaveBeenCalledTimes(2);
  });

  it('regroups a dragged attachment into an existing or new group and ignores drops without a drag', () => {
    const surface = {
      dataset: {} as Record<string, string>,
      querySelector: () => ({ closest: () => source }),
      querySelectorAll: () => [overTarget],
    };
    const source = { id: 'source' },
      destination = { id: 'destination' },
      overTarget = { dataset: { dragOver: 'true' } as Record<string, string> };
    const groupTarget = (newGroup: boolean, inside = destination) => ({
      dataset: {} as Record<string, string>,
      matches: () => newGroup,
      contains: () => false,
      closest: (selector: string) => (selector.includes('ticket-attachments') ? surface : inside),
    });
    const drop = handler('drop', 'attachment-group-drop-target');
    const over = handler('dragover', 'attachment-group-drop-target');
    // No drag in progress: dragover and drop are ignored.
    const idle = cancelable();
    over(idle, groupTarget(false));
    drop(idle, groupTarget(false));
    expect(idle.preventDefault).not.toHaveBeenCalled();
    const start = (id: string) =>
      handler('dragstart', 'drag-attachment-id')(
        { dataTransfer: { setData: vi.fn(), effectAllowed: '' } },
        { dataset: { dragAttachmentId: id }, closest: () => surface },
      );
    start('one');
    expect(surface.dataset.draggingGroupAttachment).toBe('true');
    const target = groupTarget(false);
    over(cancelable({ dataTransfer: { dropEffect: '' } }), target);
    expect(target.dataset.dragOver).toBe('true');
    handler('dragleave', 'attachment-group-drop-target')({ relatedTarget: null }, target);
    expect(target.dataset.dragOver).toBeUndefined();
    drop(cancelable(), target);
    expect(surface.dataset.draggingGroupAttachment).toBeUndefined();
    expect(overTarget.dataset.dragOver).toBeUndefined();
    start('two');
    drop(cancelable(), groupTarget(true));
    start('three');
    drop(cancelable(), {
      ...groupTarget(false),
      closest: (selector: string) => (selector.includes('ticket') ? surface : null),
    });
    start('four');
    handler('dragend', 'drag-attachment-id')({}, { closest: () => surface });
    drop(cancelable(), groupTarget(false));
    expect(bindings.regroup.mock.calls).toEqual([
      ['one', source, destination],
      ['two', source, undefined],
    ]);
  });
});
