import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MOBILE_TERMINAL_COLUMNS_CHANGE_EVENT } from './mobile-terminal-columns';
import { TERMINAL_EDIT_MENU_EVENT } from './terminal-clipboard';
import {
  TERMINAL_DRAWER_RESIZE_END_EVENT,
  TERMINAL_VIEWPORT_PARK_EVENT,
  TERMINAL_VIEWPORT_PARKED_CLOSED_EVENT,
  TERMINAL_VIEWPORT_RESUME_EVENT,
} from './terminal-viewport';
import { mountStaticTerminalViewportRuntime, mountTerminalViewportRuntime } from './terminal-viewport-runtime';

const allocated = vi.hoisted(() => ({
  proposed: { cols: 80, rows: 24 },
  openElement: false,
  terminals: [] as Array<{
    dispose: ReturnType<typeof vi.fn>;
    render: ReturnType<typeof vi.fn>;
    input: ReturnType<typeof vi.fn>;
    registerMarker: ReturnType<typeof vi.fn>;
    resize: ReturnType<typeof vi.fn>;
    reset: ReturnType<typeof vi.fn>;
    scrollToLine: ReturnType<typeof vi.fn>;
    write: ReturnType<typeof vi.fn>;
    buffer: { active: { baseY: number; cursorY: number; viewportY: number } };
    focus: ReturnType<typeof vi.fn>;
    options: Record<string, unknown>;
  }>,
}));
vi.mock('@xterm/xterm', () => ({
  Terminal: class {
    cols = 80;
    rows = 24;
    dispose = vi.fn();
    render = vi.fn();
    input = vi.fn();
    registerMarker = vi.fn(() => ({ line: 8, dispose: vi.fn() }));
    resize = vi.fn((cols: number, rows: number) => {
      this.cols = cols;
      this.rows = rows;
    });
    reset = vi.fn();
    scrollToLine = vi.fn();
    write = vi.fn();
    buffer = { active: { baseY: 20, cursorY: 3, viewportY: 20 } };
    focus = vi.fn();
    scrollLines = vi.fn();
    modes = { applicationCursorKeysMode: false };
    selection = '';
    hasSelection = vi.fn(() => this.selection !== '');
    select = vi.fn((col: number, row: number, length: number) => {
      this.selection = `${String(col)},${String(row)}+${String(length)}`;
    });
    clearSelection = vi.fn(() => {
      this.selection = '';
    });
    getSelection = vi.fn(() => this.selection);
    element?: { style: Record<string, string> };
    options: Record<string, unknown>;
    constructor(options: Record<string, unknown> = {}) {
      this.options = { ...options };
      allocated.terminals.push(this);
    }
    open() {
      if (allocated.openElement) this.element = { style: {} };
    }
    loadAddon() {}
    onRender() {
      return { dispose: this.render };
    }
    onData() {
      return { dispose: this.input };
    }
  },
}));
vi.mock('@xterm/addon-fit', () => ({
  FitAddon: class {
    fit = vi.fn();
    dispose() {}
    proposeDimensions() {
      return allocated.proposed;
    }
  },
}));
const webglAddons = vi.hoisted(() => [] as Array<{ disposed: boolean }>);
vi.mock('@xterm/addon-webgl', () => ({
  WebglAddon: class {
    disposed = false;
    constructor() {
      webglAddons.push(this);
    }
    onContextLoss() {}
    dispose() {
      this.disposed = true;
    }
  },
}));

const resize: Array<{ disconnect: ReturnType<typeof vi.fn>; callback: () => void }> = [],
  intersections: Array<{ disconnect: ReturnType<typeof vi.fn> }> = [],
  sockets: Array<EventTarget & { close: ReturnType<typeof vi.fn>; readyState: number }> = [],
  scheduledTimeouts: Array<() => void> = [];
const windowMock = {
  innerWidth: 390,
  addEventListener: vi.fn(),
  removeEventListener: vi.fn(),
  requestAnimationFrame: vi.fn(() => 1),
  cancelAnimationFrame: vi.fn(),
  setTimeout: vi.fn((callback: () => void) => {
    scheduledTimeouts.push(callback);
    return scheduledTimeouts.length;
  }),
  setInterval: vi.fn(() => 1),
  clearTimeout: vi.fn(),
  clearInterval: vi.fn(),
};
let throwSocket = false,
  throwObservation = false;

beforeEach(() => {
  windowMock.innerWidth = 390;
  allocated.terminals.length = 0;
  allocated.proposed = { cols: 80, rows: 24 };
  allocated.openElement = false;
  resize.length = 0;
  intersections.length = 0;
  sockets.length = 0;
  scheduledTimeouts.length = 0;
  throwSocket = false;
  throwObservation = false;
  vi.stubGlobal('getComputedStyle', () => ({ getPropertyValue: () => '#000' }));
  vi.clearAllMocks();
  // clearAllMocks keeps per-test mockImplementation overrides (frame/timer stubs); reset every
  // window stub to its original implementation so each test starts identically in any order
  // (HS2-M2KHW1).
  for (const value of Object.values(windowMock)) if (vi.isMockFunction(value)) value.mockReset();
  vi.stubGlobal('window', windowMock);
  vi.stubGlobal(
    'ResizeObserver',
    class {
      disconnect = vi.fn();
      constructor(readonly callback: () => void) {
        resize.push(this);
      }
      observe() {
        if (throwObservation) throw new Error('observe failed');
      }
    },
  );
  vi.stubGlobal(
    'IntersectionObserver',
    class {
      disconnect = vi.fn();
      constructor() {
        intersections.push(this);
      }
      observe() {}
    },
  );
  vi.stubGlobal(
    'WebSocket',
    class extends EventTarget {
      static OPEN = 1;
      readyState = 0;
      close = vi.fn();
      constructor() {
        super();
        if (throwSocket) throw new Error('socket failed');
        sockets.push(this);
      }
      send() {}
    },
  );
});
afterEach(() => vi.unstubAllGlobals());

