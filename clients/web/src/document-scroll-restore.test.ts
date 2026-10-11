import { describe, expect, it } from 'vitest';

import {
  type DocumentScrollRestoreHost,
  installDocumentScrollRestore,
  isTextEntryElement,
  restoreDocumentScroll,
} from './document-scroll-restore';

function element(tag: string, attrs: Record<string, string> = {}, editable = false): Element {
  return {
    tagName: tag.toUpperCase(),
    getAttribute: (name: string) => attrs[name] ?? null,
    isContentEditable: editable,
  } as unknown as Element;
}

function fakeHost(scroll: { x: number; y: number }) {
  const listeners = new Map<string, Set<() => void>>(),
    viewportListeners = new Map<string, Set<() => void>>(),
    timers: Array<() => void> = [],
    scrollCalls: Array<[number, number]> = [];
  const on = (map: Map<string, Set<() => void>>) => ({
    addEventListener: (type: string, fn: () => void) => {
      if (!map.has(type)) map.set(type, new Set());
      map.get(type)!.add(fn);
    },
    removeEventListener: (type: string, fn: () => void) => map.get(type)?.delete(fn),
  });
  const host = {
    document: { activeElement: null as Element | null, ...on(listeners) },
    get scrollX() {
      return scroll.x;
    },
    get scrollY() {
      return scroll.y;
    },
    scrollTo: (x: number, y: number) => {
      scrollCalls.push([x, y]);
      scroll.x = x;
      scroll.y = y;
    },
    visualViewport: on(viewportListeners),
    setTimeout: (fn: () => void) => timers.push(fn),
  };
  const fire = (map: Map<string, Set<() => void>>, type: string) =>
    map.get(type)?.forEach((fn) => {
      fn();
    });
  return {
    host: host as unknown as DocumentScrollRestoreHost & { document: { activeElement: Element | null } },
    scrollCalls,
    focusout: () => fire(listeners, 'focusout'),
    viewportResize: () => fire(viewportListeners, 'resize'),
    flush: () => {
      timers.splice(0).forEach((fn) => {
        fn();
      });
    },
    listenerCount: () => (listeners.get('focusout')?.size ?? 0) + (viewportListeners.get('resize')?.size ?? 0),
  };
}

describe('document scroll restore (HS2-BCA512)', () => {
  it('classifies text-entry surfaces', () => {
    expect(isTextEntryElement(element('textarea'))).toBe(true);
    expect(isTextEntryElement(element('input'))).toBe(true);
    expect(isTextEntryElement(element('input', { type: 'search' }))).toBe(true);
    expect(isTextEntryElement(element('input', { type: 'checkbox' }))).toBe(false);
    expect(isTextEntryElement(element('wa-input'))).toBe(true);
    expect(isTextEntryElement(element('div', {}, true))).toBe(true);
    expect(isTextEntryElement(element('button'))).toBe(false);
    expect(isTextEntryElement(null)).toBe(false);
  });

  it('resets a stray offset only once no text field holds focus', () => {
    const scroll = { x: 0, y: 240 },
      fake = fakeHost(scroll);
    fake.host.document.activeElement = element('textarea');
    expect(restoreDocumentScroll(fake.host)).toBe(false);
    fake.host.document.activeElement = element('button');
    expect(restoreDocumentScroll(fake.host)).toBe(true);
    expect(fake.scrollCalls).toEqual([[0, 0]]);
    expect(restoreDocumentScroll(fake.host)).toBe(false);
  });

  it('walks keyboard open, field-to-field move, dismissal, reopen, and dispose', () => {
    const scroll = { x: 0, y: 0 },
      fake = fakeHost(scroll),
      dispose = installDocumentScrollRestore(fake.host);
    // Keyboard opens on the annotation note and iOS scrolls the clipped root.
    fake.host.document.activeElement = element('textarea');
    scroll.y = 300;
    // Moving to another field: focusout fires but the deferred check sees the next field.
    fake.focusout();
    fake.host.document.activeElement = element('input');
    fake.flush();
    expect(fake.scrollCalls).toEqual([]);
    // Dismissal: focus leaves text entry, the origin returns.
    fake.focusout();
    fake.host.document.activeElement = element('button');
    fake.flush();
    expect(fake.scrollCalls).toEqual([[0, 0]]);
    // Repeated dismissal with no offset is a no-op.
    fake.focusout();
    fake.flush();
    expect(fake.scrollCalls).toHaveLength(1);
    // Keyboard hidden without blur (viewport grows back) while focus already left, sideways offset too.
    scroll.x = 12;
    scroll.y = 80;
    fake.viewportResize();
    fake.flush();
    expect(fake.scrollCalls).toEqual([
      [0, 0],
      [0, 0],
    ]);
    dispose();
    expect(fake.listenerCount()).toBe(0);
    scroll.y = 50;
    fake.focusout();
    fake.flush();
    expect(fake.scrollCalls).toHaveLength(2);
  });

  it('tolerates hosts without a visual viewport', () => {
    const scroll = { x: 0, y: 10 },
      fake = fakeHost(scroll);
    const host = { ...fake.host, visualViewport: null, document: fake.host.document } as DocumentScrollRestoreHost;
    const dispose = installDocumentScrollRestore(host);
    dispose();
    expect(fake.listenerCount()).toBe(0);
  });
});
