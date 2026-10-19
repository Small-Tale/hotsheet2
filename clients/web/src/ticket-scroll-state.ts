export interface TicketScrollPosition {
  top: number;
  left: number;
}
export type TicketScrollState = Map<string, TicketScrollPosition>;

const SCROLL_OWNER = '[data-ticket-scroll-owner]';

function scrollOwners(root: ParentNode): HTMLElement[] {
  const element = root as HTMLElement;
  return [
    ...(typeof element.matches === 'function' && element.matches(SCROLL_OWNER) ? [element] : []),
    ...root.querySelectorAll<HTMLElement>(SCROLL_OWNER),
  ];
}

export function captureTicketScrollState(root: ParentNode = document): TicketScrollState {
  return new Map(
    scrollOwners(root).flatMap((element) => {
      const key = element.dataset.ticketScrollOwner;
      return key ? [[key, { top: element.scrollTop, left: element.scrollLeft }] as const] : [];
    }),
  );
}

export function restoreTicketScrollState(state: TicketScrollState, root: ParentNode = document): void {
  for (const element of scrollOwners(root)) {
    const position = clampPosition(state.get(element.dataset.ticketScrollOwner ?? '') ?? { top: 0, left: 0 }, element);
    element.scrollTo(position.left, position.top);
  }
}

/** Recover a browser-induced zero offset after an overlay closes without undoing a newer user scroll. */
export function restoreTicketScrollResetToZero(before: TicketScrollState, root: ParentNode = document): void {
  for (const element of scrollOwners(root)) {
    const previous = before.get(element.dataset.ticketScrollOwner ?? '');
    if (!previous) continue;
    const top = element.scrollTop === 0 && previous.top > 0 ? previous.top : element.scrollTop,
      left = element.scrollLeft === 0 && previous.left > 0 ? previous.left : element.scrollLeft;
    if (top !== element.scrollTop || left !== element.scrollLeft) {
      const position = clampPosition({ top, left }, element);
      element.scrollTo(position.left, position.top);
    }
  }
}

function clampPosition(position: TicketScrollPosition, element: HTMLElement): TicketScrollPosition {
  return {
    left: Math.max(0, Math.min(position.left, element.scrollWidth - element.clientWidth)),
    top: Math.max(0, Math.min(position.top, element.scrollHeight - element.clientHeight)),
  };
}

function wouldClampPosition(state: TicketScrollState, root: ParentNode): boolean {
  return scrollOwners(root).some((element) => {
    const desired = state.get(element.dataset.ticketScrollOwner ?? '');
    if (!desired) return false;
    const available = clampPosition(desired, element);
    return available.top !== desired.top || available.left !== desired.left;
  });
}

export interface TicketScrollScope {
  project: string;
  mode: string;
  view: string;
}

function samePosition(a: TicketScrollPosition | undefined, b: TicketScrollPosition | undefined) {
  return (a?.top ?? 0) === (b?.top ?? 0) && (a?.left ?? 0) === (b?.left ?? 0);
}

/**
 * Watches the live scroll owners for structural changes so a pending restore that already reached
 * its destination skips layout reads on renders that did not add, remove or replace content
 * (HS2-4B8Q5D). Content added or removed inside an owner, a replaced owner, or an attribute change
 * on an owner or its ancestors up to the scroll root (a layout switch such as a paged board) can
 * clamp or reset an offset. Attribute-only morphs inside an owner, such as row selection, keep the
 * browser's position and are ignored. Without MutationObserver, or with non-node owners, every
 * render counts as changed.
 */
class ScrollOwnerWatch {
  private observers: MutationObserver[] = [];
  private observed: HTMLElement[] = [];
  private changed = true;
  private viewport = '';

  /** Whether the owners or their frame changed since the last `reset`. */
  note(owners: HTMLElement[], root: ParentNode): boolean {
    if (owners.length !== this.observed.length || owners.some((owner, index) => owner !== this.observed[index])) {
      this.observed = owners;
      this.changed = true;
      for (const observer of this.observers) observer.disconnect();
      this.observers = [];
      if (
        typeof MutationObserver === 'function' &&
        owners.every((owner) => typeof (owner as Partial<Node>).nodeType === 'number')
      ) {
        const changed = () => {
            this.changed = true;
          },
          content = new MutationObserver(changed),
          frame = new MutationObserver(changed),
          framed = new Set<Node>();
        for (const owner of owners) {
          content.observe(owner, { childList: true, subtree: true });
          for (let node: Node | null = owner; node && !framed.has(node); node = node.parentNode) {
            framed.add(node);
            if (node.nodeType === 1) frame.observe(node, { attributes: true });
            if (node === root) break;
          }
        }
        this.observers = [content, frame];
      }
    }
    if (!this.observers.length) return true;
    // A viewport resize can switch layouts through CSS alone; window size reads force no layout.
    const viewport = typeof window === 'undefined' ? '' : `${window.innerWidth}x${window.innerHeight}`;
    if (viewport !== this.viewport) {
      this.viewport = viewport;
      this.changed = true;
    }
    for (const observer of this.observers) if (observer.takeRecords().length) this.changed = true;
    return this.changed;
  }

