/**
 * Push a programmatically merged draft into the text control the user is typing in (HS2-A4XCXE).
 *
 * Rendering deliberately leaves a focused control's live value alone so typing never jumps, so a merge
 * that lands while the user is focused must write the control itself. It only does so while the control
 * still holds exactly `previous` (the text that was saved), and keeps the caret at the same position
 * relative to the end of the unchanged text around it.
 */
interface TextControl {
  tagName: string;
  value: string;
  selectionStart: number | null;
  selectionEnd: number | null;
  setSelectionRange(start: number, end: number): void;
}

function isTextControl(active: unknown): active is TextControl {
  const tag = (active as { tagName?: unknown } | null)?.tagName;
  return (tag === 'TEXTAREA' || tag === 'INPUT') && typeof (active as { value?: unknown }).value === 'string';
}

export function syncFocusedDraftControl(
  active: unknown,
  previous: string,
  next: string,
  matches: (value: string) => boolean = (value) => value === previous,
): boolean {
  if (!isTextControl(active)) return false;
  if (previous === next || !matches(active.value)) return false;
  const caret = active.selectionStart ?? active.value.length,
    end = active.selectionEnd ?? caret;
  let prefix = 0;
  while (prefix < previous.length && prefix < next.length && previous[prefix] === next[prefix]) prefix += 1;
  const shift = (offset: number) =>
    offset <= prefix ? offset : Math.max(prefix, offset + next.length - previous.length);
  active.value = next;
  active.setSelectionRange(shift(caret), shift(end));
  return true;
}