function element() {
  const mock = {
    dataset: { displayMode: 'interactive' },
    style: {},
    classList: { contains: () => false },
    closest: () => null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  };
  return {
    viewport: mock as unknown as HTMLElement,
    removed: mock.removeEventListener,
    listeners: mock.addEventListener,
  };
}

describe('transactional terminal initialization (HS2-3ZBQDG)', () => {
  it('disposes allocations after socket setup throws, then mounts and disposes a fresh viewer exactly once', () => {
    const { viewport, removed } = element();
    throwSocket = true;
    expect(() => mountTerminalViewportRuntime(viewport, { url: 'ws://lan/terminal', viewerId: 'one' })).toThrow(
      'socket failed',
    );
    expect(allocated.terminals[0].dispose).toHaveBeenCalledTimes(1);
    expect(allocated.terminals[0].render).toHaveBeenCalledTimes(1);
    expect(allocated.terminals[0].input).toHaveBeenCalledTimes(1);
    expect(resize[0].disconnect).toHaveBeenCalledTimes(1);
    expect(intersections[0].disconnect).toHaveBeenCalledTimes(1);
    // Drawer resize-end plus the HS2-WMN626 mobile column-change listener.
    expect(windowMock.removeEventListener.mock.calls.map(([name]) => name as string)).toEqual([
      MOBILE_TERMINAL_COLUMNS_CHANGE_EVENT,
      TERMINAL_DRAWER_RESIZE_END_EVENT,
    ]);
    expect(windowMock.cancelAnimationFrame).toHaveBeenCalledWith(1);
    // Interaction listeners plus the HS2-KFBRSB touch-scroll listeners are all removed on teardown.
    expect(removed.mock.calls.map(([name]) => name as string).sort()).toEqual(
      [
        'click',
        // HS2-KKP8YJ: the long-press suppresses the native touch context menu.
        'contextmenu',
        'focusin',
        'focusin',
        'focusout',
        'hotsheet-terminal-key',
        'hotsheet-terminal-paste',
        'hotsheet-terminal-read-text',
        TERMINAL_VIEWPORT_PARK_EVENT,
        TERMINAL_VIEWPORT_RESUME_EVENT,
        'pointerdown',
        'touchcancel',
        'touchend',
        'touchmove',
        'touchstart',
      ].sort(),
    );
    throwSocket = false;
    const dispose = mountTerminalViewportRuntime(viewport, { url: 'ws://lan/terminal', viewerId: 'two' });
    dispose();
    dispose();
    expect(allocated.terminals[1].dispose).toHaveBeenCalledTimes(1);
    expect(resize[1].disconnect).toHaveBeenCalledTimes(1);
    expect(intersections[1].disconnect).toHaveBeenCalledTimes(1);
    expect(sockets[0].close).toHaveBeenCalledTimes(1);
  });

  it('turns a still touch hold into a bubbling edit-menu request and suppresses its tap (HS2-KKP8YJ)', () => {
    const { viewport, listeners } = element(),
      dispatched: Event[] = [];
    (viewport as unknown as { dispatchEvent: (event: Event) => boolean }).dispatchEvent = (event) => {
      dispatched.push(event);
      return true;
    };
    const dispose = mountTerminalViewportRuntime(viewport, { url: 'ws://lan/terminal', viewerId: 'press' });
    const handler = (name: string) =>
        listeners.mock.calls.find(([type]) => type === name)![1] as (event: unknown) => void,
      touch = (x: number, y: number) => ({ clientX: x, clientY: y }),
      prevented = () => vi.fn();
    const editRequests = () => dispatched.filter((event) => event.type === TERMINAL_EDIT_MENU_EVENT);

    // An 80×24 grid of 10×20 px cells at the origin, scrolled to buffer line 20, with "hello world" on
    // every line (HS2-EYR96N).
    const terminal = allocated.terminals.at(-1)! as unknown as {
      element: Record<string, unknown>;
      buffer: { active: Record<string, unknown> };
      select: ReturnType<typeof vi.fn>;
      clearSelection: ReturnType<typeof vi.fn>;
    };
    terminal.element = {
      style: {},
      querySelector: () => ({ getBoundingClientRect: () => ({ left: 0, top: 0, width: 800, height: 480 }) }),
    };
    terminal.buffer.active.getLine = () => ({ isWrapped: false, translateToString: () => 'hello world' });

    // Hold still: the timer selects the word under the finger ("hello" at row 23) instead of opening the
    // menu; the native menu is suppressed.
    handler('touchstart')({ touches: [touch(30, 60)], timeStamp: 0 });
    const contextMenu = { preventDefault: prevented() };
    handler('contextmenu')(contextMenu);
    expect(contextMenu.preventDefault).toHaveBeenCalledTimes(1);
    scheduledTimeouts.at(-1)!();
    expect(terminal.select).toHaveBeenLastCalledWith(0, 23, 5);
    expect(editRequests()).toHaveLength(0);
    // Dragging on extends the selection to the cell under the finger and never scrolls.
    const afterFire = { touches: [touch(30, 140)], timeStamp: 50, preventDefault: prevented() };
    handler('touchmove')(afterFire);
    expect(afterFire.preventDefault).toHaveBeenCalledTimes(1);
    expect(terminal.select).toHaveBeenLastCalledWith(0, 23, 4 * 80 + 3 + 1);
    // The lift opens the edit menu where the finger lifted, flagged with the selection, and is no tap.
    const lift = { changedTouches: [touch(30, 140)], timeStamp: 60, preventDefault: prevented() };
    handler('touchend')(lift);
    expect(lift.preventDefault).toHaveBeenCalledTimes(1);
    expect(editRequests()).toHaveLength(1);
    const request = editRequests()[0] as CustomEvent<{ x: number; y: number; selection: boolean }>;
    expect(request.bubbles).toBe(true);
    expect(request.detail).toEqual({ x: 30, y: 140, selection: true });
    expect(terminal.clearSelection).not.toHaveBeenCalled();

    // A quick tap drops the selection, keeps its default (focus), and leaves the desktop right-click menu
    // alone afterwards.
    handler('touchstart')({ touches: [touch(10, 10)], timeStamp: 100 });
    expect(terminal.clearSelection).toHaveBeenCalledTimes(1);
    const tap = { changedTouches: [touch(10, 10)], timeStamp: 120, preventDefault: prevented() };
    handler('touchend')(tap);
    expect(tap.preventDefault).not.toHaveBeenCalled();
    expect(windowMock.clearTimeout).toHaveBeenCalled();
    const rightClick = { preventDefault: prevented() };
    handler('contextmenu')(rightClick);
    expect(rightClick.preventDefault).not.toHaveBeenCalled();

    // A drag scrolls instead: the hold is abandoned and never fires.
    handler('touchstart')({ touches: [touch(10, 10)], timeStamp: 200 });
    const timer = scheduledTimeouts.length;
    const drag = { touches: [touch(10, 60)], timeStamp: 230, preventDefault: prevented() };
    handler('touchmove')(drag);
    expect(drag.preventDefault).toHaveBeenCalledTimes(1);
    handler('touchend')({ changedTouches: [touch(10, 60)], timeStamp: 240, preventDefault: prevented() });
    expect(scheduledTimeouts).toHaveLength(timer);
    expect(editRequests()).toHaveLength(1);

    // A second finger cancels a pending hold.
    handler('touchstart')({ touches: [touch(10, 10)], timeStamp: 300 });
    handler('touchstart')({ touches: [touch(10, 10), touch(80, 80)], timeStamp: 310 });
    const pinchEnd = { changedTouches: [touch(10, 10)], timeStamp: 320, preventDefault: prevented() };
    handler('touchend')(pinchEnd);
    expect(pinchEnd.preventDefault).not.toHaveBeenCalled();
    dispose();
  });

  it('cleans a static mount when observation fails after terminal/render allocation, then supports refill', () => {
    throwObservation = true;
    expect(() => mountStaticTerminalViewportRuntime(element().viewport, { output: 'hello' })).toThrow('observe failed');
    expect(allocated.terminals[0].dispose).toHaveBeenCalledTimes(1);
    expect(allocated.terminals[0].render).toHaveBeenCalledTimes(1);
    expect(resize[0].disconnect).toHaveBeenCalledTimes(1);
    throwObservation = false;
    const dispose = mountStaticTerminalViewportRuntime(element().viewport, { output: 'recovered' });
    dispose();
    dispose();
    expect(allocated.terminals[1].dispose).toHaveBeenCalledTimes(1);
    expect(resize[1].disconnect).toHaveBeenCalledTimes(1);
    expect(windowMock.cancelAnimationFrame).toHaveBeenCalledWith(1);
  });

  it('atomically replaces emulator state with explicit lag and reconnect replay bytes (HS2-BQR774)', () => {
    const { viewport } = element(),
      dispose = mountTerminalViewportRuntime(viewport, { url: 'ws://lan/terminal', viewerId: 'viewer' }),
      terminal = allocated.terminals[0],
      replay = new TextEncoder().encode('authoritative replay\r\n').buffer;
    sockets[0].readyState = 1;
    sockets[0].dispatchEvent(new Event('open'));
    sockets[0].dispatchEvent(new MessageEvent('message', { data: replay }));
    expect(terminal.reset).not.toHaveBeenCalled();
    expect(terminal.write).toHaveBeenCalledTimes(1);

    sockets[0].dispatchEvent(new MessageEvent('message', { data: JSON.stringify({ terminal_replay: 'replace' }) }));
    sockets[0].dispatchEvent(new MessageEvent('message', { data: replay }));
    expect(terminal.reset).not.toHaveBeenCalled();
    expect(terminal.write).toHaveBeenCalledTimes(2);
    const reset = [0x1b, 0x63, 0x1b, 0x5b, 0x33, 0x4a, 0x1b, 0x5b, 0x32, 0x4a, 0x1b, 0x5b, 0x48];
    expect([...terminal.write.mock.calls[1][0]]).toEqual([...reset, ...new Uint8Array(replay)]);

    for (let cycle = 0; cycle < 2; cycle += 1) {
      sockets.at(-1)!.dispatchEvent(new Event('close'));
      scheduledTimeouts.at(-1)!();
      sockets.at(-1)!.readyState = 1;
      sockets.at(-1)!.dispatchEvent(new Event('open'));
      sockets.at(-1)!.dispatchEvent(new MessageEvent('message', { data: replay }));
    }
    expect(terminal.reset).not.toHaveBeenCalled();
    expect(terminal.write).toHaveBeenCalledTimes(4);
    expect([...terminal.write.mock.calls[2][0]]).toEqual([...reset, ...new Uint8Array(replay)]);
    expect([...terminal.write.mock.calls[3][0]]).toEqual([...reset, ...new Uint8Array(replay)]);

    sockets.at(-1)!.dispatchEvent(new Event('close'));
    scheduledTimeouts.at(-1)!();
    sockets.at(-1)!.readyState = 1;
    sockets.at(-1)!.dispatchEvent(new Event('open'));
    sockets.at(-1)!.dispatchEvent(new MessageEvent('message', { data: new ArrayBuffer(0) }));
    expect([...terminal.write.mock.calls[4][0]]).toEqual(reset);
    dispose();
  });

  it('ignores repeated size heartbeats and anchors a scrolled viewport across a real resize (HS2-CJBZPW)', () => {
    const { viewport } = element();
    mountTerminalViewportRuntime(viewport, { url: 'ws://lan/terminal', viewerId: 'viewer' });
    const terminal = allocated.terminals[0],
      size = (cols: number, rows: number) =>
        sockets[0].dispatchEvent(
          new MessageEvent('message', {
            data: JSON.stringify({ pty_size: { cols, rows }, driven_by: 'viewer' }),
          }),
        );
    terminal.buffer.active = { baseY: 40, cursorY: 4, viewportY: 12 };
    size(80, 24);
    expect(terminal.resize).not.toHaveBeenCalled();
    expect(terminal.registerMarker).not.toHaveBeenCalled();

    allocated.proposed = { cols: 100, rows: 30 };
    size(100, 30);
    expect(terminal.registerMarker).toHaveBeenCalledWith(-32);
    expect(terminal.resize).toHaveBeenCalledWith(100, 30);
    expect(terminal.scrollToLine).toHaveBeenCalledWith(8);
    expect(terminal.registerMarker.mock.results[0].value.dispose).toHaveBeenCalledTimes(1);

    terminal.buffer.active = { baseY: 40, cursorY: 4, viewportY: 40 };
    allocated.proposed = { cols: 120, rows: 35 };
    size(120, 35);
    expect(terminal.resize).toHaveBeenCalledWith(120, 35);
    expect(terminal.registerMarker).toHaveBeenCalledTimes(1);
  });
});

