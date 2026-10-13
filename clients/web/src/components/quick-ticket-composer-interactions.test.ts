import { describe, expect, it, vi } from 'vitest';

import { NEW_TICKET_ATTACHMENT_PICKER } from './quick-ticket-composer';
import { wireQuickTicketComposer } from './quick-ticket-composer-interactions';
import { wireTicketRowUpNextToggle } from './ticket-collection-interactions';

interface Registration {
  event: string;
  selector: string;
  handle: (event: unknown, target: unknown) => unknown;
}
const registrations = vi.hoisted(() => [] as Registration[]);
const pickers = vi.hoisted(() => [] as ((change: { id: string; files: File[] }) => void)[]);
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
vi.mock('@kerfjs/ui/wire-file-pickers', () => ({
  wireFilePickers: (_root: unknown, options: { onFiles: (change: { id: string; files: File[] }) => void }) => {
    pickers.push(options.onFiles);
    return () => undefined;
  },
}));
const handler = (event: string, selector: string) =>
  registrations.find((item) => item.event === event && item.selector.includes(selector))!.handle;
const cancelable = (extra: Record<string, unknown> = {}) => ({
  preventDefault: vi.fn(),
  stopPropagation: vi.fn(),
  target: { closest: () => null },
  ...extra,
});

describe('wireQuickTicketComposer (HS2-NRW5ZR)', () => {
  let expanded = false,
    submitting = false,
    ticketDrag = false;
  const bindings = {
    expand: vi.fn(() => {
      expanded = true;
    }),
    expanded: () => expanded,
    canHide: () => !submitting,
    dismissed: vi.fn(),
    setTitle: vi.fn(),
    setDetails: vi.fn(),
    setCategory: vi.fn(),
    setSource: vi.fn(),
    toggleUpNext: vi.fn(),
    addFiles: vi.fn(),
    removeAttachment: vi.fn(),
    clearAttachments: vi.fn(),
    draggingTickets: () => ticketDrag,
    submit: vi.fn(),
  };
  wireQuickTicketComposer({} as HTMLElement, bindings);

  it('routes the draft fields, Up Next, attachment removal, and submit', () => {
    handler('input', 'new-ticket-title')({}, { value: 'Title' });
    handler('input', 'new-ticket-details')({}, { value: 'Body' });
    handler('change', 'new-ticket-category')({}, { value: 'bug' });
    handler('change', 'new-ticket-source')({}, { value: '' });
    handler('change', 'new-ticket-source')({}, { value: 'git' });
    handler('click', 'toggle-new-ticket-up-next')({}, {});
    handler('click', 'remove-new-ticket-attachment')({}, { dataset: { pendingAttachmentId: 'a' } });
    handler('click', 'clear-new-ticket-attachments')({}, {});
    const submit = cancelable();
    handler('submit', 'create-ticket-form')(submit, {});
    expect(bindings.setTitle).toHaveBeenCalledWith('Title');
    expect(bindings.setDetails).toHaveBeenCalledWith('Body');
    expect(bindings.setCategory).toHaveBeenCalledWith('bug');
    expect(bindings.setSource.mock.calls).toEqual([[undefined], ['git']]);
    expect(bindings.toggleUpNext).toHaveBeenCalledTimes(1);
    expect(bindings.removeAttachment).toHaveBeenCalledWith('a');
    expect(bindings.clearAttachments).toHaveBeenCalledTimes(1);
    expect(submit.preventDefault).toHaveBeenCalled();
    expect(bindings.submit).toHaveBeenCalledTimes(1);
  });

  it('refuses to hide while submitting and resets only an expanded composer after hiding', () => {
    const popup = {};
    const hide = cancelable({ target: popup });
    submitting = true;
    handler('capture:wa-hide', 'quick-ticket-composer')(hide, popup);
    submitting = false;
    handler('capture:wa-hide', 'quick-ticket-composer')(cancelable({ target: popup }), popup);
    handler('capture:wa-hide', 'quick-ticket-composer')(cancelable({ target: {} }), popup);
    expect(hide.preventDefault).toHaveBeenCalledTimes(1);
    const afterHide = handler('capture:wa-after-hide', 'quick-ticket-composer');
    afterHide({ target: popup }, popup);
    expanded = true;
    afterHide({ target: {} }, popup);
    afterHide({ target: popup }, popup);
    expect(bindings.dismissed).toHaveBeenCalledTimes(1);
  });

  it('stages files the Kerf trigger and drop zone report, ignoring other pickers and empty picks', () => {
    bindings.addFiles.mockClear();
    const picked = [{ name: 'b' } as File];
    pickers[0]({ id: NEW_TICKET_ATTACHMENT_PICKER, files: picked });
    pickers[0]({ id: 'other', files: picked });
    pickers[0]({ id: NEW_TICKET_ATTACHMENT_PICKER, files: [] });
    expect(bindings.addFiles.mock.calls).toEqual([[picked]]);
  });

  it('drops files onto a collapsed composer by expanding it first, and ignores ticket drags', () => {
    bindings.addFiles.mockClear();
    bindings.expand.mockClear();
    expanded = false;
    const target = { dataset: {} as Record<string, string> };
    ticketDrag = true;
    const ignored = cancelable();
    handler('dragover', 'new-ticket-drop-target')(ignored, target);
    handler('drop', 'new-ticket-drop-target')(ignored, target);
    expect(ignored.preventDefault).not.toHaveBeenCalled();
    ticketDrag = false;
    handler('dragover', 'new-ticket-drop-target')(cancelable(), target);
    expect(target.dataset.dragging).toBe('true');
    handler('dragleave', 'new-ticket-drop-target')({}, target);
    expect(target.dataset.dragging).toBeUndefined();
    handler('drop', 'new-ticket-drop-target')(cancelable({ dataTransfer: { files: { length: 0 } } }), target);
    const files = { length: 1 } as FileList;
    handler('drop', 'new-ticket-drop-target')(cancelable({ dataTransfer: { files } }), target);
    handler('drop', 'new-ticket-drop-target')(cancelable({ dataTransfer: { files } }), target);
    expect(bindings.expand).toHaveBeenCalledTimes(1);
    expect(bindings.addFiles.mock.calls).toEqual([[files], [files]]);
    // A drop on the Kerf FileDropZone is staged by its own wiring, never twice (HS2-RB7KXZ).
    handler('drop', 'new-ticket-drop-target')(
      cancelable({ dataTransfer: { files }, target: { closest: () => ({}) } }),
      target,
    );
    expect(bindings.addFiles).toHaveBeenCalledTimes(2);
    handler('click', 'expand-ticket-composer')({}, target);
    expect(bindings.expand).toHaveBeenCalledTimes(2);
  });
});

describe('wireTicketRowUpNextToggle (HS2-NRW5ZR)', () => {
  it('toggles the star row from a click or Enter/Space before the row sees the event', () => {
    const toggle = vi.fn();
    wireTicketRowUpNextToggle({} as HTMLElement, toggle);
    const row = { dataset: { ticketSlug: 'HS2-1' } },
      star = { closest: () => row },
      orphan = { closest: () => null };
    const click = cancelable();
    handler('capture:click', 'toggle-row-up-next')(click, star);
    handler('capture:click', 'toggle-row-up-next')(cancelable(), orphan);
    const keydown = handler('capture:keydown', 'toggle-row-up-next');
    const enter = cancelable({ key: 'Enter' });
    keydown(enter, star);
    keydown(cancelable({ key: ' ' }), star);
    const tab = cancelable({ key: 'Tab' });
    keydown(tab, star);
    expect(click.stopPropagation).toHaveBeenCalled();
    expect(enter.preventDefault).toHaveBeenCalled();
    expect(tab.preventDefault).not.toHaveBeenCalled();
    expect(toggle.mock.calls).toEqual([[row], [row], [row]]);
  });
});
