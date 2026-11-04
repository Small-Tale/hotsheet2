import { describe, expect, it, vi } from 'vitest';

import { ConfirmationDialog } from './components/confirmation-dialog';
import { createConfirmation, type FocusReturnTarget } from './confirmation';
import { wireConfirmation } from './confirmation-interactions';
import { CONFIRMATION_ACTIONS, CONFIRMATION_TARGETS } from './interaction-attrs/confirmation';

interface Registration {
  event: string;
  selector: string;
  capture: boolean;
  handle: (event: Event, target: Element) => void;
  attached: boolean;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => {
  const register = (registration: Omit<Registration, 'attached'>) => {
    const entry = { ...registration, attached: true };
    registrations.push(entry);
    return () => {
      entry.attached = false;
    };
  };
  return {
    ...(await original<typeof import('kerfjs')>()),
    delegate: (_root: unknown, event: string, selector: string, handle: Registration['handle']) =>
      register({ event, selector, handle, capture: false }),
    delegateCapture: (_root: unknown, event: string, selector: string, handle: Registration['handle']) =>
      register({ event, selector, handle, capture: true }),
  };
});

function opener(): FocusReturnTarget & { focused: number } {
  return {
    isConnected: true,
    focused: 0,
    focus() {
      this.focused += 1;
    },
  };
}

/** Collect the deferred focus returns so a test can run them as the closed dialog leaves the page. */
function deferredFocus() {
  const pending: (() => void)[] = [];
  return {
    afterClose: (run: () => void) => pending.push(run),
    flush: () => {
      for (const run of pending.splice(0)) run();
    },
  };
}

/** A fake element whose data-confirmation-id names the request it was rendered for. */
const element = (id: number) => ({ dataset: { confirmationId: String(id) } }) as unknown as Element;

