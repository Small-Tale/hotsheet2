import type { ProviderAccount } from './api';

type AccountEndpoint = { apiBase?: string; webOrigin?: string } | { error: string };
const diagnostic =
  'This saved GitHub account has an invalid or unknown endpoint. Choose another account or sign in again.';
const asciiLower = (value: string) => value.replace(/[A-Z]/g, (character) => character.toLowerCase());
const outerSpaces = (value: string) => value.replace(/^ +| +$/g, '');
const hasControl = (value: string) => {
  for (const character of value) {
    const code = character.charCodeAt(0);
    if (code < 32 || (code >= 127 && code <= 159)) return true;
  }
  return false;
};

/** HS2-WV6QCR: account base_url is a complete API endpoint, never a web-origin shorthand. */
export function githubAccountEndpoint(account: {
  [Key in 'base_url' | 'host']?: ProviderAccount[Key] | null;
}): AccountEndpoint {
  if (account.base_url === undefined) {
    // An unknown/Enterprise host says nothing about a custom API prefix. Never guess one.
    const host = account.host ?? '';
    return !hasControl(host) && asciiLower(outerSpaces(host)) === 'github.com' ? {} : { error: diagnostic };
  }
  const raw = account.base_url;
  // The existing account response parser permits null optional fields; present null is invalid.
  if (typeof raw !== 'string') return { error: diagnostic };
  // Check before URL/trim can erase C0 controls (including outer tabs/newlines).
  if (hasControl(raw)) return { error: diagnostic };
  const apiBase = outerSpaces(raw);
  if (!apiBase || /\s|[\\?#]/.test(apiBase)) return { error: diagnostic };
  const parts = /^(https?:\/\/)([^/]+)(\/.*)?$/i.exec(apiBase);
  const scheme = parts?.[1],
    authority = parts?.[2];
  if (!scheme || !authority || authority.includes('@') || authority.endsWith(':')) return { error: diagnostic };
  try {
    // Validate only. Serializing URL.origin/href would erase a deliberate default port/path.
    const parsed = new URL(apiBase);
    if (!parsed.hostname || parsed.username || parsed.password) return { error: diagnostic };
  } catch {
    return { error: diagnostic };
  }
  const webOrigin = asciiLower(`${scheme}${authority}`);
  // Public branding is independent of the explicit API value, which remains unchanged.
  return webOrigin === 'https://api.github.com' || webOrigin === 'https://github.com'
    ? { apiBase }
    : { apiBase, webOrigin };
}
