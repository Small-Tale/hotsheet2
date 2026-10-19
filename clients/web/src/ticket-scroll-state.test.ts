import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  captureTicketScrollState,
  restoreTicketScrollResetToZero,
  restoreTicketScrollState,
  TicketScrollMemory,
} from './ticket-scroll-state';

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
  it('recovers an overlay-induced zero while preserving subsequent user scroll and other columns', () => {
    const first = scroller('column:not-started', 260),
      second = scroller('column:started', 340, 40),
      root = rootFor(first, second),
      before = captureTicketScrollState(root);
    first.scrollTop = 0;
    second.scrollTop = 365;
    second.scrollLeft = 0;
    restoreTicketScrollResetToZero(before, root);
    expect([first.scrollTop, second.scrollTop, second.scrollLeft]).toEqual([260, 365, 40]);
    first.scrollTop = 280;
    second.scrollLeft = 55;
    restoreTicketScrollResetToZero(before, root);
    expect([first.scrollTop, second.scrollTop, second.scrollLeft]).toEqual([280, 365, 55]);
  });

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

  it('defers a contained list destination until progressive rows settle', () => {
    const memory = new TicketScrollMemory(),
      owner = scroller(),
      root = rootFor(owner),
      list = { project: 'one', mode: 'list', view: 'queue' },
      board = { ...list, mode: 'board' };
    memory.afterRender(memory.beforeRender(list, root), root, true);
    owner.scrollTop = 900;
    memory.afterRender(memory.beforeRender(board, root), root, true);
    owner.scrollTop = 0;
    owner.scrollHeight = 600;
    memory.afterRender(memory.beforeRender(list, root), root, false, true);
    expect(owner.scrollTop).toBe(0);
    owner.scrollHeight = 2000;
    memory.afterRender(memory.beforeRender(list, root), root, true, true);
    expect(owner.scrollTop).toBe(900);
  });

  it('keeps a user scroll made before a contained list settles', () => {
    const memory = new TicketScrollMemory(),
      owner = scroller(),
      root = rootFor(owner),
      list = { project: 'one', mode: 'list', view: 'queue' },
      board = { ...list, mode: 'board' };
    memory.afterRender(memory.beforeRender(list, root), root, true);
    owner.scrollTop = 900;
    memory.afterRender(memory.beforeRender(board, root), root, true);
    owner.scrollTop = 0;
    owner.scrollHeight = 600;
    memory.afterRender(memory.beforeRender(list, root), root, false, true);
    owner.scrollTop = 260;
    owner.scrollHeight = 2000;
    memory.afterRender(memory.beforeRender(list, root), root, true, true);
    expect(owner.scrollTop).toBe(260);
  });

  it('restores an in-range scroll during a contained list mutation before settling', () => {
    const memory = new TicketScrollMemory(),
      owner = scroller(),
      root = rootFor(owner),
      list = { project: 'one', mode: 'list', view: 'queue' };
    memory.afterRender(memory.beforeRender(list, root), root, false, true);
    owner.scrollTop = 200;
    const generation = memory.beforeRender(list, root);
    owner.scrollTop = 0; // The row mutation morph resets the viewport.
    owner.scrollHeight = 600;
    memory.afterRender(generation, root, false, true);
    expect(owner.scrollTop).toBe(200);
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

describe('ticket scroll state layout reads (HS2-4B8Q5D)', () => {
  // A controllable MutationObserver: tests queue structural records for the observed owners.
  const observers: FakeObserver[] = [];
  class FakeObserver {
    records: unknown[] = [];
    constructor(readonly callback: () => void) {
      observers.push(this);
    }
    observe() {}
    disconnect() {}
    takeRecords() {
      return this.records.splice(0);
    }
  }
  const mutate = () => {
    for (const observer of observers) observer.records.push({ type: 'childList' });
  };

  function measuredScroller(key = 'workspace') {
    const owner = Object.assign(scroller(key), { nodeType: 1, sizeReads: 0 }),
      sizes = { scrollHeight: 2000, clientHeight: 200, scrollWidth: 1000, clientWidth: 200 };
    for (const name of Object.keys(sizes) as (keyof typeof sizes)[])
      Object.defineProperty(owner, name, {
        get() {
          owner.sizeReads += 1;
          return sizes[name];
        },
        set(value: number) {
          sizes[name] = value;
        },
      });
    return owner;
  }

  beforeEach(() => {
    observers.length = 0;
    vi.stubGlobal('MutationObserver', FakeObserver);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('skips clamping once a pending destination is reached until content changes structurally', () => {
    const memory = new TicketScrollMemory(),
      owner = measuredScroller(),
      root = rootFor(owner),
      a = { project: 'one', mode: 'list', view: 'queue' },
      b = { ...a, view: 'archive' };
    memory.afterRender(memory.beforeRender(a, root), root, true);
    owner.scrollTop = 900;
    memory.afterRender(memory.beforeRender(b, root), root, true);
    // Returning while progressive rows are still arriving restores, then reaches, the destination.
    memory.afterRender(memory.beforeRender(a, root), root, false);
    expect(owner.scrollTop).toBe(900);
    // Attribute-only renders (selection, edits) while unsettled read no scroll size at all.
    owner.sizeReads = 0;
    for (let index = 0; index < 5; index += 1) memory.afterRender(memory.beforeRender(a, root), root, false);
    expect(owner.sizeReads).toBe(0);
    expect(owner.scrollTop).toBe(900);
    // A user scroll during that time becomes the destination without measurement.
    owner.scrollTop = 640;
    memory.afterRender(memory.beforeRender(a, root), root, false);
    expect(owner.sizeReads).toBe(0);
    // A structural morph that resets the viewport is restored to the user's position.
    const generation = memory.beforeRender(a, root);
    mutate();
    owner.scrollTop = 0;
    memory.afterRender(generation, root, false);
    expect(owner.scrollTop).toBe(640);
    expect(owner.sizeReads).toBeGreaterThan(0);
    // Settling clears the pending restore without another measurement.
    owner.sizeReads = 0;
    memory.afterRender(memory.beforeRender(a, root), root, true);
    expect(owner.sizeReads).toBe(0);
    memory.afterRender(memory.beforeRender(a, root), root, true);
    expect(owner.sizeReads).toBe(0);
  });

  it('keeps measuring while a destination is clamped, then after shrink, growth and refill', () => {
    const memory = new TicketScrollMemory(),
      owner = measuredScroller(),
      root = rootFor(owner),
      a = { project: 'one', mode: 'list', view: 'queue' },
      b = { ...a, view: 'archive' };
    memory.afterRender(memory.beforeRender(a, root), root, true);
    owner.scrollTop = 900;
    memory.afterRender(memory.beforeRender(b, root), root, true);
    owner.scrollHeight = 600;
    memory.afterRender(memory.beforeRender(a, root), root, false);
    expect(owner.scrollTop).toBe(400);
    // Clamped, so the destination is not reached: the next pass measures even without mutations.
    owner.sizeReads = 0;
    owner.scrollHeight = 2000;
    memory.afterRender(memory.beforeRender(a, root), root, false);
    expect(owner.sizeReads).toBeGreaterThan(0);
    expect(owner.scrollTop).toBe(900);
    // Empty-then-refill after reaching it: rows removed, then added back.
    let generation = memory.beforeRender(a, root);
    mutate();
    owner.scrollHeight = 0;
    owner.scrollTop = 0;
    memory.afterRender(generation, root, false);
    expect(owner.scrollTop).toBe(0);
    generation = memory.beforeRender(a, root);
    mutate();
    owner.scrollHeight = 2000;
    memory.afterRender(generation, root, true);
    expect(owner.scrollTop).toBe(900);
  });

  it('does not mistake a coalesced structural reset after a reached pass for user input', () => {
    const memory = new TicketScrollMemory(),
      owner = measuredScroller('column:started'),
      root = rootFor(owner),
      scope = { project: 'one', mode: 'board', view: 'queue' };
    memory.afterRender(memory.beforeRender(scope, root), root, true);
    owner.scrollTop = 430;
    // A settled re-render records the reached offset without measuring.
    memory.afterRender(memory.beforeRender(scope, root), root, true);
    const first = memory.beforeRender(scope, root);
    mutate();
    owner.scrollTop = 0; // A layout-switching morph resets the column before its restore runs.
    const second = memory.beforeRender(scope, root);
    memory.afterRender(first, root, true);
    memory.afterRender(second, root, true);
    expect(owner.scrollTop).toBe(430);
  });

  it('restores a coalesced WebKit reset without a DOM mutation record, then accepts user scrolling', () => {
    const memory = new TicketScrollMemory(),
      owner = measuredScroller('workspace'),
      root = rootFor(owner),
      scope = { project: 'one', mode: 'list', view: 'queue' };
    memory.afterRender(memory.beforeRender(scope, root), root, true);
    owner.scrollTop = 430;
    memory.afterRender(memory.beforeRender(scope, root), root, true);
    const first = memory.beforeRender(scope, root);
    owner.scrollTop = 0; // WebKit resets the native scroller during the first morph.
    const second = memory.beforeRender(scope, root);
    memory.afterRender(first, root, true);
    memory.afterRender(second, root, true);
    expect(owner.scrollTop).toBe(430);
    owner.scrollTop = 455;
    memory.afterRender(memory.beforeRender(scope, root), root, true);
    expect(owner.scrollTop).toBe(455);
  });

  it('treats a replaced scroll owner as changed even without mutation records', () => {
    const memory = new TicketScrollMemory(),
      first = measuredScroller(),
      second = measuredScroller(),
      owners = [first],
      root = { querySelectorAll: () => owners } as unknown as ParentNode,
      a = { project: 'one', mode: 'list', view: 'queue' };
    memory.afterRender(memory.beforeRender(a, root), root, false);
    first.scrollTop = 300;
    memory.afterRender(memory.beforeRender(a, root), root, false);
    const generation = memory.beforeRender(a, root);
    owners[0] = second;
    memory.afterRender(generation, root, false);
    expect(second.scrollTop).toBe(300);
  });
});
