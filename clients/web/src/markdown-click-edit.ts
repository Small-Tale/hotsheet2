/**
 * Single-click entry into a rendered Markdown field's editor (HS2-H1K9YY).
 *
 * Details and notes render Markdown that may contain links, ticket references, attachment
 * buttons, and other controls. A click on one of those performs its own action and must never
 * also start editing the field that contains it, so the edit handlers ignore any activation
 * that starts inside an interactive descendant of the editable surface. The descendant's own
 * delegated handler (for example `open-linked-ticket`) still runs, because the event keeps
 * propagating to it; only the edit entry is suppressed.
 */

/** Elements inside rendered Markdown that own their own click or key activation. */
export const MARKDOWN_INTERACTIVE_SELECTOR = [
  'a[href]',
  'button',
  'input:not(:disabled)',
  'select',
  'textarea',
  'label',
  'summary',
  '[contenteditable]:not([contenteditable="false"])',
  '[data-action]',
  '[role="button"]',
  '[role="link"]',
  '[role="checkbox"]',
  '[role="menuitem"]',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

/** The element surface the walk needs; real DOM elements satisfy it. */
type ElementLike = Pick<Element, 'matches' | 'localName' | 'contains'> & { parentElement: ElementLike | null };

function isElementLike(value: unknown): value is ElementLike {
  return typeof (value as Partial<ElementLike> | null)?.matches === 'function';
}

/** True when `start` sits inside an interactive element that is a descendant of `surface`. */
export function startsInInteractiveDescendant(start: unknown, surface: ElementLike): boolean {
  if (!isElementLike(start) || !surface.contains(start as unknown as Node)) return false;
  for (let node: ElementLike | null = start; node && node !== surface; node = node.parentElement)
    if (node.matches(MARKDOWN_INTERACTIVE_SELECTOR) || node.localName.startsWith('wa-')) return true;
  return false;
}

/** True when the user has a non-empty text selection inside `surface` (a drag-select, not a click). */
export function hasTextSelectionWithin(
  surface: Pick<Element, 'contains'>,
  selection: Pick<Selection, 'isCollapsed' | 'rangeCount' | 'getRangeAt'> | null = getSelection(),
): boolean {
  if (!selection || selection.isCollapsed || selection.rangeCount === 0) return false;
  const range = selection.getRangeAt(0);
  return surface.contains(range.commonAncestorContainer);
}

/**
 * Whether a pointer click on `surface` should begin editing it: a plain primary-button click
 * that neither started on an interactive descendant nor finished a text selection.
 */
export function clickBeginsMarkdownEdit(
  event: Pick<MouseEvent, 'defaultPrevented' | 'button' | 'metaKey' | 'ctrlKey' | 'shiftKey' | 'altKey' | 'target'>,
  surface: ElementLike,
): boolean {
  if (event.defaultPrevented || event.button !== 0) return false;
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return false;
  if (startsInInteractiveDescendant(event.target, surface)) return false;
  return !hasTextSelectionWithin(surface);
}

/**
 * Whether an Enter/Space keydown on `surface` should begin editing it. Keys pressed while focus
 * is on a nested link or control belong to that control (Enter follows the link).
 */
export function keyBeginsMarkdownEdit(
  event: Pick<KeyboardEvent, 'defaultPrevented' | 'key' | 'target'>,
  surface: ElementLike,
): boolean {
  if (event.defaultPrevented || !['Enter', ' '].includes(event.key)) return false;
  return !startsInInteractiveDescendant(event.target, surface);
}

/**
 * Containers whose editor a single click opens in place of the rendered Markdown. A note's Kerf
 * ContentItem frame (HS2-GZDKM7) owns its padding, so a repeat press there belongs to the note too.
 */
const CLICK_EDIT_CONTAINER =
  '[data-component="note-card"], [data-component="content-item"]:has(> [data-component="note-card"]), [data-component="markdown-editor"]';

/**
 * Whether a `mousedown` is the repeat press of a double-click whose first click just opened an
 * editor in the same container. Preventing its default keeps focus in the new editor, so a
 * habitual double-click opens the editor instead of opening it and immediately closing it.
 */
export function repeatPressWouldLeaveNewEditor(
  event: Pick<MouseEvent, 'detail' | 'button' | 'target'>,
  active: Pick<Element, 'localName'> | null,
): boolean {
  if (event.detail < 2 || event.button !== 0 || active?.localName !== 'textarea') return false;
  const target = event.target as (ElementLike & Pick<Element, 'closest'>) | null;
  if (!target || typeof target.closest !== 'function' || target === active) return false;
  const container = target.closest(CLICK_EDIT_CONTAINER) as ElementLike | null;
  return (
    !!container && container.contains(active as unknown as Node) && !startsInInteractiveDescendant(target, container)
  );
}

/** A primary press on a click-to-edit surface, held until the click that completes it (HS2-628K80). */
export interface EditSurfacePress {
  readonly surface: Pick<Element, 'contains' | 'isConnected'>;
  readonly x: number;
  readonly y: number;
}

/** Pointer travel, in CSS pixels, between press and click that still reads as one click. */
const CLICK_SLOP = 4;

/**
 * The click-to-edit surface (matching `selector`) that a primary `mousedown` pressed, or undefined
 * when the press began elsewhere or on a nested link or control.
 */
export function pressOnEditSurface(
  event: Pick<MouseEvent, 'button' | 'target' | 'clientX' | 'clientY'>,
  selector: string,
): EditSurfacePress | undefined {
  if (event.button !== 0) return undefined;
  const target = event.target as (ElementLike & Pick<Element, 'closest'>) | null;
  if (!target || typeof target.closest !== 'function') return undefined;
  const surface = target.closest(selector);
  if (!surface || startsInInteractiveDescendant(target, surface)) return undefined;
  return { surface, x: event.clientX, y: event.clientY };
}

/**
 * The surface a click still begins editing although the browser targeted the click elsewhere.
 *
 * Pressing a surface moves focus to it, so another open editor (the Details source or a note
 * edit) loses focus, saves, and closes before the button is released. Closing it re-flows the
 * panel, the release lands on other content, and the browser sends the click to the nearest
 * common ancestor of press and release, outside the surface. Without scroll anchoring (WebKit, so
 * Safari and WKWebView, or any panel scrolled to the top) the content moves under a pointer that
 * did not. A click at the press point whose target contains the pressed surface therefore still
 * completes that press; a drag, a text selection, or a modified click does not.
 */
export function clickCompletingShiftedPress(
  event: Pick<
    MouseEvent,
    'defaultPrevented' | 'button' | 'metaKey' | 'ctrlKey' | 'shiftKey' | 'altKey' | 'target' | 'clientX' | 'clientY'
  >,
  press: EditSurfacePress | undefined,
  selection: Pick<Selection, 'isCollapsed'> | null = getSelection(),
): EditSurfacePress['surface'] | undefined {
  if (!press?.surface.isConnected) return undefined;
  const target = event.target as Pick<Node, 'contains'> | null;
  // A click inside the surface takes the ordinary path; a target that does not contain the
  // surface is not a release the re-flow moved off it.
  if (!target || typeof target.contains !== 'function' || press.surface.contains(target as Node)) return undefined;
  if (!target.contains(press.surface as Node)) return undefined;
  if (Math.abs(event.clientX - press.x) > CLICK_SLOP || Math.abs(event.clientY - press.y) > CLICK_SLOP)
    return undefined;
  if (event.defaultPrevented || event.button !== 0) return undefined;
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return undefined;
  if (selection && !selection.isCollapsed) return undefined;
  return press.surface;
}
