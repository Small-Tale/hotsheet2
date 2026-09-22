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
    const position = state.get(element.dataset.ticketScrollOwner ?? '') ?? { top: 0, left: 0 };
    element.scrollTo(
      Math.max(0, Math.min(position.left, element.scrollWidth - element.clientWidth)),
      Math.max(0, Math.min(position.top, element.scrollHeight - element.clientHeight)),
    );
  }
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
  private pending: { key: string; state: TicketScrollState } | undefined;
  private generation = 0;

  beforeRender(scope: TicketScrollScope, root: ParentNode): number {
    const key = JSON.stringify([scope.project, scope.mode, scope.view]);
    if (this.renderedKey !== undefined && !this.pending)
      this.snapshots.set(this.renderedKey, captureTicketScrollState(root));
    if (this.pending?.key !== key)
      this.pending = { key, state: this.snapshots.get(key) ?? new Map<string, TicketScrollPosition>() };
    this.renderedKey = key;
    return ++this.generation;
  }

  afterRender(generation: number, root: ParentNode, settled: boolean): void {
    if (generation !== this.generation || !this.pending) return;
    restoreTicketScrollState(this.pending.state, root);
    if (settled) this.pending = undefined;
  }
}
