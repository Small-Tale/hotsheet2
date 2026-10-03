/**
 * Single-line semantics for the wrapping ticket title editor (HS2-98ZVPE).
 *
 * The title editor is a textarea so a long title wraps like the static heading, but a ticket title is
 * still one line: Enter finishes the edit instead of inserting a newline, and line breaks that arrive
 * through paste or drop collapse to a single space.
 */

const LINE_BREAK_RUN = /[^\S\r\n]*[\r\n][\s]*/g;

/** Collapses every run of line breaks, with the whitespace around it, into one space. */
export function singleLineTicketTitle(value: string): string {
  return value.replace(LINE_BREAK_RUN, ' ');
}

/** Whether a keydown in the title editor finishes the edit (Enter outside IME composition). */
export function ticketTitleKeyFinishesEdit(event: KeyboardEvent): boolean {
  return event.key === 'Enter' && !event.isComposing;
}

/**
 * Removes line breaks from the live title field, keeping the caret at the same logical position, and
 * returns the single-line value. A field that already holds one line is left untouched.
 */
export function normalizeTicketTitleField(field: HTMLTextAreaElement | HTMLInputElement): string {
  const value = field.value,
    normalized = singleLineTicketTitle(value);
  if (normalized === value) return value;
  const caret = singleLineTicketTitle(value.slice(0, field.selectionEnd ?? value.length)).length;
  field.value = normalized;
  field.setSelectionRange(caret, caret);
  return normalized;
}