describe('shared in-app confirmation (HS2-6S1346)', () => {
  it('resolves true on confirm and false on cancel, then returns focus to the opener', async () => {
    const button = opener(),
      frames = deferredFocus();
    const confirmation = createConfirmation({ focused: () => button, afterClose: frames.afterClose });
    expect(confirmation.current.value).toBeUndefined();
    const accepted = confirmation.request({ title: 'Run checks?', confirmLabel: 'Run Command' });
    expect(confirmation.current.value).toMatchObject({ title: 'Run checks?', confirmLabel: 'Run Command' });
    confirmation.confirm();
    await expect(accepted).resolves.toBe(true);
    expect(confirmation.current.value).toBeUndefined();
    expect(button.focused).toBe(0);
    frames.flush();
    expect(button.focused).toBe(1);

    const declined = confirmation.request({ title: 'Stop?', confirmLabel: 'Stop Turn', tone: 'danger' });
    confirmation.cancel();
    await expect(declined).resolves.toBe(false);
    frames.flush();
    expect(button.focused).toBe(2);
    // An answer with nothing on screen does nothing.
    confirmation.confirm();
    confirmation.cancel();
    frames.flush();
    expect(button.focused).toBe(2);
  });

  it('shows concurrent requests one at a time in order and settles each on its own answer', async () => {
    const button = opener(),
      frames = deferredFocus();
    let focusedNow: FocusReturnTarget | undefined = button;
    const confirmation = createConfirmation({ focused: () => focusedNow, afterClose: frames.afterClose });
    const first = confirmation.request({ title: 'First?', confirmLabel: 'One' });
    // The second request is made while the first dialog has focus; it inherits the first opener.
    focusedNow = { isConnected: true, focus: () => undefined };
    const second = confirmation.request({ title: 'Second?', confirmLabel: 'Two' });
    const firstId = confirmation.current.value?.id;
    expect(confirmation.current.value?.title).toBe('First?');
    confirmation.cancel();
    await expect(first).resolves.toBe(false);
    expect(confirmation.current.value?.title).toBe('Second?');
    expect(confirmation.current.value?.id).not.toBe(firstId);
    // A repeated click on the first dialog's button cannot answer the second request.
    confirmation.confirm(firstId);
    expect(confirmation.current.value?.title).toBe('Second?');
    frames.flush();
    expect(button.focused).toBe(0);
    confirmation.confirm(confirmation.current.value?.id);
    await expect(second).resolves.toBe(true);
    frames.flush();
    expect(button.focused).toBe(1);
  });

  it('returns focus to where the asking control settled focus in the same task (a Select trigger)', async () => {
    const option = { ...opener(), isConnected: true },
      trigger = opener(),
      frames = deferredFocus();
    let focusedNow: FocusReturnTarget = option;
    const confirmation = createConfirmation({ focused: () => focusedNow, afterClose: frames.afterClose });
    const answer = confirmation.request({ title: 'Use Bypass permissions?', confirmLabel: 'Use' });
    // The Select closes and focuses its trigger after reporting the change, before microtasks run.
    focusedNow = trigger;
    await Promise.resolve();
    confirmation.cancel();
    await expect(answer).resolves.toBe(false);
    frames.flush();
    expect(trigger.focused).toBe(1);
    expect(option.focused).toBe(0);
  });

  it('skips focus return when the opener left the page or a new request opened, and cancels all', async () => {
    const gone = { ...opener(), isConnected: false },
      frames = deferredFocus();
    const confirmation = createConfirmation({ focused: () => gone, afterClose: frames.afterClose });
    const first = confirmation.request({ title: 'A?', confirmLabel: 'A' });
    confirmation.confirm();
    // A request made before the closed dialog leaves keeps focus in its own dialog.
    const next = confirmation.request({ title: 'B?', confirmLabel: 'B' });
    const queued = confirmation.request({ title: 'C?', confirmLabel: 'C' });
    frames.flush();
    expect(gone.focused).toBe(0);
    await expect(first).resolves.toBe(true);
    confirmation.cancelAll();
    await expect(next).resolves.toBe(false);
    await expect(queued).resolves.toBe(false);
    expect(confirmation.current.value).toBeUndefined();
    frames.flush();
    expect(gone.focused).toBe(0);
  });

  it('answers from the dialog: the confirm and Cancel buttons, and Escape or the close button cancel', async () => {
    registrations.length = 0;
    const confirmation = createConfirmation({ focused: () => undefined });
    const dispose = wireConfirmation({} as HTMLElement, confirmation);
    const handler = (event: string, selector: string) =>
      registrations.find((item) => item.event === event && item.selector === selector && item.attached)!;
    const confirm = handler('click', CONFIRMATION_ACTIONS.confirm.selector),
      cancel = handler('click', CONFIRMATION_ACTIONS.cancel.selector),
      close = handler('kui-request-close', CONFIRMATION_TARGETS.dialog.selector);
    expect(close.capture).toBe(true);

    const confirmed = confirmation.request({ title: 'Run?', confirmLabel: 'Run' });
    confirm.handle({} as Event, element(confirmation.current.value!.id));
    await expect(confirmed).resolves.toBe(true);

    const cancelled = confirmation.request({ title: 'Run?', confirmLabel: 'Run' });
    cancel.handle({} as Event, element(confirmation.current.value!.id));
    await expect(cancelled).resolves.toBe(false);

    const escaped = confirmation.request({ title: 'Run?', confirmLabel: 'Run' });
    const dialog = element(confirmation.current.value!.id);
    // A close request bubbling from a nested surface is not this dialog's.
    close.handle({ target: {} } as unknown as Event, dialog);
    expect(confirmation.current.value).toBeDefined();
    close.handle({ target: dialog } as unknown as Event, dialog);
    await expect(escaped).resolves.toBe(false);

    dispose();
    expect(registrations.every((item) => !item.attached)).toBe(true);
  });

  it('renders a Kerf alert dialog with Cancel leading and the confirming action trailing', () => {
    expect(ConfirmationDialog({})).toBeNull();
    const markup = String(
      ConfirmationDialog({
        request: {
          id: 7,
          title: 'Delete the old Hot Sheet 1 files?',
          message: 'Backups are kept.',
          confirmLabel: 'Delete Old Files',
          tone: 'danger',
        },
      }),
    );
    expect(markup).toContain('role="alertdialog"');
    expect(markup).toContain('aria-describedby="confirmation-dialog-message"');
    expect(markup).toContain('data-confirmation-dialog="true"');
    expect(markup).toContain('data-confirmation-id="7"');
    expect(markup).toContain('data-controlled-open="true"');
    expect(markup).toContain('Backups are kept.');
    const cancelAt = markup.indexOf('data-action="cancel-confirmation"'),
      confirmAt = markup.indexOf('data-action="confirm-confirmation"');
    expect(cancelAt).toBeGreaterThan(-1);
    expect(confirmAt).toBeGreaterThan(cancelAt);
    expect(markup).toContain('>Cancel<');
    expect(markup).toContain('Delete Old Files');
    const plain = String(
      ConfirmationDialog({ request: { id: 8, title: 'Run checks?', confirmLabel: 'Run', cancelLabel: 'Keep' } }),
    );
    expect(plain).not.toContain('aria-describedby');
    expect(plain).toContain('Keep');
  });
});
