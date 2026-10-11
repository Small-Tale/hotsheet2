/**
 * The application shell is a fixed `100dvh` page whose root clips overflow (`style.css`), so the
 * document itself never scrolls by design. iOS Safari still scrolls the root programmatically to
 * reveal a focused text field above the virtual keyboard, and because the root clips overflow the
 * user cannot scroll it back once the keyboard closes: the whole app stays shifted up with an empty
 * band underneath (HS2-BCA512). When focus leaves text entry, or the keyboard-shrunk visual viewport
 * resizes, return the document to its origin.
 */
export interface DocumentScrollRestoreHost {
  readonly document: Pick<Document, 'activeElement' | 'addEventListener' | 'removeEventListener'>;
  readonly scrollX: number;
  readonly scrollY: number;
  scrollTo(x: number, y: number): void;
  readonly visualViewport?: Pick<VisualViewport, 'addEventListener' | 'removeEventListener'> | null;
  setTimeout(callback: () => void, ms: number): unknown;
}

const TEXT_INPUT_TYPES = new Set(['', 'text', 'search', 'email', 'url', 'tel', 'password', 'number']);

/** Whether `element` is a text-entry surface that legitimately holds the keyboard open. */
export function isTextEntryElement(element: Element | null | undefined): boolean {
  if (!element) return false;
  const tag = element.tagName.toLowerCase();
  if (tag === 'textarea' || tag === 'wa-input' || tag === 'wa-textarea') return true;
  if (tag === 'input') return TEXT_INPUT_TYPES.has((element.getAttribute('type') ?? '').toLowerCase());
  return (element as HTMLElement).isContentEditable;
}

/** Reset a stray document scroll offset when no text field still owns the keyboard. */
export function restoreDocumentScroll(host: DocumentScrollRestoreHost): boolean {
  if (isTextEntryElement(host.document.activeElement)) return false;
  if (host.scrollX === 0 && host.scrollY === 0) return false;
  host.scrollTo(0, 0);
  return true;
}

/** Install the restore listeners; returns a disposer. */
export function installDocumentScrollRestore(host: DocumentScrollRestoreHost): () => void {
  // A field-to-field move fires focusout before the next focusin; deferring lets that move keep the
  // keyboard's offset so only a real dismissal restores the origin.
  const schedule = () => {
    host.setTimeout(() => restoreDocumentScroll(host), 0);
  };
  host.document.addEventListener('focusout', schedule);
  host.visualViewport?.addEventListener('resize', schedule);
  return () => {
    host.document.removeEventListener('focusout', schedule);
    host.visualViewport?.removeEventListener('resize', schedule);
  };
}