describe('terminal geometry settling (HS2-GSRZX6)', () => {
  it('claims a stable layout once per frame, with no delayed re-settle that would resize the PTY again', () => {
    const frames = new Map<number, () => void>(),
      sent: string[] = [];
    let nextFrame = 0;
    windowMock.requestAnimationFrame.mockImplementation(((callback: () => void) => {
      nextFrame += 1;
      frames.set(nextFrame, callback);
      return nextFrame;
    }) as never);
    windowMock.cancelAnimationFrame.mockImplementation(((handle: number) => {
      frames.delete(handle);
    }) as never);
    const { viewport } = element(),
      dispose = mountTerminalViewportRuntime(viewport, { url: 'ws://lan/terminal', viewerId: 'viewer' }),
      socket = sockets[0] as unknown as EventTarget & { readyState: number; send: (value: string) => void },
      terminal = allocated.terminals[0],
      runFrames = () => {
        const due = [...frames.values()];
        frames.clear();
        for (const frame of due) frame();
      },
      claims = () => sent.filter((value) => value.includes('"viewer"'));
    socket.send = (value: string) => sent.push(value);
    socket.readyState = 1;
    socket.dispatchEvent(new Event('open'));
    runFrames();
    expect(claims()).toHaveLength(1);
    expect(viewport.dataset.gridSize).toBe('80x24');
    expect(scheduledTimeouts, 'geometry schedules no delayed second pass').toHaveLength(0);

    // Burst of layout changes in one frame: one coalesced pass, one claim, at the new size.
    allocated.proposed = { cols: 100, rows: 30 };
    (terminal.resize as unknown as (cols: number, rows: number) => void)(100, 30);
    resize[0].callback();
    resize[0].callback();
    runFrames();
    expect(claims()).toHaveLength(2);
    expect(JSON.parse(claims()[1]) as unknown).toMatchObject({ resize: { cols: 100, rows: 30 } });
    expect(scheduledTimeouts).toHaveLength(0);
    dispose();
    windowMock.cancelAnimationFrame.mockReset();
  });
});

