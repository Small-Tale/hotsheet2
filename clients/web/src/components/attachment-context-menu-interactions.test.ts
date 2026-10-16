import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ATTACHMENTS_AND_GALLERY_TARGETS } from '../interaction-attrs/attachments-and-gallery';
import { ATTACHMENT_CONTEXT_MENU_HEIGHT } from './attachment-context-menu';
import {
  ATTACHMENT_CONTEXT_MENU_WIDTH,
  attachmentContextMenuPosition,
  type AttachmentContextMenuRequest,
  wireAttachmentContextMenuOpeners,
} from './attachment-context-menu-interactions';

interface Registration {
  event: string;
  selector: string;
  handle: (event: unknown, target: unknown) => unknown;
  attached: boolean;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => ({
  ...(await original<typeof import('kerfjs')>()),
  delegate: (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    const entry = { event, selector, handle, attached: true };
    registrations.push(entry);
    return () => {
      entry.attached = false;
    };
  },
}));

const handler = (event: string, selector: string) => {
  const found = registrations.find((item) => item.event === event && item.selector === selector);
  expect(found, `${event} ${selector}`).toBeDefined();
  return found!.handle;
};
/** A fake element: `closest` answers only the selectors listed in `ancestors`. */
const element = (
  dataset: Record<string, string>,
  rect = { right: 0, bottom: 0 },
  ancestors: Record<string, unknown> = {},
) =>
  ({
    dataset,
    getBoundingClientRect: () => rect,
    closest: (selector: string) => ancestors[selector] ?? null,
  }) as unknown as HTMLElement;
const pointerEvent = (clientX: number, clientY: number) => ({
  clientX,
  clientY,
  defaultPrevented: false,
  preventDefault() {
    this.defaultPrevented = true;
  },
});

beforeEach(() => {
  registrations.length = 0;
  vi.stubGlobal('window', { innerWidth: 1280, innerHeight: 800 });
});
afterEach(() => vi.unstubAllGlobals());

describe('attachment context-menu open and positioning (HS2-1RQA1E)', () => {
  it('clamps the shared menu size inside the viewport margin', () => {
    expect(attachmentContextMenuPosition(100, 120, 1280, 800)).toEqual({ x: 100, y: 120 });
    expect(attachmentContextMenuPosition(1270, 790, 1280, 800)).toEqual({
      x: 1280 - 8 - ATTACHMENT_CONTEXT_MENU_WIDTH,
      y: 800 - 8 - ATTACHMENT_CONTEXT_MENU_HEIGHT,
    });
    // A phone-sized viewport narrower than the menu pins it to the margin.
    expect(attachmentContextMenuPosition(300, 200, 200, 200)).toEqual({ x: 8, y: 8 });
  });

  it('opens row, gallery, and reference menus with their kind, origin, and clamped position', () => {
    const opened: AttachmentContextMenuRequest[] = [];
    let editable = true;
    const dispose = wireAttachmentContextMenuOpeners({} as HTMLElement, {
      open: (request) => opened.push(request),
      canOpenFromPointer: () => editable,
    });
    // Row ellipsis: anchored below the trigger, and the click does not reach the row.
    const stop = vi.fn();
    const row = element({}, { right: 1270, bottom: 40 });
    handler('click', '[data-action="open-attachment-menu"]')({ stopPropagation: stop }, row);
    expect(stop).toHaveBeenCalledOnce();
    expect(opened.at(-1)).toMatchObject({ kind: 'item', origin: 'row', x: 1280 - 8 - 224, y: 40, target: row });
    // Gallery menu button: host kind, anchored at the trigger.
    const galleryButton = element({}, { right: 600, bottom: 64 });
    handler('click', '[data-action="open-gallery-attachment-menu"]')({}, galleryButton);
    expect(opened.at(-1)).toMatchObject({ kind: 'host', origin: 'gallery', x: 600, y: 64 });
    // Right-clicks: a row item, gallery media, and a Markdown reference.
    const contextmenu = handler('contextmenu', '[data-attachment-url]');
    const rowPointer = pointerEvent(50, 790);
    contextmenu(rowPointer, element({ attachmentMenuKind: 'item' }));
    expect(rowPointer.defaultPrevented).toBe(true);
    expect(opened.at(-1)).toMatchObject({ kind: 'item', origin: 'row', x: 50, y: 800 - 8 - 274 });
    contextmenu(
      pointerEvent(400, 300),
      element({}, undefined, { [ATTACHMENTS_AND_GALLERY_TARGETS.attachmentGallery.selector]: {} }),
    );
    contextmenu(pointerEvent(10, 20), element({}));
    expect(opened.slice(-2).map(({ kind, origin }) => [kind, origin])).toEqual([
      ['host', 'gallery'],
      ['host', 'reference'],
    ]);
    // A delete-only provider's row declares the reduced `remote` menu from its ellipsis and a
    // right-click alike (HS2-51ADSG).
    const remoteRow = element(
      {},
      { right: 300, bottom: 40 },
      {
        '[data-attachment-menu-kind]': { dataset: { attachmentMenuKind: 'remote' } },
      },
    );
    handler('click', '[data-action="open-attachment-menu"]')({ stopPropagation: stop }, remoteRow);
    expect(opened.at(-1)).toMatchObject({ kind: 'remote', origin: 'row' });
    contextmenu(pointerEvent(60, 70), element({ attachmentMenuKind: 'remote' }));
    expect(opened.at(-1)).toMatchObject({ kind: 'remote', origin: 'row', x: 60, y: 70 });
    // Read-only: a right-click keeps the native menu and opens nothing; triggers still open.
    editable = false;
    const count = opened.length,
      readOnly = pointerEvent(10, 20);
    contextmenu(readOnly, element({}));
    expect([opened.length, readOnly.defaultPrevented]).toEqual([count, false]);
    // Teardown detaches every opener.
    dispose();
    expect(registrations.every((item) => !item.attached)).toBe(true);
  });
});
