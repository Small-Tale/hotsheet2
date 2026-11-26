import { signal } from 'kerfjs';
import { describe, expect, it } from 'vitest';

import { loadMinimizedComposers, MINIMIZED_COMPOSERS_STORAGE_KEY, type MinimizedComposer } from '../minimized-composer';
import { createMinimizedComposer, MINIMIZED_COMPOSER_KIND } from './minimized-composer';

function memoryStorage() {
  const values = new Map<string, string>();
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
  } as unknown as Storage & { values: Map<string, string> };
}

/**
 * A fake composer whose draft lives in signals, as the workflow's do: opening only expands it, and
 * discarding resets the draft. The project session is a per-project snapshot of that draft.
 */
function harness(
  options: {
    minimized?: MinimizedComposer[];
    storage?: Storage & { values: Map<string, string> };
    composerHidden?: () => Promise<void>;
  } = {},
) {
  const storage = options.storage ?? memoryStorage();
  const minimizedComposers = signal<MinimizedComposer[]>(options.minimized ?? loadMinimizedComposers(storage)),
    selectedProjectId = signal('alpha'),
    composerExpanded = signal(false),
    title = signal(''),
    details = signal(''),
    attachments = signal<string[]>([]),
    submitting = signal(false);
  const calls = {
    opened: 0,
    discarded: 0,
    sessions: 0,
    flushed: 0,
    minimizeMotion: 0,
    restoreMotion: 0,
    released: 0,
    focused: 0,
  };
  let now = 100;
  const controller = createMinimizedComposer({
    minimizedComposers,
    selectedProjectId,
    composerExpanded,
    submitting: () => submitting.value,
    draft: () => ({ title: title.value, details: details.value, attachmentCount: attachments.value.length }),
    flushDraftInputs: () => {
      calls.flushed += 1;
    },
    openComposer: () => {
      calls.opened += 1;
      composerExpanded.value = true;
      controller.opened(selectedProjectId.value);
    },
    discardComposer: () => {
      calls.discarded += 1;
      composerExpanded.value = false;
      title.value = '';
      details.value = '';
      attachments.value = [];
    },
    persistSession: () => {
      calls.sessions += 1;
    },
    composerPanel: () => null,
    composerHidden: options.composerHidden,
    storage: () => storage,
    now: () => ++now,
    animate: {
      minimize: () => {
        calls.minimizeMotion += 1;
        return Promise.resolve();
      },
      restore: () => {
        calls.restoreMotion += 1;
        return Promise.resolve(() => {
          calls.released += 1;
        });
      },
    },
    afterOpen: (run) => {
      run();
    },
    focusControl: () => {
      calls.focused += 1;
    },
  });
  const open = () => {
    composerExpanded.value = true;
    controller.opened(selectedProjectId.value);
  };
  return {
    controller,
    minimizedComposers,
    selectedProjectId,
    composerExpanded,
    title,
    details,
    attachments,
    submitting,
    storage,
    calls,
    open,
  };
}

