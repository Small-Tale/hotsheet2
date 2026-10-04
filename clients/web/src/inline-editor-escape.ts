/**
 * Escape inside an inline text editor (HS2-Q2T01A).
 *
 * Inline editors autosave when focus leaves them, so Escape finishes the edit by blurring the field and
 * stops there: the narrow-layout inspector overlay, which Kerf's Workbench dismisses from a document
 * keydown listener, stays open until a second Escape outside the editor. Fields with their own Escape
 * semantics (the attachment batch label restores its value) keep them and only stop the key.
 */

/** Native and Web Awesome text-entry controls that count as inline editors. */
export const INLINE_EDITOR_SELECTOR =
  'textarea, input:not([type="checkbox"], [type="radio"], [type="button"], [type="submit"], [type="reset"], [type="file"], [type="range"], [type="color"]), wa-input, wa-textarea, [contenteditable="true"]';

export type InlineEditorEscape = 'none' | 'stop' | 'finish';

/**
 * What an Escape keydown does when it starts in `target`: `finish` blurs the editor and stops the key,
 * `stop` only stops it (the field handles Escape itself), `none` leaves it alone.
 */
export function inlineEditorEscape(
  event: Pick<KeyboardEvent, 'key' | 'isComposing'>,
  target: Pick<Element, 'matches'> | null,
  ownEscapeSelector?: string,
): InlineEditorEscape {
  if (event.key !== 'Escape' || event.isComposing || !target?.matches(INLINE_EDITOR_SELECTOR)) return 'none';
  return ownEscapeSelector && target.matches(ownEscapeSelector) ? 'stop' : 'finish';
}
