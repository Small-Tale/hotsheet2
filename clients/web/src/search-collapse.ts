import type { TokenSearchFieldValue } from '@kerfjs/ui/token-search-field';

/** A query with only whitespace has no search meaning and can close on blur. */
export function isWhitespaceOnlySearch(value: TokenSearchFieldValue): boolean {
  return value.tokens.length === 0 && value.query.length > 0 && value.query.trim().length === 0;
}

/** Web Component triggers can receive focus inside a shadow root; follow hosts to keep-open surfaces. */
export function keepSearchOpenForShadowTarget(target: Node | null): boolean {
  let current = target;
  while (current) {
    if (current instanceof Element && current.closest('[data-token-search-keep-open]')) return true;
    const root = current.getRootNode();
    current = root instanceof ShadowRoot ? root.host : null;
  }
  return false;
}