describe('minimizing the new-ticket composer (HS2-3JR0ZH)', () => {
  it('hides the open composer, keeps its draft, lists it for this tab, and persists the record', async () => {
    const h = harness();
    h.open();
    h.title.value = 'Draft a release note';
    h.details.value = 'Steps';
    h.attachments.value = ['file-1'];
    expect(await h.controller.minimize()).toBe(true);
    expect(h.composerExpanded.value).toBe(false);
    expect(h.title.value).toBe('Draft a release note');
    expect(h.details.value).toBe('Steps');
    expect(h.attachments.value).toEqual(['file-1']);
    expect(h.calls).toMatchObject({ flushed: 1, sessions: 1, minimizeMotion: 1, focused: 1, discarded: 0 });
    expect(h.controller.isMinimized()).toBe(true);
    expect(h.controller.items('alpha')).toEqual([
      {
        id: 'new-ticket:alpha',
        kind: MINIMIZED_COMPOSER_KIND,
        label: 'New ticket: Draft a release note',
        shortLabel: 'new ticket',
        icon: expect.objectContaining({ name: 'plus' }) as unknown,
        draft: 'Unsaved ticket',
        minimizedAt: 101,
      },
    ]);
    expect(h.controller.items('beta')).toEqual([]);
    expect(loadMinimizedComposers(h.storage).map((entry) => entry.id)).toEqual(['new-ticket:alpha']);
  });

  it('lists an empty composer as Untitled without a draft indicator', async () => {
    const h = harness();
    h.open();
    await h.controller.minimize();
    expect(h.controller.items('alpha')[0]).toMatchObject({ label: 'New ticket: Untitled', draft: undefined });
  });

  it('refuses while the composer is closed, submitting, or no tab is selected', async () => {
    const closed = harness();
    expect(await closed.controller.minimize()).toBe(false);
    const busy = harness();
    busy.open();
    busy.submitting.value = true;
    expect(await busy.controller.minimize()).toBe(false);
    expect(busy.composerExpanded.value).toBe(true);
    const noTab = harness();
    noTab.open();
    noTab.selectedProjectId.value = '';
    expect(await noTab.controller.minimize()).toBe(false);
    expect(closed.minimizedComposers.value).toEqual([]);
  });

  it('restores the same draft into the open composer and takes it off the list', async () => {
    const h = harness();
    h.open();
    h.title.value = 'Keep me';
    await h.controller.minimize();
    expect(await h.controller.restore(['new-ticket:alpha'])).toBe(true);
    expect(h.composerExpanded.value).toBe(true);
    expect(h.title.value).toBe('Keep me');
    expect(h.calls).toMatchObject({ opened: 1, restoreMotion: 1, released: 1 });
    expect(h.controller.items('alpha')).toEqual([]);
    expect(h.storage.values.has(MINIMIZED_COMPOSERS_STORAGE_KEY)).toBe(false);
    // A second restore has nothing left to open.
    expect(await h.controller.restore(['new-ticket:alpha'])).toBe(false);
  });

  it('restores only the current tab composer', async () => {
    const h = harness();
    h.open();
    await h.controller.minimize();
    h.selectedProjectId.value = 'beta';
    expect(await h.controller.restore(['new-ticket:alpha'])).toBe(false);
    expect(h.calls.opened).toBe(0);
    h.selectedProjectId.value = 'alpha';
    expect(await h.controller.restore(['new-ticket:alpha'])).toBe(true);
  });

  it('opening the composer any other way takes it off the list without discarding the draft', async () => {
    const h = harness();
    h.open();
    h.title.value = 'Via launcher';
    await h.controller.minimize();
    h.open();
    expect(h.controller.items('alpha')).toEqual([]);
    expect(h.title.value).toBe('Via launcher');
    h.controller.opened(undefined);
    h.controller.opened('beta');
    expect(h.calls.discarded).toBe(0);
  });

  it('closing discards the draft and staged files of the current tab', async () => {
    const h = harness();
    h.open();
    h.title.value = 'Throw away';
    h.attachments.value = ['file-1'];
    await h.controller.minimize();
    h.controller.discard(['new-ticket:alpha']);
    expect(h.calls.discarded).toBe(1);
    expect(h.title.value).toBe('');
    expect(h.attachments.value).toEqual([]);
    expect(h.controller.items('alpha')).toEqual([]);
    h.controller.discard(['new-ticket:alpha']);
    expect(h.calls.discarded).toBe(1);
  });

  it('drops a closed tab record without touching the current composer', async () => {
    const h = harness({
      minimized: [{ id: 'new-ticket:beta', tabProjectId: 'beta', title: 'B', hasDraft: true, minimizedAt: 1 }],
    });
    h.open();
    h.title.value = 'Alpha draft';
    await h.controller.minimize();
    h.controller.dispose(['beta']);
    expect(h.minimizedComposers.value.map((entry) => entry.tabProjectId)).toEqual(['alpha']);
    h.controller.dispose(['gamma']);
    expect(h.minimizedComposers.value).toHaveLength(1);
    h.controller.discard(['new-ticket:beta']);
    expect(h.calls.discarded).toBe(0);
    expect(h.title.value).toBe('Alpha draft');
  });

  it('keeps the minimized composer across a reload, then restores the session-restored draft', async () => {
    const first = harness();
    first.open();
    first.title.value = 'Survive reload';
    first.details.value = 'Body';
    await first.controller.minimize();
    // The reload reads the record from storage; the draft returns from the project session.
    const reloaded = harness({ storage: first.storage });
    reloaded.title.value = 'Survive reload';
    reloaded.details.value = 'Body';
    expect(reloaded.controller.items('alpha')).toMatchObject([
      { label: 'New ticket: Survive reload', draft: 'Unsaved ticket' },
    ]);
    expect(await reloaded.controller.restore(['new-ticket:alpha'])).toBe(true);
    expect(reloaded.composerExpanded.value).toBe(true);
    expect(reloaded.details.value).toBe('Body');
  });

  it('waits for a composer still closing from its minimize before reopening it', async () => {
    let hidden: () => void = () => undefined;
    const h = harness({
      composerHidden: () =>
        new Promise((resolve) => {
          hidden = resolve;
        }),
    });
    h.open();
    await h.controller.minimize();
    const restoring = h.controller.restore(['new-ticket:alpha']);
    await Promise.resolve();
    expect(h.calls.opened).toBe(0);
    hidden();
    expect(await restoring).toBe(true);
    expect(h.calls.opened).toBe(1);
  });
});
