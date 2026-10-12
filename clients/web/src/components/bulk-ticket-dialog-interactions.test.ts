import { describe, expect, it, vi } from 'vitest';

import { wireBulkTicketDialog } from './bulk-ticket-dialog-interactions';

interface Registration {
  event: string;
  selector: string;
  handle: (event: Event, target: Element) => unknown;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => ({
  ...(await original<typeof import('kerfjs')>()),
  delegate: (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    registrations.push({ event, selector, handle });
    return () => undefined;
  },
}));
const handler = (event: string, selector: string) =>
  registrations.find((item) => item.event === event && item.selector.startsWith(selector))!.handle;

describe('wireBulkTicketDialog (HS2-RWY78R)', () => {
  const bindings = {
    submitTag: vi.fn(),
    confirmDelete: vi.fn(),
    confirmEmptyTrash: vi.fn(),
    dismiss: vi.fn(),
  };
  const input = { value: '', focus: vi.fn() };
  wireBulkTicketDialog({ querySelector: () => input } as unknown as HTMLElement, bindings);

  it('submits the tag with its mode and ignores a form without one', () => {
    const form = (tagMode?: string) =>
      ({ dataset: { tagMode }, querySelector: () => ({ value: 'client' }) }) as unknown as Element;
    const submit = handler('submit', '[data-action="submit-bulk-tag"]');
    submit({ preventDefault: vi.fn() } as unknown as Event, form('remove'));
    submit({ preventDefault: vi.fn() } as unknown as Event, form());
    expect(bindings.submitTag.mock.calls).toEqual([['remove', 'client']]);
  });

  it('fills the tag field from a suggestion chip', () => {
    handler('click', '[data-action="choose-bulk-tag"]')(new Event('click'), {
      dataset: { tag: 'ux' },
    } as unknown as Element);
    expect(input.value).toBe('ux');
    expect(input.focus).toHaveBeenCalled();
  });

  it('dismisses on cancel and on the dialog’s own wa-hide, not a nested popup’s', () => {
    handler('click', '[data-action="cancel-bulk-ticket-action"]')(new Event('click'), {} as Element);
    const dialog = {} as Element,
      hide = handler('wa-hide', '[data-component="bulk-tag-dialog"]');
    hide({ target: {} } as unknown as Event, dialog);
    hide({ target: dialog } as unknown as Event, dialog);
    expect(bindings.dismiss.mock.calls).toEqual([['cancel'], ['hide']]);
  });
});
