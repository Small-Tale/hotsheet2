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
const routes = vi.hoisted(
  () => [] as { id: string; handle: (files: File[], selection: { picker: HTMLElement }) => void }[],
);
vi.mock('../file-picker-routes', () => ({
  routeFilePicker: (_root: unknown, id: string, handle: (typeof routes)[number]['handle']) => {
    routes.push({ id, handle });
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

  it('adds files the Kerf Add trigger and drop zone report through the shared route (HS2-VHY325)', () => {
    expect(routes.map((route) => route.id)).toEqual(['ticket-attachments']);
    expect(registrations.some((item) => item.event === 'change' && item.selector.includes('ticket-attachments'))).toBe(
      false,
    );
    const files = [{ name: 'a.png' }] as unknown as File[],
      picker = { id: 'zone' } as unknown as HTMLElement;
    routes[0].handle(files, { picker });
    expect(bindings.addFiles.mock.calls).toEqual([[files, picker]]);
  });

  it('leaves surface and zone drops to Kerf while accepting reader and rail drops outside the surface once', () => {
    const files = { length: 2 } as FileList,
      rail = {} as HTMLElement,
      reader = {} as HTMLElement,
      outside = { closest: () => null },
      inside = { closest: () => ({ id: 'surface' }) };
    expect(registrations.filter((item) => item.event === 'dragleave' && item.selector === '#rail')).toHaveLength(0);
    expect(registrations.find((item) => item.event === 'dragover' && item.selector.includes('#rail'))).toBeDefined();
    const railHover = cancelable({ target: outside });
    handler('dragover', '#rail')(railHover, rail);
    expect(railHover.preventDefault).toHaveBeenCalledOnce();
    const surfaceHover = cancelable({ target: inside });
    handler('dragover', '#rail')(surfaceHover, rail);
    expect(surfaceHover.preventDefault).not.toHaveBeenCalled();

    bindings.addFiles.mockClear();
    const surfaceDrop = cancelable({ target: inside, dataTransfer: { files } });
    handler('drop', '#rail')(surfaceDrop, rail);
    expect(surfaceDrop.preventDefault).not.toHaveBeenCalled();
    expect(bindings.addFiles).not.toHaveBeenCalled();
    const railDrop = cancelable({ target: outside, dataTransfer: { files } });
    handler('drop', '#rail')(railDrop, rail);
    expect(railDrop.preventDefault).toHaveBeenCalledOnce();
    expect(bindings.addFiles.mock.calls).toEqual([[files, rail]]);
    const readerDrop = cancelable({ target: outside, dataTransfer: { files } });
    handler('drop', 'data-attachment-drop-target')(readerDrop, reader);
    expect(readerDrop.preventDefault).toHaveBeenCalledOnce();
    expect(bindings.addFiles.mock.calls).toEqual([
      [files, rail],
      [files, reader],
    ]);
    handler('drop', '#rail')(cancelable({ target: outside, dataTransfer: { files: { length: 0 } } }), rail);
    expect(bindings.addFiles).toHaveBeenCalledTimes(2);
  });

  it('reports purpose changes for a batch with ids only', () => {
    const batch = { dataset: { attachmentIds: 'a,,b' } };
    handler('change', 'attachment-batch-purpose')({}, { closest: () => batch });
    handler('change', 'attachment-batch-purpose')({}, { closest: () => ({ dataset: { attachmentIds: '' } }) });
    expect(bindings.changePurpose.mock.calls).toEqual([[batch, ['a', 'b']]]);
  });

  it('edits a batch label: begin focuses, input drafts, Enter blurs and refocuses after the save settles', async () => {
    // The heading's Edit action and a double-click on the heading both open the editor (HS2-K9HSXZ).
    handler('click', 'edit-attachment-batch-label')({}, { open: true });
    handler('click', 'edit-attachment-batch-label')({}, { open: false });
    handler('dblclick', 'data-attachment-batch-title')({}, { open: true });
    handler('dblclick', 'data-attachment-batch-title')({}, { open: false });
    await Promise.resolve();
    expect(editorInput.focus).toHaveBeenCalledTimes(2);
    expect(editorInput.select).toHaveBeenCalledTimes(2);
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

  it('blurs a Web Awesome editor on Escape only after it renders the restored label (HS2-K9HSXZ)', async () => {
    let rendered!: () => void;
    const field = {
      blur: vi.fn(),
      closest: () => ({ dataset: { attachmentIds: 'a' } }),
      updateComplete: new Promise<void>((resolve) => (rendered = resolve)),
    };
    labelEditor.escape.mockClear();
    handler('keydown', 'attachment-batch-label')({ key: 'Escape' }, field);
    expect(labelEditor.escape).toHaveBeenCalledWith(field);
    expect(field.blur).not.toHaveBeenCalled();
    rendered();
    await field.updateComplete;
    expect(field.blur).toHaveBeenCalledTimes(1);
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
