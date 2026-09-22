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

function clampPosition(position: TicketScrollPosition, element: HTMLElement): TicketScrollPosition {
  return {
    left: Math.max(0, Math.min(position.left, element.scrollWidth - element.clientWidth)),
    top: Math.max(0, Math.min(position.top, element.scrollHeight - element.clientHeight)),
  };
}

export interface TicketScrollScope {
  project: string;
  mode: string;
  view: string;
}

/** In-session scroll memory; loading and progressive renders retain the desired destination. */
export class TicketScrollMemory {
  private readonly snapshots = new Map<string, TicketScrollState>();
  private renderedKey: string | undefined;
  private pending: { key: string; state: TicketScrollState; applied?: TicketScrollState } | undefined;
  private generation = 0;

  beforeRender(scope: TicketScrollScope, root: ParentNode): number {
    const key = JSON.stringify([scope.project, scope.mode, scope.view]);
    if (this.renderedKey !== undefined) {
      if (this.pending) {
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
    if (this.pending?.key !== key) this.pending = { key, state: new Map(this.snapshots.get(key)) };
    this.renderedKey = key;
    return ++this.generation;
  }

  afterRender(generation: number, root: ParentNode, settled: boolean): void {
    if (generation !== this.generation || !this.pending) return;
    restoreTicketScrollState(this.pending.state, root);
    this.pending.applied = captureTicketScrollState(root);
    if (settled) this.pending = undefined;
  }
}
