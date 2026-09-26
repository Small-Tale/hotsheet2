import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MOBILE_TERMINAL_COLUMNS_CHANGE_EVENT } from './mobile-terminal-columns';
import { TERMINAL_DRAWER_RESIZE_END_EVENT } from './terminal-viewport';
import { mountStaticTerminalViewportRuntime, mountTerminalViewportRuntime } from './terminal-viewport-runtime';

const allocated = vi.hoisted(() => ({
  proposed: { cols: 80, rows: 24 },
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
    constructor() {
      allocated.terminals.push(this);
    }
    open() {}
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
    dispose() {}
    proposeDimensions() {
      return allocated.proposed;
    }
  },
}));
vi.mock('@xterm/addon-webgl', () => ({
  WebglAddon: class {
    dispose() {}
  },
}));

const resize: Array<{ disconnect: ReturnType<typeof vi.fn> }> = [],
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
  allocated.terminals.length = 0;
  allocated.proposed = { cols: 80, rows: 24 };
  resize.length = 0;
  intersections.length = 0;
  sockets.length = 0;
  scheduledTimeouts.length = 0;
  throwSocket = false;
  throwObservation = false;
  vi.stubGlobal('getComputedStyle', () => ({ getPropertyValue: () => '#000' }));
  vi.clearAllMocks();
  vi.stubGlobal('window', windowMock);
  vi.stubGlobal(
    'ResizeObserver',
    class {
      disconnect = vi.fn();
      constructor() {
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
  return { viewport: mock as unknown as HTMLElement, removed: mock.removeEventListener };
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
    expect(removed).toHaveBeenCalledTimes(5);
    throwSocket = false;
    const dispose = mountTerminalViewportRuntime(viewport, { url: 'ws://lan/terminal', viewerId: 'two' });
    dispose();
    dispose();
    expect(allocated.terminals[1].dispose).toHaveBeenCalledTimes(1);
    expect(resize[1].disconnect).toHaveBeenCalledTimes(1);
    expect(intersections[1].disconnect).toHaveBeenCalledTimes(1);
    expect(sockets[0].close).toHaveBeenCalledTimes(1);
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
