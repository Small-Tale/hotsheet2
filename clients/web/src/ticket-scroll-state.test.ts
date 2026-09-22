import { describe, expect, it } from 'vitest';

import { captureTicketScrollState, restoreTicketScrollState, TicketScrollMemory } from './ticket-scroll-state';

function scroller(key = 'workspace', top = 0, left = 0) {
  return {
    dataset: { ticketScrollOwner: key },
    scrollTop: top,
    scrollLeft: left,
    scrollHeight: 2000,
    clientHeight: 200,
    scrollWidth: 1000,
    clientWidth: 200,
    scrollTo(nextLeft: number, nextTop: number) {
      this.scrollLeft = nextLeft;
      this.scrollTop = nextTop;
    },
  };
}

const rootFor = (...owners: ReturnType<typeof scroller>[]) =>
  ({ querySelectorAll: () => owners }) as unknown as ParentNode;

describe('ticket scroll state', () => {
  it('restores every named ticket scroll owner after its contents move', () => {
    const owners = [scroller('list', 420), scroller('column:started', 180, 12)];
    const root = rootFor(...owners);
    const state = captureTicketScrollState(root);
    for (const owner of owners) {
      owner.scrollTop = 0;
      owner.scrollLeft = 0;
    }
    restoreTicketScrollState(state, root);
    expect(owners.map((owner) => [owner.scrollLeft, owner.scrollTop])).toEqual([
      [0, 420],
      [12, 180],
    ]);
  });

  it('captures the root scroll owner and clamps both axes or resets an unseen owner', () => {
    const owner = scroller('workspace', 600, 400),
      child = scroller('column:started', 100, 20),
      root = Object.assign(owner, { matches: () => true, querySelectorAll: () => [child] }) as unknown as ParentNode,
      state = captureTicketScrollState(root);
    expect(state.size).toBe(2);
    owner.scrollHeight = 300;
    owner.scrollWidth = 250;
    child.dataset.ticketScrollOwner = 'new-column';
    restoreTicketScrollState(state, root);
    expect([owner.scrollTop, owner.scrollLeft]).toEqual([100, 50]);
    expect([child.scrollTop, child.scrollLeft]).toEqual([0, 0]);
    owner.scrollHeight = 0;
    owner.scrollWidth = 0;
    restoreTicketScrollState(state, root);
    expect([owner.scrollTop, owner.scrollLeft]).toEqual([0, 0]);
  });

  it('isolates projects, modes and views, then captures user edits after every restoration', () => {
    const memory = new TicketScrollMemory(),
      owner = scroller(),
      root = rootFor(owner),
      scopes = [
        { project: 'one', mode: 'list', view: 'queue' },
        { project: 'two', mode: 'list', view: 'queue' },
        { project: 'one', mode: 'board', view: 'queue' },
        { project: 'one', mode: 'list', view: 'backlog' },
      ];
    for (const [index, scope] of scopes.entries()) {
      memory.afterRender(memory.beforeRender(scope, root), root, true);
      expect(owner.scrollTop).toBe(0);
      owner.scrollTop = 100 * (index + 1);
    }
    for (const [index, scope] of scopes.entries()) {
      memory.afterRender(memory.beforeRender(scope, root), root, true);
      expect(owner.scrollTop).toBe(100 * (index + 1));
      owner.scrollTop += 25;
    }
    memory.afterRender(memory.beforeRender(scopes[0], root), root, true);
    expect(owner.scrollTop).toBe(125);
    // IDs containing separators must not alias another scope.
    memory.afterRender(memory.beforeRender({ project: 'one:list', mode: '', view: 'queue' }, root), root, true);
    expect(owner.scrollTop).toBe(0);
  });

  it('retains a destination through empty and partial loading before clamping changed content', () => {
    const memory = new TicketScrollMemory(),
      owner = scroller(),
      root = rootFor(owner),
      a = { project: 'one', mode: 'list', view: 'queue' },
      b = { ...a, view: 'archive' };
    memory.afterRender(memory.beforeRender(a, root), root, true);
    owner.scrollTop = 900;
    memory.afterRender(memory.beforeRender(b, root), root, true);
    let generation = memory.beforeRender(a, root);
    owner.scrollHeight = 0;
    memory.afterRender(generation, root, false);
    expect(owner.scrollTop).toBe(0);
    generation = memory.beforeRender(a, root);
    owner.scrollHeight = 600;
    memory.afterRender(generation, root, false);
    expect(owner.scrollTop).toBe(400);
    generation = memory.beforeRender(a, root);
    owner.scrollHeight = 2000;
    memory.afterRender(generation, root, true);
    expect(owner.scrollTop).toBe(900);
    memory.afterRender(memory.beforeRender(b, root), root, true);
    generation = memory.beforeRender(a, root);
    owner.scrollHeight = 500;
    memory.afterRender(generation, root, true);
    expect(owner.scrollTop).toBe(300);
    memory.afterRender(memory.beforeRender(b, root), root, true);
    owner.scrollHeight = 2000;
    memory.afterRender(memory.beforeRender(a, root), root, true);
    expect(owner.scrollTop).toBe(300);
  });

  it('ignores out-of-order restore callbacks and keeps each independent board column', () => {
    const memory = new TicketScrollMemory(),
      first = scroller('column:started'),
      second = scroller('column:completed'),
      root = rootFor(first, second),
      a = { project: 'one', mode: 'board', view: 'queue' },
      b = { ...a, project: 'two' };
    memory.afterRender(memory.beforeRender(a, root), root, true);
    first.scrollTop = 400;
    second.scrollTop = 175;
    const stale = memory.beforeRender(b, root);
    const current = memory.beforeRender(a, root);
    memory.afterRender(stale, root, true);
    expect([first.scrollTop, second.scrollTop]).toEqual([400, 175]);
    first.scrollTop = second.scrollTop = 0;
    memory.afterRender(current, root, true);
    expect([first.scrollTop, second.scrollTop]).toEqual([400, 175]);
    const rerender = memory.beforeRender(a, root);
    const coalesced = memory.beforeRender(a, root);
    first.scrollTop = second.scrollTop = 0;
    memory.afterRender(rerender, root, true);
    expect(first.scrollTop).toBe(0);
    memory.afterRender(coalesced, root, true);
    expect([first.scrollTop, second.scrollTop]).toEqual([400, 175]);
  });

  it('preserves user scrolling during pending progressive renders and repeated mutations', () => {
    for (const mode of ['list', 'board']) {
      const memory = new TicketScrollMemory(),
        owners = [scroller('workspace'), scroller('column:started'), scroller('column:completed')],
        root = rootFor(...owners),
        scope = { project: 'one', mode, view: 'queue' };
      memory.afterRender(memory.beforeRender(scope, root), root, false);
      for (const top of [260, 420, 180]) {
        owners.forEach((owner, index) => {
          owner.scrollTop = top + index * 30;
          owner.scrollLeft = index * 10;
        });
        const next = memory.beforeRender(scope, root);
        // A mutation morph can clamp or replace live scroll offsets before restoration.
        owners.forEach((owner) => {
          owner.scrollTop = owner.scrollLeft = 0;
        });
        memory.afterRender(next, root, false);
        expect(owners.map((owner) => [owner.scrollTop, owner.scrollLeft])).toEqual([
          [top, 0],
          [top + 30, 10],
          [top + 60, 20],
        ]);
      }
      memory.afterRender(memory.beforeRender(scope, root), root, true);
      expect(owners.map((owner) => owner.scrollTop)).toEqual([180, 210, 240]);
    }
  });

  it('keeps untouched destinations through shrinking, growth and empty refill while one axis is edited', () => {
    const memory = new TicketScrollMemory(),
      first = scroller('column:started'),
      second = scroller('column:completed'),
      root = rootFor(first, second),
      a = { project: 'one', mode: 'board', view: 'queue' },
      b = { ...a, project: 'two' };
    memory.afterRender(memory.beforeRender(a, root), root, true);
    first.scrollTop = second.scrollTop = 900;
    first.scrollLeft = second.scrollLeft = 700;
    memory.afterRender(memory.beforeRender(b, root), root, true);
    let generation = memory.beforeRender(a, root);
    first.scrollHeight = second.scrollHeight = 600;
    first.scrollWidth = second.scrollWidth = 600;
    memory.afterRender(generation, root, false);
    expect([first.scrollTop, first.scrollLeft, second.scrollTop]).toEqual([400, 400, 400]);
    first.scrollTop = 180;
    generation = memory.beforeRender(a, root);
    first.scrollHeight = second.scrollHeight = 2000;
    first.scrollWidth = second.scrollWidth = 1000;
    memory.afterRender(generation, root, false);
    expect([first.scrollTop, first.scrollLeft, second.scrollTop, second.scrollLeft]).toEqual([180, 700, 900, 700]);
    // A layout shrink between renders clamps actual positions, not the desired destination.
    for (const owner of [first, second]) {
      owner.scrollHeight = owner.scrollWidth = 0;
      owner.scrollTop = owner.scrollLeft = 0;
    }
    generation = memory.beforeRender(a, root);
    memory.afterRender(generation, root, false);
    generation = memory.beforeRender(a, root);
    first.scrollHeight = second.scrollHeight = 2000;
    first.scrollWidth = second.scrollWidth = 1000;
    memory.afterRender(generation, root, true);
    expect([first.scrollTop, first.scrollLeft, second.scrollTop, second.scrollLeft]).toEqual([180, 700, 900, 700]);
  });

  it('remembers edits made before settling when leaving and returning to a pending scope', () => {
    const memory = new TicketScrollMemory(),
      owner = scroller(),
      root = rootFor(owner),
      a = { project: 'one', mode: 'list', view: 'queue' },
      b = { ...a, view: 'backlog' };
    memory.afterRender(memory.beforeRender(a, root), root, false);
    owner.scrollTop = 350;
    const stale = memory.beforeRender(a, root);
    memory.afterRender(stale, root, false);
    owner.scrollTop = 480;
    memory.afterRender(memory.beforeRender(b, root), root, false);
    owner.scrollTop = 210;
    const returned = memory.beforeRender(a, root);
    memory.afterRender(stale, root, true);
    memory.afterRender(returned, root, true);
    expect(owner.scrollTop).toBe(480);
    memory.afterRender(memory.beforeRender(b, root), root, false);
    expect(owner.scrollTop).toBe(210);
    owner.scrollTop = 0; // An intentional return to the top is also an edit.
    memory.afterRender(memory.beforeRender(b, root), root, true);
    expect(owner.scrollTop).toBe(0);
  });

  it('does not mistake a coalesced morph reset for user input before its restore callback', () => {
    const memory = new TicketScrollMemory(),
      owner = scroller(),
      root = rootFor(owner),
      scope = { project: 'one', mode: 'board', view: 'queue' };
    memory.afterRender(memory.beforeRender(scope, root), root, false);
    owner.scrollTop = 480;
    memory.afterRender(memory.beforeRender(scope, root), root, false);
    const first = memory.beforeRender(scope, root);
    owner.scrollTop = 0; // The first morph clamps/replaces DOM before its queued restore runs.
    const second = memory.beforeRender(scope, root);
    memory.afterRender(first, root, true);
    expect(owner.scrollTop).toBe(0);
    memory.afterRender(second, root, true);
    expect(owner.scrollTop).toBe(480);
  });
});
