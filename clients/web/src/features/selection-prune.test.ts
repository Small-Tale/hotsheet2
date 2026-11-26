import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createFocusLossAutosave } from '../debounced-autosave';
import { pruneSelectionSavingEdits } from './selection-prune';

// HS2-41KEQD: a bulk change that moves the open ticket out of the view saves its unsaved edit first,
// against that ticket, instead of cancelling it; with nothing pending the prune is unchanged.
function harness({ visible, selected = ['HS2-A'] }: { visible: string[]; selected?: string[] }) {
  const state = {
    selected: [...selected],
    open: selected.length === 1 ? selected[0] : (undefined as string | undefined),
    visible,
    current: true,
  };
  const writes: Array<{ slug: string | undefined; value: string }> = [];
  const pendingSaves: Array<(saved: boolean) => void> = [];
  let holdSaves = false,
    failSaves = false;
  const autosave = createFocusLossAutosave((value: string) => {
    // Like updateSelectedTracked, the save resolves its ticket when it starts.
    writes.push({ slug: state.open, value });
    if (holdSaves) return new Promise<boolean>((resolve) => pendingSaves.push(resolve));
    return Promise.resolve(state.open !== undefined && !failSaves);
  });
  const applied: string[][] = [];
  const dependencies = {
    visibleSelection: () => state.selected.filter((slug) => state.visible.includes(slug)),
    selectedSlugs: () => state.selected,
    selectedTicketSlug: () => state.open,
    flushSelectedTicketEdits: vi.fn(() => (autosave.pending() ? autosave.flush() : undefined)),
    stillCurrent: () => state.current,
    apply: (next: readonly string[]) => {
      applied.push([...next]);
      state.selected = [...next];
      if (state.open !== undefined && !next.includes(state.open)) {
        autosave.cancel();
        state.open = undefined;
      }
      if (next.length !== 1) state.open = undefined;
    },
  };
  return {
    state,
    writes,
    applied,
    autosave,
    dependencies,
    prune: () => {
      pruneSelectionSavingEdits(dependencies);
    },
    hold: () => {
      holdSaves = true;
    },
    fail: () => {
      failSaves = true;
    },
    settle: (saved = true) => {
      pendingSaves.shift()?.(saved);
    },
  };
}

describe('pruneSelectionSavingEdits (HS2-41KEQD)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('saves a pending edit to the ticket that is leaving the view, then prunes it', async () => {
    const h = harness({ visible: [] });
    h.autosave.schedule('typed before the bulk change');
    h.prune();
    // The save started while the ticket was still selected, so it went to that ticket.
    expect(h.writes).toEqual([{ slug: 'HS2-A', value: 'typed before the bulk change' }]);
    expect(h.applied).toEqual([]);
    expect(h.state.open).toBe('HS2-A');
    await vi.runAllTimersAsync();
    expect(h.applied).toEqual([[]]);
    expect(h.state.open).toBeUndefined();
    // A later focus-loss flush (the editor unmounting) has nothing left to write.
    await h.autosave.flush();
    expect(h.writes).toHaveLength(1);
  });

  it('prunes straight away, without a write, when no edit is pending', () => {
    const h = harness({ visible: [] });
    h.prune();
    expect(h.applied).toEqual([[]]);
    expect(h.state.open).toBeUndefined();
    expect(h.writes).toEqual([]);
  });

  it('does nothing when every selected row is still shown', () => {
    const h = harness({ visible: ['HS2-A'] });
    h.autosave.schedule('still typing');
    h.prune();
    expect(h.dependencies.flushSelectedTicketEdits).not.toHaveBeenCalled();
    expect(h.applied).toEqual([]);
    expect(h.writes).toEqual([]);
    expect(h.autosave.pending()).toBe(true);
  });

  it('never writes mid-typing when the open ticket stays in view and other rows leave', () => {
    const h = harness({ visible: ['HS2-A'], selected: ['HS2-A', 'HS2-B'] });
    h.state.open = 'HS2-A';
    h.autosave.schedule('still typing');
    h.prune();
    expect(h.dependencies.flushSelectedTicketEdits).not.toHaveBeenCalled();
    expect(h.applied).toEqual([['HS2-A']]);
    expect(h.state.open).toBe('HS2-A');
    expect(h.writes).toEqual([]);
  });

  it('waits for a save in flight and a later edit, both to the same ticket, before pruning', async () => {
    const h = harness({ visible: [] });
    h.hold();
    h.autosave.schedule('first');
    void h.autosave.flush();
    h.autosave.schedule('first and second');
    h.prune();
    expect(h.applied).toEqual([]);
    // Repeated prunes while waiting (a second bulk response) do not deselect early.
    h.prune();
    expect(h.applied).toEqual([]);
    h.settle();
    await vi.advanceTimersByTimeAsync(0);
    expect(h.writes.map((write) => write.slug)).toEqual(['HS2-A', 'HS2-A']);
    expect(h.applied).toEqual([]);
    h.settle();
    await vi.runAllTimersAsync();
    expect(h.writes).toEqual([
      { slug: 'HS2-A', value: 'first' },
      { slug: 'HS2-A', value: 'first and second' },
    ]);
    expect(h.applied.at(-1)).toEqual([]);
    expect(h.state.open).toBeUndefined();
  });

  it('saves an edit typed after an earlier save before the ticket leaves', async () => {
    const h = harness({ visible: ['HS2-A'] });
    h.autosave.schedule('saved on blur');
    await h.autosave.flush();
    h.autosave.schedule('typed again after the save');
    h.state.visible = [];
    h.prune();
    await vi.runAllTimersAsync();
    expect(h.writes.map((write) => write.value)).toEqual(['saved on blur', 'typed again after the save']);
    expect(h.writes.every((write) => write.slug === 'HS2-A')).toBe(true);
    expect(h.applied).toEqual([[]]);
  });

  it('still prunes after a failed save', async () => {
    const h = harness({ visible: [] });
    h.fail();
    h.autosave.schedule('rejected by the server');
    h.prune();
    await vi.runAllTimersAsync();
    expect(h.writes).toHaveLength(1);
    expect(h.applied).toEqual([[]]);
  });

  it('keeps the ticket selected when it is back in view by the time the save settles', async () => {
    const h = harness({ visible: [] });
    h.hold();
    h.autosave.schedule('typed');
    h.prune();
    h.state.visible = ['HS2-A'];
    h.settle();
    await vi.runAllTimersAsync();
    expect(h.applied).toEqual([]);
    expect(h.state.open).toBe('HS2-A');
    expect(h.writes).toHaveLength(1);
  });

  it('skips the deferred prune once the project is no longer active', async () => {
    const h = harness({ visible: [] });
    h.hold();
    h.autosave.schedule('typed');
    h.prune();
    h.state.current = false;
    h.settle();
    await vi.runAllTimersAsync();
    expect(h.writes).toHaveLength(1);
    expect(h.applied).toEqual([]);
  });
});
