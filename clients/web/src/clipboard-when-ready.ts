/**
 * Copy text that is only known after an async step (a device-sign-in code), without losing the
 * user's click. Safari only allows clipboard writes inside a user gesture, so the write must start
 * synchronously with a promise-backed `ClipboardItem`; browsers without it fall back to
 * `writeText` once the text resolves. Resolves `false` instead of throwing when copying is refused.
 */
export async function copyWhenReady(text: Promise<string>): Promise<boolean> {
  try {
    const clipboard = typeof navigator === 'undefined' ? undefined : navigator.clipboard;
    if (!clipboard) return false;
    if (typeof ClipboardItem !== 'undefined' && typeof clipboard.write === 'function') {
      await clipboard.write([
        new ClipboardItem({ 'text/plain': text.then((value) => new Blob([value], { type: 'text/plain' })) }),
      ]);
      return true;
    }
    await clipboard.writeText(await text);
    return true;
  } catch {
    return false;
  }
}