describe('initial terminal auto-focus retries (HS2-Y9VK3C)', () => {
  function mountAutoFocused(innerWidth = 1440) {
    // Automatic focus is a larger-device behavior; phones focus only on the user's tap (HS2-YD7RZ7).
    windowMock.innerWidth = innerWidth;
    allocated.openElement = true;
    const frames: Array<() => void> = [],
      body = {},
      doc = { activeElement: body as unknown, body };
    windowMock.requestAnimationFrame.mockImplementation(((callback: () => void) => {
      frames.push(callback);
      return frames.length;
    }) as never);
    vi.stubGlobal('document', doc);
    vi.stubGlobal(
      'Element',
      class {
        readonly fake = true;
      },
    );
    const { viewport, listeners } = element(),
      dispose = mountTerminalViewportRuntime(viewport, {
        url: 'ws://lan/terminal',
        viewerId: 'viewer',
        autoFocus: true,
      }),
      terminal = allocated.terminals[0],
      runRetries = () => {
        for (const frame of frames.splice(0)) frame();
        for (const timeout of scheduledTimeouts.splice(0)) timeout();
      },
      focusLanded = () => {
        for (const [type, handler] of listeners.mock.calls as Array<[string, () => void]>)
          if (type === 'focusin') handler();
      };
    return { terminal, doc, body, runRetries, focusLanded, dispose };
  }

  it('ignores an automatic focus request in the mobile layout (HS2-YD7RZ7)', () => {
    const { terminal, runRetries, dispose } = mountAutoFocused(390);
    runRetries();
    expect(terminal.focus).not.toHaveBeenCalled();
    dispose();
  });

  it('stops retrying once the requested focus landed, so an unmounted Exit control cannot pull focus back', () => {
    const { terminal, doc, body, runRetries, focusLanded, dispose } = mountAutoFocused();
    expect(terminal.focus).toHaveBeenCalledTimes(1);
    focusLanded();
    // The user taps Exit; the pill unmounts and focus falls back to <body> before the settle retry.
    doc.activeElement = body;
    runRetries();
    runRetries();
    expect(terminal.focus).toHaveBeenCalledTimes(1);
    dispose();
  });

  it('keeps retrying while the requested focus has not landed yet', () => {
    const { terminal, runRetries, dispose } = mountAutoFocused();
    expect(terminal.focus).toHaveBeenCalledTimes(1);
    runRetries();
    expect(terminal.focus.mock.calls.length).toBeGreaterThan(1);
    dispose();
  });
});

