/**
 * Copy known text from a click handler (HS2-1A2BQR). Safari intermittently refuses
 * `navigator.clipboard.writeText` with `NotAllowedError` even inside a click — for example when
 * the click lands while the document is not yet focused. WebKit rejects synchronously, so the
 * rejection is handled while the click is still being dispatched, and a selection-based copy can
 * take over within that same user gesture. Rejects with the original reason when both refuse.
 * Call it before the handler's first `await`.
 */
export async function copyText(text: string): Promise<void> {
  const clipboard = typeof navigator === 'undefined' ? undefined : navigator.clipboard;
  if (!clipboard) {
    if (copyWithSelection(text)) return;
    throw new Error('Copying is not available in this browser.');
  }
  try {
    await clipboard.writeText(text);
  } catch (reason) {
    if (copyWithSelection(text)) return;
    throw reason;
  }
}

/**
 * Copy through a temporary off-screen selection and the legacy copy command, then restore the
 * focus and selection the user had. Returns whether the browser performed the copy.
 */
export function copyWithSelection(text: string): boolean {
  if (typeof document === 'undefined' || !('execCommand' in document)) return false;
  const active = document.activeElement instanceof HTMLElement ? document.activeElement : null,
    selection = document.getSelection(),
    ranges = selection ? Array.from({ length: selection.rangeCount }, (_, index) => selection.getRangeAt(index)) : [],
    area = document.createElement('textarea');
  area.value = text;
  area.readOnly = true;
  area.setAttribute('aria-hidden', 'true');
  area.style.position = 'fixed';
  area.style.top = '0';
  area.style.left = '-9999px';
  area.style.opacity = '0';
  document.body.append(area);
  area.select();
  let copied: boolean;
  try {
    // Compatibility boundary: the legacy copy command is the only synchronous copy path browsers
    // still honor when they refuse the asynchronous Clipboard API.
    // eslint-disable-next-line @typescript-eslint/no-deprecated
    copied = document.execCommand('copy');
  } catch {
    copied = false;
  } finally {
    area.remove();
    if (selection) {
      selection.removeAllRanges();
      for (const range of ranges) selection.addRange(range);
    }
    active?.focus({ preventScroll: true });
  }
  return copied;
}
