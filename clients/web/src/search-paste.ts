/**
 * HS2-PQWZRG: a paste into a search field inserts the clipboard's plain text only. Search fields
 * hold one line, so every line break becomes a space, as Kerf's SearchField paste does (KF-BACCQE).
 */
export function searchPasteText(text: string | undefined): string {
  return (text ?? '').replace(/\r\n?|\n/g, ' ');
}

/**
 * Insert a paste into a Kerf SearchField editor as plain text through the browser's editing
 * command, so the insertion joins the native undo history. Kerf's own paste handler inserts
 * the same plain text with a DOM range, which Undo cannot reverse (KF-TBH48Z). It skips a paste
 * that is already default-prevented, so this runs first and leaves the event to Kerf only when
 * the browser refuses the command. The editor then reports a normal text input, which Kerf's
 * model parses as typed text. Reapplying the caret around the insertion closes the browser's
 * typing run, so Undo removes the paste on its own rather than with the keys typed beside it.
 */
export function pastePlainSearchText(event: ClipboardEvent, editor: HTMLElement): void {
  if (event.defaultPrevented || !editor.isContentEditable) return;
  const text = searchPasteText(event.clipboardData?.getData('text/plain'));
  if (!text) {
    event.preventDefault();
    return;
  }
  const document = editor.ownerDocument;
  let inserted: boolean;
  try {
    closeTypingRun(document);
    // eslint-disable-next-line @typescript-eslint/no-deprecated -- The only undoable scripted text insertion in a contenteditable editor (HS2-PQWZRG).
    inserted = document.execCommand('insertText', false, text);
  } catch {
    inserted = false;
  }
  if (!inserted) return;
  event.preventDefault();
  closeTypingRun(document);
}

/** Reapply the current selection: a scripted selection change ends the browser's typing run. */
function closeTypingRun(document: Document) {
  const selection = document.getSelection();
  if (!selection?.rangeCount) return;
  const range = selection.getRangeAt(0);
  selection.removeAllRanges();
  selection.addRange(range);
}