describe('parked terminal viewports (HS2-WGTQ6X)', () => {
  it('releases WebGL while parked, restores it after the first resumed frame, and stops at a closed socket', () => {
    webglAddons.length = 0;
    windowMock.innerWidth = 1440;
    vi.stubGlobal('navigator', { userAgent: 'Mozilla/5.0 Gecko/20100101 Firefox/150.0' });
    const frames: Array<() => void> = [];
    windowMock.requestAnimationFrame.mockImplementation(((callback: () => void) => {
      frames.push(callback);
      return frames.length;
    }) as never);
    const dispatched: string[] = [],
      handlers = new Map<string, (event: Event) => void>(),
      viewport = {
        dataset: { displayMode: 'interactive' } as Record<string, string>,
        style: {},
        classList: { contains: (name: string) => name === 'terminal-viewport--dedicated' },
        closest: () => null,
        addEventListener: (type: string, handler: (event: Event) => void) => handlers.set(type, handler),
        removeEventListener: vi.fn(),
        dispatchEvent: (event: Event) => dispatched.push(event.type),
      } as unknown as HTMLElement,
      dispose = mountTerminalViewportRuntime(viewport, { url: 'ws://lan/terminal', viewerId: 'viewer' }),
      socket = sockets[0];
    expect(viewport.dataset.renderer).toBe('webgl');
    socket.readyState = 1;
    socket.dispatchEvent(new Event('open'));

    handlers.get(TERMINAL_VIEWPORT_PARK_EVENT)!(new CustomEvent(TERMINAL_VIEWPORT_PARK_EVENT));
    expect(webglAddons[0].disposed).toBe(true);
    expect(viewport.dataset.renderer).toBe('dom');

    // Resume paints with the DOM renderer first, then restores WebGL on the next frame.
    frames.length = 0;
    handlers.get(TERMINAL_VIEWPORT_RESUME_EVENT)!(
      new CustomEvent(TERMINAL_VIEWPORT_RESUME_EVENT, { detail: { focus: true } }),
    );
    expect(viewport.dataset.renderer).toBe('dom');
    expect(allocated.terminals[0].focus).toHaveBeenCalled();
    for (const frame of frames.splice(0)) frame();
    expect(webglAddons).toHaveLength(2);
    expect(viewport.dataset.renderer).toBe('webgl');

    // Parked again, a closed socket is reported to the owner instead of reconnecting.
    handlers.get(TERMINAL_VIEWPORT_PARK_EVENT)!(new CustomEvent(TERMINAL_VIEWPORT_PARK_EVENT));
    scheduledTimeouts.length = 0;
    socket.dispatchEvent(new Event('close'));
    expect(dispatched).toContain(TERMINAL_VIEWPORT_PARKED_CLOSED_EVENT);
    expect(viewport.dataset.connection).toBe('closed');
    expect(scheduledTimeouts, 'no background reconnect').toHaveLength(0);
    expect(sockets).toHaveLength(1);
    dispose();
  });

  it('keeps reconnecting a visible viewport whose socket closes', () => {
    const { viewport } = element();
    mountTerminalViewportRuntime(viewport, { url: 'ws://lan/terminal', viewerId: 'viewer' });
    sockets[0].readyState = 1;
    sockets[0].dispatchEvent(new Event('open'));
    sockets[0].dispatchEvent(new Event('close'));
    expect(viewport.dataset.connection).toBe('reconnecting');
    scheduledTimeouts.at(-1)!();
    expect(sockets).toHaveLength(2);
  });
});

