import { placeTokenSearchCaret } from '@kerfjs/ui/token-search-field';

const pending = new WeakMap<ParentNode, Map<string, object>>();

/**
 * Restore app-owned token-edit focus after the current synchronous render/batch.
 * Complete before the next input task: a delayed animation frame could collapse a
 * newer replacement selection and turn its insertion into duplicated query text.
 */
export function restoreInlineSearchCaret(root: ParentNode, selector: string, offset?: number): void {
  const editor = root.querySelector<HTMLElement>(selector);
  if (!editor) return;
  const document = editor.ownerDocument,
    active = document.activeElement,
    request = {};
  let requests = pending.get(root);
  if (!requests) {
    requests = new Map();
    pending.set(root, requests);
  }
  requests.set(selector, request);
  queueMicrotask(() => {
    if (requests.get(selector) !== request) return;
    requests.delete(selector);
    const replacement = root.querySelector<HTMLElement>(selector);
    if (
      !replacement ||
      (document.activeElement !== active &&
        document.activeElement !== replacement &&
        document.activeElement !== document.body)
    )
      return;
    placeTokenSearchCaret(replacement, offset);
  });
}
