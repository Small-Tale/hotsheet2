import { describe, expect, it, vi } from 'vitest';

import { createFocusLossAutosave } from './debounced-autosave';
import { installDesktopClose } from './desktop-close';
import { DESKTOP_CLOSE_COMMAND, DESKTOP_RECOVERY_FLUSH_EVENT, type DesktopCloseHost } from './desktop-close-contract';
import { listenForDesktopRecovery } from './desktop-recovery-flush';
import { captureTicketDraft } from './ticket-draft-store';

function fixture() {
  const events = new EventTarget(),
    invoke = vi.fn(async () => undefined),
    alert = vi.fn(),
    documentElement = { inert: false } as HTMLElement,
    host: DesktopCloseHost = {
      document: { documentElement },
      dispatchEvent: events.dispatchEvent.bind(events),
      __TAURI_INTERNALS__: { invoke },
      alert,
    };
  return { events, invoke, alert, documentElement, host };
}

describe('controlled native desktop close (HS2-E0H7S0)', () => {
  it('fails closed on quota and captures a newer edit behind an old server save before retry ACK', async () => {
    const { host, events, invoke, documentElement } = fixture(),
      copies: string[] = [];
    let fail = true,
      release!: (saved: boolean) => void;
    const store = {
        setItem: (_key: string, value: string) => {
          if (fail) throw new Error('Synthetic quota');
          copies.push(value);
        },
        removeItem: () => {},
      },
      save = vi.fn(
        () =>
          new Promise<boolean>((resolve) => {
            release = resolve;
          }),
      ),
      autosave = createFocusLossAutosave<string>(save, {
        prepareRecovery: (draft) => ({
          destination: 'hotsheet.ticket-draft:owned',
          capture: () => {
            captureTicketDraft(store, 'hotsheet.ticket-draft:owned', { base: 'base', draft });
          },
        }),
      });
    autosave.schedule('old');
    const old = autosave.flush();
    autosave.schedule('latest');
    listenForDesktopRecovery({ addEventListener: events.addEventListener.bind(events) }, () => {
      autosave.captureRecovery();
    });
    installDesktopClose({ flushForNativeClose: async () => undefined }, host);
    host.__HOTSHEET_DESKTOP_CLOSE__!('1');
    await vi.waitFor(() => {
      expect(invoke).toHaveBeenCalledWith(DESKTOP_CLOSE_COMMAND, { request: '1', success: false });
    });
    expect(documentElement.inert).toBe(false);
    expect(copies).toEqual([]);
    fail = false;
    host.__HOTSHEET_DESKTOP_CLOSE__!('2');
    await vi.waitFor(() => {
      expect(invoke).toHaveBeenCalledWith(DESKTOP_CLOSE_COMMAND, { request: '2', success: true });
    });
    expect(JSON.parse(copies[0] ?? '{}')).toMatchObject({ draft: 'latest' });
    expect(save).toHaveBeenCalledOnce();
    autosave.cancel();
    release(false);
    await old;
  });
  it('reports a timeout before listener readiness and fences late delivery from a new retry', async () => {
    const { host, invoke, alert, documentElement } = fixture(),
      flushForNativeClose = vi.fn(async () => undefined);
    installDesktopClose({ flushForNativeClose }, host);
    host.__HOTSHEET_DESKTOP_CLOSE_CANCEL__!('1');
    host.__HOTSHEET_DESKTOP_CLOSE_CANCEL__!('1');
    host.__HOTSHEET_DESKTOP_CLOSE__!('1');
    expect(alert).toHaveBeenCalledOnce();
    expect(flushForNativeClose).not.toHaveBeenCalled();
    expect(documentElement.inert).toBe(false);
    host.__HOTSHEET_DESKTOP_CLOSE__!('2');
    await vi.waitFor(() => {
      expect(invoke).toHaveBeenCalledWith(DESKTOP_CLOSE_COMMAND, { request: '2', success: true });
    });
  });
  it('refuses close when a parent recovery capture fails instead of swallowing a DOM listener error', async () => {
    const { host, events, invoke, alert } = fixture(),
      flushForNativeClose = vi.fn(async () => undefined);
    listenForDesktopRecovery({ addEventListener: events.addEventListener.bind(events) }, () => {
      throw new Error('synthetic capture failure');
    });
    installDesktopClose({ flushForNativeClose }, host);
    host.__HOTSHEET_DESKTOP_CLOSE__!('1');
    await vi.waitFor(() => {
      expect(invoke).toHaveBeenCalledWith(DESKTOP_CLOSE_COMMAND, { request: '1', success: false });
    });
    expect(flushForNativeClose).not.toHaveBeenCalled();
    expect(alert).toHaveBeenCalledOnce();
  });
  it('captures later parent recovery handlers before draining and acknowledges only after success', async () => {
    const { host, events, invoke, documentElement } = fixture(),
      order: string[] = [];
    let finish!: () => void;
    const flushForNativeClose = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    installDesktopClose({ flushForNativeClose }, host);
    events.addEventListener(DESKTOP_RECOVERY_FLUSH_EVENT, () => order.push('parent captured'));
    host.__HOTSHEET_DESKTOP_CLOSE__!('1');
    host.__HOTSHEET_DESKTOP_CLOSE__!('1');
    expect(order).toEqual(['parent captured']);
    expect(flushForNativeClose).toHaveBeenCalledTimes(1);
    expect(documentElement.inert).toBe(true);
    expect(invoke).not.toHaveBeenCalled();
    finish();
    await vi.waitFor(() => {
      expect(invoke).toHaveBeenCalledWith(DESKTOP_CLOSE_COMMAND, { request: '1', success: true });
    });
    expect(documentElement.inert).toBe(true); // native teardown, never user edits between ACK and exit
  });

  it('retains failed data, unlocks for retry, and rejects delayed old callbacks and acknowledgements', async () => {
    const { host, invoke, alert, documentElement } = fixture();
    let resolveOld!: () => void;
    const flushForNativeClose = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise<void>((resolve) => {
            resolveOld = resolve;
          }),
      )
      .mockRejectedValueOnce(new Error('synthetic failure'))
      .mockResolvedValue(undefined);
    installDesktopClose({ flushForNativeClose }, host);
    host.__HOTSHEET_DESKTOP_CLOSE__!('1');
    host.__HOTSHEET_DESKTOP_CLOSE_CANCEL__!('1');
    expect(documentElement.inert).toBe(false);
    expect(alert).toHaveBeenCalledTimes(1);
    host.__HOTSHEET_DESKTOP_CLOSE__!('2');
    host.__HOTSHEET_DESKTOP_CLOSE_CANCEL__!('1');
    host.__HOTSHEET_DESKTOP_CLOSE__!('1');
    resolveOld();
    await vi.waitFor(() => {
      expect(invoke).toHaveBeenCalledWith(DESKTOP_CLOSE_COMMAND, { request: '2', success: false });
    });
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(documentElement.inert).toBe(false);
    host.__HOTSHEET_DESKTOP_CLOSE__!('3');
    await vi.waitFor(() => {
      expect(invoke).toHaveBeenCalledWith(DESKTOP_CLOSE_COMMAND, { request: '3', success: true });
    });
    expect(alert).toHaveBeenCalledTimes(2);
  });

  it('never accepts a missing or failed IPC transport as successful shutdown', async () => {
    for (const missing of [true, false]) {
      const { host, invoke, alert, documentElement } = fixture();
      if (missing) delete host.__TAURI_INTERNALS__;
      else invoke.mockRejectedValueOnce(new Error('synthetic IPC refusal'));
      installDesktopClose({ flushForNativeClose: async () => undefined }, host);
      host.__HOTSHEET_DESKTOP_CLOSE__!('1');
      await vi.waitFor(() => {
        expect(alert).toHaveBeenCalledOnce();
      });
      expect(documentElement.inert).toBe(false);
    }
  });
});