describe('unpainted until the first geometry pass (HS2-MHPHZB)', () => {
  it('marks a fresh magnified viewport not ready, keeps a settled one ready, and leaves dedicated ones paintable', () => {
    const dedicated = element().viewport;
    const disposeDedicated = mountTerminalViewportRuntime(dedicated, {
      url: 'ws://lan/terminal',
      viewerId: 'dedicated',
    });
    expect(dedicated.dataset.geometryReady).toBeUndefined();
    disposeDedicated();
    const magnifiedElement = () => {
      const created = element().viewport;
      (created as unknown as { closest: (selector: string) => unknown }).closest = (selector: string) =>
        selector === '[data-fixed-aspect-terminal-card="magnified"]' ? {} : null;
      return created;
    };
    const fresh = magnifiedElement();
    const disposeFresh = mountTerminalViewportRuntime(fresh, { url: 'ws://lan/terminal', viewerId: 'fresh' });
    expect(fresh.dataset.geometryReady).toBe('false');
    disposeFresh();
    const settled = magnifiedElement();
    settled.dataset.geometryReady = 'true';
    const disposeSettled = mountTerminalViewportRuntime(settled, { url: 'ws://lan/terminal', viewerId: 'settled' });
    expect(settled.dataset.geometryReady).toBe('true');
    disposeSettled();
  });
});

