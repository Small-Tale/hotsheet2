import { signal } from 'kerfjs';
import { Plus } from 'lucide';
import { describe, expect, it } from 'vitest';

import type { ConfirmationOptions } from '../confirmation';
import type { MinimizedDialogItem, MinimizedDialogKind } from '../minimized-dialogs';
import { createMinimizedDialogs } from './minimized-dialogs';

interface FakeKind extends MinimizedDialogKind {
  list: MinimizedDialogItem[];
  log: string[];
}

function fakeKind(
  kind: string,
  log: string[],
  list: Omit<MinimizedDialogItem, 'kind' | 'label' | 'shortLabel' | 'icon'>[],
): FakeKind {
  const items = list.map((entry) => ({
    ...entry,
    kind,
    label: entry.id,
    shortLabel: entry.id,
    icon: { icon: Plus, name: 'plus' },
  }));
  const fake: FakeKind = {
    kind,
    list: items,
    log,
    items: (tab) => fake.list.filter((entry) => entry.id.startsWith(`${tab}:`)),
    restore: (ids) => {
      log.push(`restore ${kind} ${ids.join(',')}`);
      fake.list = fake.list.filter((entry) => !ids.includes(entry.id));
      return Promise.resolve(ids.length > 0);
    },
    discard: (ids) => {
      log.push(`discard ${kind} ${ids.join(',')}`);
      fake.list = fake.list.filter((entry) => !ids.includes(entry.id));
    },
    dispose: (projectIds) => {
      log.push(`dispose ${kind} ${projectIds.join(',')}`);
    },
  };
  return fake;
}

function harness(options: { confirm?: boolean } = {}) {
  const log: string[] = [];
  const readers = fakeKind('ticket-reader', log, [
      { id: 'a:r1', minimizedAt: 1 },
      { id: 'a:r2', minimizedAt: 3, draft: 'Unsent note' },
      { id: 'b:r3', minimizedAt: 2 },
    ]),
    composer = fakeKind('new-ticket', log, [{ id: 'a:new', minimizedAt: 2, draft: 'Unsaved ticket' }]);
  const selectedProjectId = signal('a'),
    questions: ConfirmationOptions[] = [];
  let menus = 0;
  let onConfirm: (() => void) | undefined;
  const controller = createMinimizedDialogs({
    kinds: [readers, composer],
    selectedProjectId,
    confirm: (question) => {
      questions.push(question);
      onConfirm?.();
      return Promise.resolve(options.confirm ?? true);
    },
    openMinimizedMenu: () => {
      menus += 1;
    },
  });
  return {
    controller,
    readers,
    composer,
    selectedProjectId,
    questions,
    log,
    menus: () => menus,
    whileConfirming: (run: () => void) => {
      onConfirm = run;
    },
  };
}

describe('the minimized dialogs controller (HS2-3JR0ZH)', () => {
  it('lists every kind of the current tab oldest first', () => {
    const h = harness();
    expect(h.controller.items().map((item) => [item.kind, item.id])).toEqual([
      ['ticket-reader', 'a:r1'],
      ['new-ticket', 'a:new'],
      ['ticket-reader', 'a:r2'],
    ]);
    h.selectedProjectId.value = 'b';
    expect(h.controller.items().map((item) => item.id)).toEqual(['b:r3']);
  });

  it('routes Restore to the owning kind and ignores unknown or other-tab ids', async () => {
    const h = harness();
    expect(await h.controller.restore('a:new')).toBe(true);
    expect(await h.controller.restore('b:r3')).toBe(false);
    expect(await h.controller.restore('missing')).toBe(false);
    expect(h.log).toEqual(['restore new-ticket a:new']);
  });

  it('restores all kinds, the kind with the newest item last', async () => {
    const h = harness();
    expect(await h.controller.restoreAll()).toBe(true);
    expect(h.log).toEqual(['restore new-ticket a:new', 'restore ticket-reader a:r1,a:r2']);
    expect(h.controller.items()).toEqual([]);
    expect(await h.controller.restoreAll()).toBe(false);
  });

  it('asks before Close All discards drafts, then discards each kind', async () => {
    const h = harness();
    expect(await h.controller.closeAll()).toBe(true);
    expect(h.questions).toHaveLength(1);
    expect(h.questions[0]).toMatchObject({
      title: 'Close 3 minimized tickets?',
      message: '2 of them have unsent text that will be discarded.',
    });
    expect(h.log).toEqual(['discard ticket-reader a:r1,a:r2', 'discard new-ticket a:new']);
    expect(await h.controller.closeAll()).toBe(false);
  });

  it('keeps everything when Close All is cancelled', async () => {
    const h = harness({ confirm: false });
    expect(await h.controller.closeAll()).toBe(false);
    expect(h.log).toEqual([]);
    expect(h.controller.items()).toHaveLength(3);
  });

  it('closes without asking when nothing holds a draft', async () => {
    const h = harness();
    h.selectedProjectId.value = 'b';
    expect(await h.controller.closeAll()).toBe(true);
    expect(h.questions).toEqual([]);
    expect(h.log).toEqual(['discard ticket-reader b:r3']);
  });

  it('leaves an item minimized while the question was open', async () => {
    const h = harness();
    h.whileConfirming(() => {
      h.composer.list = [
        ...h.composer.list,
        {
          id: 'a:late',
          kind: 'new-ticket',
          label: 'late',
          shortLabel: 'late',
          icon: { icon: Plus, name: 'plus' },
          minimizedAt: 9,
        },
      ];
    });
    await h.controller.closeAll();
    expect(h.controller.items().map((item) => item.id)).toEqual(['a:late']);
  });

  it('restores the only item from the keyboard, or opens the menu for several', async () => {
    const h = harness();
    h.controller.restoreOrList();
    expect(h.menus()).toBe(1);
    h.selectedProjectId.value = 'b';
    h.controller.restoreOrList();
    await Promise.resolve();
    expect(h.log).toEqual(['restore ticket-reader b:r3']);
    h.controller.restoreOrList();
    expect(h.menus()).toBe(1);
  });

  it('disposes closed project tabs in every kind', () => {
    const h = harness();
    h.controller.disposeProjects(['a']);
    expect(h.log).toEqual(['dispose ticket-reader a', 'dispose new-ticket a']);
  });
});
