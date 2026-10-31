import { afterEach, expect, it, vi } from 'vitest';

import { focusTopTicketReader, restoreTicketReaderFocus } from './ticket-reader-focus';

afterEach(() => vi.unstubAllGlobals());

function fixture() {
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => frames.push(callback));
  const input = {},
    outside = {},
    close = { focus: vi.fn() };
  const reader = {
    contains: (node: unknown) => node === input || node === close,
    querySelector: () => close,
  };
  let active: unknown = outside,
    current: typeof reader | null = reader;
  const root = {
    get activeElement() {
      return active;
    },
    querySelector: () => current,
  } as unknown as Document;
  return {
    root,
    reader,
    input,
    outside,
    close,
    active: (node: unknown) => {
      active = node;
    },
    current: (node: typeof reader | null) => {
      current = node;
    },
    frame: () => {
      for (const callback of frames.splice(0)) callback(0);
    },
  };
}

it('HS2-XBWGP6 preserves input focused while the initial reader focus is pending', () => {
  const f = fixture();
  focusTopTicketReader(f.root);
  f.frame();
  f.active(f.input);
  f.frame();
  expect(f.close.focus).not.toHaveBeenCalled();
});

it('HS2-XBWGP6 contains outside focus and leaves an already focused reader control alone', () => {
  const f = fixture();
  focusTopTicketReader(f.root);
  f.frame();
  f.frame();
  expect(f.close.focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
  f.active(f.close);
  focusTopTicketReader(f.root);
  f.frame();
  f.frame();
  expect(f.close.focus).toHaveBeenCalledOnce();
});

it('HS2-XBWGP6 resolves the current reader when nesting changes or the reader closes', () => {
  const f = fixture(),
    nextClose = { focus: vi.fn() };
  focusTopTicketReader(f.root);
  f.frame();
  f.current({ contains: () => false, querySelector: () => nextClose });
  f.frame();
  expect(nextClose.focus).toHaveBeenCalledOnce();
  expect(f.close.focus).not.toHaveBeenCalled();
  focusTopTicketReader(f.root);
  f.frame();
  f.current(null);
  f.frame();
  expect(nextClose.focus).toHaveBeenCalledOnce();
  f.current(f.reader);
  focusTopTicketReader(f.root);
  f.frame();
  f.frame();
  expect(f.close.focus).toHaveBeenCalledOnce();
});

function returnFixture() {
  const callbacks: Array<() => void> = [];
  vi.stubGlobal('setTimeout', (callback: () => void) => callbacks.push(callback));
  vi.stubGlobal('CSS', { escape: (value: string) => value });
  const body = {},
    documentElement = {};
  let active: unknown = body;
  const element = (visible = true) => {
    const value = {
      isConnected: true,
      offsetParent: visible ? {} : null,
      getClientRects: () => (visible ? [{}] : []),
      focus: vi.fn(),
    };
    value.focus.mockImplementation(() => {
      active = value;
    });
    return value;
  };
  const trigger = element(),
    row = element(),
    hiddenRow = element(false),
    inspector = element(),
    workspace = element();
  let rows = [hiddenRow, row],
    inspectorTarget: typeof inspector | undefined = inspector,
    workspaceTarget: typeof workspace | undefined = workspace;
  const root = {
    body,
    documentElement,
    get activeElement() {
      return active;
    },
    querySelectorAll: () => rows,
    querySelector: (selector: string) => (selector.startsWith('#app-right-rail') ? inspectorTarget : workspaceTarget),
  } as unknown as Document;
  return {
    root,
    body,
    documentElement,
    trigger,
    row,
    hiddenRow,
    inspector,
    workspace,
    element,
    active: (next: unknown) => {
      active = next;
    },
    rows: (next: typeof rows) => {
      rows = next;
    },
    inspectorTarget: (next: typeof inspectorTarget) => {
      inspectorTarget = next;
    },
    workspaceTarget: (next: typeof workspaceTarget) => {
      workspaceTarget = next;
    },
    flush: () => {
      for (const callback of callbacks.splice(0)) callback();
    },
  };
}

it('HS2-FRFPA5 returns to the live opener only when deferred delivery arrives', () => {
  const f = returnFixture();
  restoreTicketReaderFocus(f.trigger as unknown as HTMLElement, 'HS2-DEMO01', f.root);
  expect(f.trigger.focus).not.toHaveBeenCalled();
  f.flush();
  expect(f.trigger.focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
  expect(f.row.focus).not.toHaveBeenCalled();
});

it('HS2-FRFPA5 preserves newer visible focus through queued returns and a further ordinary close', () => {
  const f = returnFixture(),
    newer = f.element();
  restoreTicketReaderFocus(f.trigger as unknown as HTMLElement, 'HS2-DEMO01', f.root);
  restoreTicketReaderFocus(undefined, 'HS2-DEMO01', f.root);
  f.active(newer);
  f.flush();
  expect(f.trigger.focus).not.toHaveBeenCalled();
  expect(f.row.focus).not.toHaveBeenCalled();
  expect(f.root.activeElement).toBe(newer);
  f.active(f.body);
  restoreTicketReaderFocus(f.trigger as unknown as HTMLElement, 'HS2-DEMO01', f.root);
  f.flush();
  expect(f.trigger.focus).toHaveBeenCalledOnce();
  restoreTicketReaderFocus(undefined, 'HS2-DEMO01', f.root);
  f.flush();
  expect(f.root.activeElement).toBe(f.trigger);
  expect(f.row.focus).not.toHaveBeenCalled();
});

it.each(['absent', 'body', 'document root', 'hidden', 'disconnected'] as const)(
  'HS2-FRFPA5 allows focus restoration when current focus is %s',
  (state) => {
    const f = returnFixture(),
      hidden = f.element(false),
      disconnected = f.element();
    disconnected.isConnected = false;
    f.active({ absent: null, body: f.body, 'document root': f.documentElement, hidden, disconnected }[state]);
    restoreTicketReaderFocus(f.trigger as unknown as HTMLElement, 'HS2-DEMO01', f.root);
    f.flush();
    expect(f.trigger.focus).toHaveBeenCalledOnce();
  },
);

it('HS2-FRFPA5 retains row, inspector and workspace fallbacks through removal and refill', () => {
  const f = returnFixture();
  f.trigger.isConnected = false;
  restoreTicketReaderFocus(f.trigger as unknown as HTMLElement, 'HS2-DEMO01', f.root);
  f.flush();
  expect(f.hiddenRow.focus).not.toHaveBeenCalled();
  expect(f.row.focus).toHaveBeenCalledOnce();
  expect(f.inspector.focus).not.toHaveBeenCalled();
  f.rows([]);
  f.active(f.body);
  restoreTicketReaderFocus(undefined, 'HS2-DEMO01', f.root);
  f.flush();
  expect(f.inspector.focus).toHaveBeenCalledOnce();
  expect(f.workspace.focus).not.toHaveBeenCalled();
  f.inspectorTarget(undefined);
  f.active(f.body);
  restoreTicketReaderFocus(undefined, 'HS2-DEMO01', f.root);
  f.flush();
  expect(f.workspace.focus).toHaveBeenCalledOnce();
  f.workspaceTarget(undefined);
  f.active(f.body);
  restoreTicketReaderFocus(undefined, 'HS2-DEMO01', f.root);
  expect(() => f.flush()).not.toThrow();
  expect(f.root.activeElement).toBe(f.body);
  f.workspaceTarget(f.workspace);
  restoreTicketReaderFocus(undefined, 'HS2-DEMO01', f.root);
  f.flush();
  expect(f.workspace.focus).toHaveBeenCalledTimes(2);
});