  reset() {
    this.changed = false;
  }
}

/** In-session scroll memory; loading and progressive renders retain the desired destination. */
export class TicketScrollMemory {
  private readonly snapshots = new Map<string, TicketScrollState>();
  private renderedKey: string | undefined;
  private pending:
    { key: string; state: TicketScrollState; applied?: TicketScrollState; reached?: boolean } | undefined;
  private generation = 0;
  private awaitingAfterRender = false;
  private readonly watch = new ScrollOwnerWatch();

  beforeRender(scope: TicketScrollScope, root: ParentNode): number {
    const key = JSON.stringify([scope.project, scope.mode, scope.view]);
    // Two renders can coalesce before the first restore callback. A morph between them may
    // transiently reset a WebKit scroller without changing its observed DOM structure; that
    // zero is not user input and must not replace the saved destination (HS2-KTXBK2).
    if (this.awaitingAfterRender && this.pending) this.pending.reached = false;
    if (this.renderedKey !== undefined && !this.awaitingAfterRender) {
      if (this.pending?.reached && !this.watch.note(scrollOwners(root), root)) {
        // The destination is reached and nothing structural changed since, so live offsets are
        // user input and need no scroll-size measurement to interpret.
        const live = captureTicketScrollState(root);
        for (const [owner, position] of live) this.pending.state.set(owner, position);
        this.pending.applied = undefined;
        this.snapshots.set(this.renderedKey, new Map(this.pending.state));
      } else if (this.pending) {
        this.pending.reached = false;
        // Loading may clamp a desired destination, but it must not undo a newer user scroll.
        // Compare with the last applied position, clamped for any intervening layout shrink.
        for (const element of scrollOwners(root)) {
          const owner = element.dataset.ticketScrollOwner ?? '',
            applied = this.pending.applied?.get(owner);
          if (!applied) continue;
          const expected = clampPosition(applied, element),
            desired = this.pending.state.get(owner) ?? { top: 0, left: 0 };
          this.pending.state.set(owner, {
            top: element.scrollTop === expected.top ? desired.top : element.scrollTop,
            left: element.scrollLeft === expected.left ? desired.left : element.scrollLeft,
          });
        }
        this.snapshots.set(this.renderedKey, new Map(this.pending.state));
        // A coalesced render may see the intervening morph's clamp before restoration.
        // Only compare live input once per completed restoration, never against that DOM.
        this.pending.applied = undefined;
      } else this.snapshots.set(this.renderedKey, captureTicketScrollState(root));
    }
    if (!this.pending && key === this.renderedKey) {
      // A re-render of a settled scope: the live offsets just captured are the destination and
      // are already in place, so only a structural change can require a restore.
      const state = new Map(this.snapshots.get(key));
      this.pending = { key, state, reached: true };
    } else if (this.pending?.key !== key) this.pending = { key, state: new Map(this.snapshots.get(key)) };
    this.renderedKey = key;
    this.awaitingAfterRender = true;
    return ++this.generation;
  }

  afterRender(generation: number, root: ParentNode, settled: boolean, deferUntilSettled = false): void {
    if (generation !== this.generation || !this.pending) return;
    this.awaitingAfterRender = false;
    const owners = scrollOwners(root);
    if (this.pending.reached && !this.watch.note(owners, root)) {
      // Nothing could have moved a reached destination, so this pass reads no layout. The live
      // offsets still equal the destination, which a later pass compares against for user input.
      this.pending.applied = new Map(this.pending.state);
      if (settled) this.pending = undefined;
      return;
    }
    if (deferUntilSettled && !settled && wouldClampPosition(this.pending.state, root)) {
      // Do not apply a shortened-list clamp as WebKit can then shift that anchor
      // again while rows grow. An in-range user scroll still restores on mutation.
      this.pending.applied = captureTicketScrollState(root);
      return;
    }
    restoreTicketScrollState(this.pending.state, root);
    const applied = captureTicketScrollState(root),
      state = this.pending.state;
    this.pending.applied = applied;
    this.watch.note(owners, root);
    this.watch.reset();
    this.pending.reached = owners.every((owner) => {
      const key = owner.dataset.ticketScrollOwner ?? '';
      return samePosition(applied.get(key), state.get(key));
    });
    if (settled) this.pending = undefined;
  }
}