describe('scaled-preview canvas fits its frame (HS2-S7E53Q)', () => {
  // The 1000×600 mocked screen filling the 1280×768 canvas (less the 1px guard).
  const canvasFill = Math.min(1279 / 1000, 767 / 600);
  // A mirroring preview fits that grid footprint, not the whole canvas, to its frame (HS2-RBS46R).
  const mirrorFrameScale = (width: number, height: number) =>
    Math.min(width / (1000 * canvasFill), height / (600 * canvasFill));
  function previewElement(gridPolicy?: string) {
    const created = element().viewport,
      frame = { clientWidth: 318, clientHeight: 240 };
    Object.assign(created.dataset, { displayMode: 'scaled-preview' }, gridPolicy ? { gridPolicy } : {});
    Object.assign(created as unknown as Record<string, unknown>, { parentElement: frame, removeAttribute: vi.fn() });
    return { viewport: created, frame };
  }
  function captureFrames() {
    const frames = new Map<number, () => void>();
    let next = 0;
    windowMock.requestAnimationFrame.mockImplementation(((callback: () => void) => {
      next += 1;
      frames.set(next, callback);
      return next;
    }) as never);
    windowMock.cancelAnimationFrame.mockImplementation(((handle: number) => {
      frames.delete(handle);
    }) as never);
    return () => {
      for (let pass = 0; pass < 3 && frames.size; pass += 1) {
        const due = [...frames.values()];
        frames.clear();
        for (const callback of due) callback();
      }
    };
  }
  function giveScreen(terminal: (typeof allocated.terminals)[number]) {
    Object.assign((terminal as unknown as { element: Record<string, unknown> }).element, {
      querySelector: () => ({ offsetWidth: 1000, offsetHeight: 600 }),
    });
  }

  it('scales a live TerminalPreview (no grid policy) to its frame and follows resizes without claiming sizing', async () => {
    allocated.openElement = true;
    const runFrames = captureFrames(),
      { viewport, frame } = previewElement(),
      dispose = mountTerminalViewportRuntime(viewport, { url: 'ws://lan/terminal', viewerId: 'close-preview' }),
      sent: string[] = [];
    giveScreen(allocated.terminals[0]);
    expect(viewport.style.width).toBe('1280px');
    expect(viewport.style.height).toBe('768px');
    const socket = sockets[0] as unknown as EventTarget & { readyState: number; send: (value: string) => void };
    socket.send = (value: string) => sent.push(value);
    socket.readyState = 1;
    socket.dispatchEvent(new Event('open'));
    runFrames();
    // A 318px frame scales the 1280px canvas by 318/1280 instead of showing a native-size crop.
    expect(viewport.style.transform).toBe(`scale(${318 / 1280})`);
    expect(viewport.dataset.previewScale).toBe(String(318 / 1280));
    // A height-bound frame scales by height.
    Object.assign(frame, { clientWidth: 1200, clientHeight: 360 });
    resize[0].callback();
    runFrames();
    expect(viewport.style.transform).toBe(`scale(${360 / 768})`);
    // Shrinking back (repeated, coalesced observations) tracks the new frame.
    Object.assign(frame, { clientWidth: 318, clientHeight: 240 });
    resize[0].callback();
    resize[0].callback();
    runFrames();
    expect(viewport.style.transform).toBe(`scale(${318 / 1280})`);
    // A server size message reconciles the inner mismatch scale without dropping the canvas scale.
    socket.dispatchEvent(
      new MessageEvent('message', {
        data: JSON.stringify({ pty_size: { cols: 200, rows: 60 }, driven_by: 'drawer' }),
      }),
    );
    await Promise.resolve();
    expect(viewport.style.transform).toBe(`scale(${318 / 1280})`);
    // The borrowed terminal keeps its geometry: the preview never claims PTY sizing (HS2-6C0WZN).
    expect(viewport.dataset.sizingFocus).toBe('false');
    expect(sent.length).toBeGreaterThan(0);
    expect(sent.every((value) => !value.includes('"focus":true'))).toBe(true);
    dispose();
  });

  it('keeps the dashboard tile preview on the same scale contract', () => {
    allocated.openElement = true;
    const runFrames = captureFrames(),
      { viewport } = previewElement('dashboard-80x24'),
      dispose = mountTerminalViewportRuntime(viewport, { url: 'ws://lan/terminal', viewerId: 'tile' });
    giveScreen(allocated.terminals[0]);
    sockets[0].readyState = 1;
    sockets[0].dispatchEvent(new Event('open'));
    runFrames();
    expect(viewport.style.transform).toBe(`scale(${318 / 1280})`);
    expect(viewport.dataset.scale).toBe(String(318 / 1280));
    expect(viewport.dataset.previewScale).toBe(String(318 / 1280));
    dispose();
  });

  it('scales a static (demo) TerminalPreview canvas to its frame and tracks resizes', () => {
    allocated.openElement = true;
    const runFrames = captureFrames(),
      { viewport, frame } = previewElement(),
      dispose = mountStaticTerminalViewportRuntime(viewport, { output: 'PASS\r\n' });
    giveScreen(allocated.terminals[0]);
    runFrames();
    expect(viewport.style.transform).toBe(`scale(${mirrorFrameScale(318, 240)})`);
    expect(viewport.dataset.geometryReady).toBe('true');
    Object.assign(frame, { clientWidth: 640, clientHeight: 600 });
    resize[0].callback();
    runFrames();
    expect(viewport.style.transform).toBe(`scale(${mirrorFrameScale(640, 600)})`);
    dispose();
  });

  it('leaves a static interactive viewport untransformed', () => {
    allocated.openElement = true;
    const runFrames = captureFrames(),
      { viewport } = element(),
      dispose = mountStaticTerminalViewportRuntime(viewport, { output: 'hello' });
    giveScreen(allocated.terminals[0]);
    runFrames();
    expect(viewport.style.transform).toBeUndefined();
    expect(viewport.dataset.previewScale).toBeUndefined();
    dispose();
  });

  describe('a non-claiming preview mirrors the PTY grid at a legible font (HS2-XHBDRV)', () => {
    function claims(sent: string[]) {
      return sent
        .map((value) => (JSON.parse(value) as { resize?: Record<string, unknown> }).resize)
        .filter((value): value is Record<string, unknown> => Boolean(value));
    }
    function mountLivePreview() {
      allocated.openElement = true;
      vi.stubGlobal('getComputedStyle', () => ({
        getPropertyValue: () => '#000',
        paddingLeft: '0px',
        paddingRight: '0px',
        paddingTop: '0px',
        paddingBottom: '0px',
      }));
      const runFrames = captureFrames(),
        { viewport } = previewElement();
      Object.assign(viewport as unknown as Record<string, unknown>, { clientWidth: 1280, clientHeight: 768 });
      const dispose = mountTerminalViewportRuntime(viewport, { url: 'ws://lan/terminal', viewerId: 'close-preview' }),
        terminal = allocated.terminals[0],
        sent: string[] = [];
      giveScreen(terminal);
      const socket = sockets[0] as unknown as EventTarget & { readyState: number; send: (value: string) => void };
      socket.send = (value: string) => sent.push(value);
      socket.readyState = 1;
      const size = async (cols: number, rows: number) => {
        socket.dispatchEvent(
          new MessageEvent('message', { data: JSON.stringify({ pty_size: { cols, rows }, driven_by: 'drawer' }) }),
        );
        await Promise.resolve();
      };
      return { viewport, terminal, socket, sent, runFrames, size, dispose };
    }

    it('renders the 80×24 fallback at the dashboard font, scaled to fill the canvas, before a PTY size arrives', () => {
      const { viewport, terminal, socket, sent, runFrames, dispose } = mountLivePreview();
      // The 177-column 12px fit is gone: the grid starts at the dashboard tile's 80×24 and font.
      expect(terminal.options).toMatchObject({ cols: 80, rows: 24, fontSize: 24 });
      expect(viewport.dataset.fontSize).toBe('24');
      expect(viewport.dataset.geometryReady).toBe('false');
      socket.dispatchEvent(new Event('open'));
      runFrames();
      expect(terminal.resize).toHaveBeenLastCalledWith(80, 24);
      const style = (terminal as unknown as { element: { style: Record<string, string> } }).element.style;
      expect(style.transform).toBe(`scale(${canvasFill})`);
      expect(style.transformOrigin).toBe('top left');
      expect(viewport.dataset.geometryReady).toBe('true');
      // The canvas still takes the frame's preview scale on top of the grid fill (HS2-S7E53Q).
      expect(viewport.style.transform).toBe(`scale(${mirrorFrameScale(318, 240)})`);
      expect(viewport.dataset.gridSize).toBe('80x24');
      // Its heartbeat claim is non-focus and non-interacting, so it cannot drive the PTY.
      expect(claims(sent).at(-1)).toMatchObject({ cols: 80, rows: 24, focus: false, interacting: false });
      dispose();
    });

    it('adopts each reported PTY size, keeps filling the canvas, and never claims sizing (HS2-6C0WZN)', async () => {
      const { viewport, terminal, socket, sent, runFrames, size, dispose } = mountLivePreview();
      socket.dispatchEvent(new Event('open'));
      runFrames();
      // A wider borrowed PTY: the local grid matches it, so its output renders without rewrapping.
      await size(132, 40);
      expect(terminal.resize).toHaveBeenLastCalledWith(132, 40);
      expect(viewport.dataset.gridSize).toBe('132x40');
      expect(viewport.dataset.ptySize).toBe('132x40');
      expect(viewport.dataset.driving).toBe('false');
      // A later layout pass keeps the mirrored grid instead of refitting 177 columns, and re-claims it.
      resize[0].callback();
      runFrames();
      expect(terminal.resize).toHaveBeenLastCalledWith(132, 40);
      expect(claims(sent).at(-1)).toMatchObject({ cols: 132, rows: 40, focus: false, interacting: false });
      // A repeated report is a no-op; a narrower PTY shrinks the grid back.
      const calls = terminal.resize.mock.calls.length;
      await size(132, 40);
      expect(terminal.resize.mock.calls.length).toBe(calls);
      await size(80, 24);
      expect(terminal.resize).toHaveBeenLastCalledWith(80, 24);
      resize[0].callback();
      runFrames();
      expect(viewport.dataset.gridSize).toBe('80x24');
      expect(viewport.style.transform).toBe(`scale(${mirrorFrameScale(318, 240)})`);
      // Across every transition the preview only ever reported the PTY's own size, without focus.
      expect(claims(sent).every((claim) => claim.focus === false && claim.interacting === false)).toBe(true);
      expect(viewport.dataset.sizingFocus).toBe('false');
      dispose();
    });

    it('publishes the mirrored grid aspect on the preview container, live and static (HS2-RBS46R)', async () => {
      const attachPreview = (viewport: HTMLElement) => {
        const container = { style: { setProperty: vi.fn() } },
          preview = { dataset: {} as Record<string, string>, parentElement: container };
        (viewport as unknown as { closest: (selector: string) => unknown }).closest = (selector: string) =>
          selector === '[data-component="terminal-preview"]' ? preview : null;
        return { preview, container };
      };
      const live = mountLivePreview(),
        { preview, container } = attachPreview(live.viewport);
      live.socket.dispatchEvent(new Event('open'));
      live.runFrames();
      // The 1000×600 mocked screen publishes its own 5:3 aspect, once per distinct value.
      expect(preview.dataset.gridAspect).toBe('1.6667');
      expect(container.style.setProperty).toHaveBeenCalledWith('--terminal-preview-grid-aspect', '1.6667');
      const published = container.style.setProperty.mock.calls.length;
      resize[0].callback();
      live.runFrames();
      expect(container.style.setProperty.mock.calls.length).toBe(published);
      // A differently shaped mirrored grid republishes its new aspect.
      Object.assign((live.terminal as unknown as { element: Record<string, unknown> }).element, {
        querySelector: () => ({ offsetWidth: 721, offsetHeight: 450 }),
      });
      await live.size(80, 24);
      resize[0].callback();
      live.runFrames();
      expect(preview.dataset.gridAspect).toBe('1.6022');
      expect(container.style.setProperty).toHaveBeenLastCalledWith('--terminal-preview-grid-aspect', '1.6022');
      live.dispose();

      allocated.openElement = true;
      const runFrames = captureFrames(),
        { viewport } = previewElement(),
        demo = attachPreview(viewport),
        dispose = mountStaticTerminalViewportRuntime(viewport, { output: 'PASS\r\n' });
      giveScreen(allocated.terminals.at(-1)!);
      runFrames();
      expect(demo.preview.dataset.gridAspect).toBe('1.6667');
      expect(demo.container.style.setProperty).toHaveBeenCalledWith('--terminal-preview-grid-aspect', '1.6667');
      dispose();
    });

    it('renders a static (demo) TerminalPreview on the same 80×24 dashboard-font grid', () => {
      allocated.openElement = true;
      const runFrames = captureFrames(),
        { viewport } = previewElement(),
        dispose = mountStaticTerminalViewportRuntime(viewport, { output: 'PASS\r\n' }),
        terminal = allocated.terminals[0];
      giveScreen(terminal);
      runFrames();
      expect(terminal.options).toMatchObject({ cols: 80, rows: 24, fontSize: 24 });
      const style = (terminal as unknown as { element: { style: Record<string, string> } }).element.style;
      expect(style.transform).toBe(`scale(${canvasFill})`);
      expect(style.transformOrigin).toBe('top left');
      expect(viewport.style.transform).toBe(`scale(${mirrorFrameScale(318, 240)})`);
      expect(viewport.dataset.gridSize).toBe('80x24');
      expect(viewport.dataset.fontSize).toBe('24');
      dispose();
    });
  });
});
