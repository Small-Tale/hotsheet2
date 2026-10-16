/**
 * Publish the drawer chat's floating composer clearance (HS2-W0KJVQ).
 *
 * The embedded conversation floats its composer form (the running-turn pill or may-be-stuck banner
 * plus the Kerf Composer) over the bottom of its transcript. Instead of reserving a fixed padding,
 * this wiring measures how far the form covers the conversation's block end and publishes it on the
 * conversation root as Kerf's inherited `--kui-scroll-clearance-block-end` context. The transcript is
 * a Kerf `List` with `scrollClearance="block-end"`, so it adds exactly that clearance inside its own
 * scroll area: messages scroll beneath the composer and the last one still rests above it, the same
 * contract a `DialogSurface` floating footer uses in the dialog presentation.
 *
 * The measurement follows the form as it grows or shrinks (a multi-line draft, the pill or banner
 * appearing) through a `ResizeObserver`, picks up conversations as they render through a
 * `MutationObserver`, and restores the property before paint when a re-render drops it.
 */
export const COMPOSER_CLEARANCE_PROPERTY = '--kui-scroll-clearance-block-end';

const CONVERSATION_SELECTOR = '.ai-conversation--embedded';
const COMPOSER_SELECTOR = ':scope > .ai-conversation__composer';

interface BlockBox {
  readonly top: number;
  readonly bottom: number;
}

/**
 * The block-end clearance a floating composer needs: the distance from its top edge to the bottom of
 * the conversation it covers, rounded up so the last message never rests a fraction under it. A
 * missing composer (a read-only transcript) needs none.
 */
export function composerClearance(conversation: BlockBox, composer: BlockBox | undefined): number {
  if (!composer) return 0;
  return Math.max(0, Math.ceil(conversation.bottom - composer.top));
}

export interface ComposerClearanceEnvironment {
  createResizeObserver: (callback: () => void) => Pick<ResizeObserver, 'observe' | 'unobserve' | 'disconnect'>;
  createMutationObserver: (
    callback: (records: readonly MutationRecord[]) => void,
  ) => Pick<MutationObserver, 'observe' | 'disconnect'>;
  /** Run once the next frame lays out, before it paints. */
  requestFrame: (callback: () => void) => number;
  cancelFrame: (handle: number) => void;
}

const browserEnvironment: ComposerClearanceEnvironment = {
  createResizeObserver: (callback) => new ResizeObserver(callback),
  createMutationObserver: (callback) => new MutationObserver(callback),
  requestFrame: (callback) => requestAnimationFrame(callback),
  cancelFrame: (handle) => {
    cancelAnimationFrame(handle);
  },
};

/** Wire every embedded conversation under `root`; the disposer stops and removes the clearance. */
export function wireEmbeddedComposerClearance(root: HTMLElement, injected?: ComposerClearanceEnvironment): () => void {
  // Without layout observers (a non-browser host) there is nothing to measure.
  if (!injected && (typeof ResizeObserver === 'undefined' || typeof MutationObserver === 'undefined')) return () => {};
  const environment = injected ?? browserEnvironment;
  const observed = new Set<Element>();
  const published = new Set<HTMLElement>();
  const conversations = () => {
    const found = Array.from(root.querySelectorAll<HTMLElement>(CONVERSATION_SELECTOR));
    if (root.matches(CONVERSATION_SELECTOR)) found.unshift(root);
    return found;
  };
  const publish = (conversation: HTMLElement) => {
    const composer = conversation.querySelector<HTMLElement>(COMPOSER_SELECTOR) ?? undefined;
    const value = `${composerClearance(conversation.getBoundingClientRect(), composer?.getBoundingClientRect())}px`;
    if (conversation.style.getPropertyValue(COMPOSER_CLEARANCE_PROPERTY) !== value)
      conversation.style.setProperty(COMPOSER_CLEARANCE_PROPERTY, value);
    published.add(conversation);
  };
  const resize = environment.createResizeObserver(() => {
    for (const conversation of conversations()) publish(conversation);
  });
  const sync = () => {
    const live = conversations();
    const composers = new Set<Element>();
    for (const conversation of live) {
      const composer = conversation.querySelector(COMPOSER_SELECTOR);
      if (composer) composers.add(composer);
      publish(conversation);
    }
    for (const composer of observed)
      if (!composers.has(composer)) {
        resize.unobserve(composer);
        observed.delete(composer);
      }
    for (const composer of composers)
      if (!observed.has(composer)) {
        resize.observe(composer);
        observed.add(composer);
      }
    for (const conversation of published) if (!conversation.isConnected) published.delete(conversation);
  };
  // Structure (a conversation or its composer rendering) and a re-render that rewrites a
  // conversation root's `style` re-sync once per frame, before paint; other style changes under the
  // root are ignored, and an unchanged value is never rewritten, so the sync cannot feed itself.
  let frame: number | undefined;
  const mutations = environment.createMutationObserver((records) => {
    const relevant = records.some(
      (record) =>
        record.type === 'childList' || (record.target as Partial<Element>).matches?.(CONVERSATION_SELECTOR) === true,
    );
    if (!relevant || frame !== undefined) return;
    frame = environment.requestFrame(() => {
      frame = undefined;
      sync();
    });
  });
  mutations.observe(root, { childList: true, subtree: true, attributes: true, attributeFilter: ['style'] });
  sync();
  return () => {
    if (frame !== undefined) environment.cancelFrame(frame);
    frame = undefined;
    mutations.disconnect();
    resize.disconnect();
    observed.clear();
    for (const conversation of published) conversation.style.removeProperty(COMPOSER_CLEARANCE_PROPERTY);
    published.clear();
  };
}
