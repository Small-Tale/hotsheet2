/**
 * Minimize and restore motion for ticket readers (HS2-Q8WF64). A reader dialog lives in the top
 * layer, so the app animates its own stand-in instead of the Kerf dialog: a plain surface card that
 * flies between the reader's rectangle and the minimized tickets control in about 200 ms. Under
 * `prefers-reduced-motion` the card cross-fades in place instead of moving. The card is decorative,
 * inert, and hidden from assistive technology, and nothing here issues a request.
 */

export const READER_MINIMIZE_DURATION_MS = 200;
const EASING = 'cubic-bezier(.2,.8,.2,1)';
/** Above the workspace and its floating controls; reader dialogs are in the top layer above it. */
const MOTION_LAYER = '95';

export interface MotionRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface ReaderMotionEnvironment {
  document: Document;
  reduceMotion: boolean;
}

/** The selector of the control a minimized reader flies into and a restored one flies out of. */
export const MINIMIZED_TICKETS_CONTROL_SELECTOR = '[data-minimized-tickets-control]';

function defaultEnvironment(): ReaderMotionEnvironment | undefined {
  if (typeof document === 'undefined') return undefined;
  const view = document.defaultView;
  return {
    document,
    reduceMotion: Boolean(view?.matchMedia('(prefers-reduced-motion: reduce)').matches),
  };
}

/** Where a reader dialog lands when it opens: centered, at most 1024px wide, viewport height. */
export function readerTargetRect(viewport: { width: number; height: number }): MotionRect {
  const width = Math.min(1024, Math.max(0, viewport.width - 32)),
    height = Math.max(0, viewport.height - 48);
  return { left: (viewport.width - width) / 2, top: (viewport.height - height) / 2, width, height };
}

/** The transform that maps `from` onto `to` for a card laid out at `from`. */
export function rectTransform(from: MotionRect, to: MotionRect): string {
  const scaleX = from.width > 0 ? to.width / from.width : 1,
    scaleY = from.height > 0 ? to.height / from.height : 1;
  return `translate(${to.left - from.left}px, ${to.top - from.top}px) scale(${scaleX}, ${scaleY})`;
}

function controlRect(document: Document): MotionRect | undefined {
  const control = [...document.querySelectorAll<HTMLElement>(MINIMIZED_TICKETS_CONTROL_SELECTOR)].find(
    (candidate) => candidate.getClientRects().length > 0,
  );
  const rect = control?.getBoundingClientRect();
  return rect && rect.width > 0
    ? { left: rect.left, top: rect.top, width: rect.width, height: rect.height }
    : undefined;
}

function ghostCard(document: Document, rect: MotionRect, kind: 'minimize' | 'restore') {
  const ghost = document.createElement('div');
  ghost.dataset.readerMotionGhost = kind;
  ghost.ariaHidden = 'true';
  ghost.inert = true;
  ghost.style.cssText = [
    'position:fixed',
    `z-index:${MOTION_LAYER}`,
    'pointer-events:none',
    'margin:0',
    `left:${rect.left}px`,
    `top:${rect.top}px`,
    `width:${rect.width}px`,
    `height:${rect.height}px`,
    'transform-origin:0 0',
    'border-radius:var(--wa-border-radius-l, 16px)',
    'border:1px solid var(--wa-color-surface-border)',
    'background:var(--wa-color-surface-default)',
    'box-shadow:var(--wa-shadow-l)',
    'will-change:transform,opacity',
  ].join(';');
  document.body.append(ghost);
  return ghost;
}

/** Play the keyframes; the card stays at its last frame until the caller removes it. */
async function run(ghost: HTMLElement, keyframes: Keyframe[]) {
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Older WebViews and the DOM test environment lack the Web Animations API. (HS2-Q8WF64)
  if (!ghost.animate) return;
  const animation = ghost.animate(keyframes, {
    duration: READER_MINIMIZE_DURATION_MS,
    easing: EASING,
    fill: 'forwards',
  });
  try {
    await animation.finished;
  } catch {
    // A cancelled animation still ends the motion.
  }
}

/** Fly a stand-in from the closed reader's rectangle into the minimized tickets control. */
export async function animateReaderMinimize(
  from: MotionRect | undefined,
  environment = defaultEnvironment(),
): Promise<void> {
  if (!environment || !from || from.width <= 0) return;
  const { document, reduceMotion } = environment,
    ghost = ghostCard(document, from, 'minimize'),
    to = controlRect(document);
  await run(
    ghost,
    reduceMotion || !to
      ? [{ opacity: 1 }, { opacity: 0 }]
      : [
          { opacity: 1, transform: 'none' },
          { opacity: 0.2, transform: rectTransform(from, to) },
        ],
  );
  ghost.remove();
}

/**
 * Fly a stand-in out of the minimized tickets control to where the restored reader opens. The card
 * stays in place until the caller calls the returned function once the reader dialog is open, so no
 * frame shows neither.
 */
export async function animateReaderRestore(
  to: MotionRect | undefined,
  environment = defaultEnvironment(),
): Promise<() => void> {
  const done = () => undefined;
  if (!environment) return done;
  const { document, reduceMotion } = environment,
    view = document.defaultView,
    target = to ?? (view ? readerTargetRect({ width: view.innerWidth, height: view.innerHeight }) : undefined);
  if (!target || target.width <= 0) return done;
  const from = controlRect(document),
    ghost = ghostCard(document, target, 'restore');
  await run(
    ghost,
    reduceMotion || !from
      ? [{ opacity: 0 }, { opacity: 1 }]
      : [
          { opacity: 0.2, transform: rectTransform(target, from) },
          { opacity: 1, transform: 'none' },
        ],
  );
  return () => {
    ghost.remove();
  };
}

/** The on-screen rectangle of an open reader dialog, captured before it closes. */
export function readerDialogRect(dialog: Element | null | undefined): MotionRect | undefined {
  const rect = dialog?.getBoundingClientRect();
  return rect && rect.width > 0
    ? { left: rect.left, top: rect.top, width: rect.width, height: rect.height }
    : undefined;
}
