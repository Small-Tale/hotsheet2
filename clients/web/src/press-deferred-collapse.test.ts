import { describe, expect, it } from 'vitest';

import { createPressDeferredCollapse } from './press-deferred-collapse';

function harness() {
  const listeners = new Map<string, () => void>(),
    scheduled: Array<() => void> = [],
    state = { focused: true, eligible: true, collapsed: 0 };
  const doc = {
    addEventListener: (type: string, listener: () => void) => listeners.set(type, listener),
    removeEventListener: (type: string) => listeners.delete(type),
  } as unknown as Pick<Document, 'addEventListener' | 'removeEventListener'>;
  const guard = createPressDeferredCollapse({
    doc,
    ownsFocus: () => state.focused,
    shouldCollapse: () => state.eligible,
    collapse: () => {
      state.collapsed += 1;
    },
    schedule: (callback) => scheduled.push(callback),
  });
  const fire = (type: string) => listeners.get(type)?.();
  const flush = () => {
    for (const task of scheduled.splice(0)) task();
  };
  return { guard, fire, flush, state, listeners, scheduled };
}

describe('press-deferred collapse (HS2-YVBGW3)', () => {
  it('keeps the field open during a press and collapses after the press when focus left', () => {
    const h = harness();
    h.fire('pointerdown');
    expect(h.guard.pressing()).toBe(true);
    h.state.focused = false; // mousedown moved focus to the pressed tab
    expect(h.state.collapsed).toBe(0);
    h.fire('pointerup');
    expect(h.guard.pressing()).toBe(false);
    expect(h.state.collapsed).toBe(0); // the click is dispatched before the scheduled task
    h.flush();
    expect(h.state.collapsed).toBe(1);
  });

  it('does not collapse when focus stayed in the field or the field became ineligible', () => {
    const h = harness();
    h.fire('pointerdown');
    h.fire('pointerup');
    h.flush();
    h.fire('pointerdown');
    h.state.focused = false;
    h.state.eligible = false; // the user typed a query, or the field was already closed
    h.fire('pointerup');
    h.flush();
    expect(h.state.collapsed).toBe(0);
  });

  it('ignores presses that started while the field was not focused (blur-only semantics)', () => {
    const h = harness();
    h.state.focused = false;
    h.fire('pointerdown');
    h.fire('pointerup');
    h.flush();
    expect(h.state.collapsed).toBe(0);
    expect(h.scheduled).toHaveLength(0);
  });

  it('treats pointercancel as the end of the press and ignores a stray pointerup', () => {
    const h = harness();
    h.fire('pointerup');
    expect(h.scheduled).toHaveLength(0);
    h.fire('pointerdown');
    h.state.focused = false;
    h.fire('pointercancel');
    h.flush();
    expect(h.state.collapsed).toBe(1);
    h.fire('pointerup');
    h.flush();
    expect(h.state.collapsed).toBe(1);
  });

  it('skips the deferred collapse when a new press began or focus returned before it ran', () => {
    const h = harness();
    h.fire('pointerdown');
    h.state.focused = false;
    h.fire('pointerup');
    h.fire('pointerdown'); // a quick second press before the task runs
    h.flush();
    expect(h.state.collapsed).toBe(0);
    h.fire('pointerup'); // focus was already outside at this press start: no collapse scheduled
    h.flush();
    expect(h.state.collapsed).toBe(0);
    h.state.focused = true;
    h.fire('pointerdown');
    h.state.focused = false;
    h.fire('pointerup');
    h.state.focused = true; // focus came back (the user re-entered the field)
    h.flush();
    expect(h.state.collapsed).toBe(0);
  });

  it('removes every listener on dispose', () => {
    const h = harness();
    expect([...h.listeners.keys()].sort()).toEqual(['pointercancel', 'pointerdown', 'pointerup']);
    h.guard.dispose();
    expect(h.listeners.size).toBe(0);
  });
});
