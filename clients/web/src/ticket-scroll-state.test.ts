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
});
