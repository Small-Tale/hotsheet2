/** HS2-VKNA12: keep the configured scheme and explicit port as sign-in identity. */
export function githubSignInOrigin(value: string): string | undefined {
  const raw = value.trim().replace(/\/+$/, '');
  if (!raw) return undefined;
  const origin = (/^https?:\/\//i.test(raw) ? raw : `https://${raw}`).replace(/[A-Z]/g, (character) =>
    character.toLowerCase(),
  );
  // Validate the typed authority before URL can discard ports or rewrite paths.
  if (!/^https?:\/\/[^/\\?#@\s]+$/.test(origin)) return undefined;
  for (const character of origin) {
    const code = character.charCodeAt(0);
    if (code < 32 || code === 127) return undefined;
  }
  try {
    const url = new URL(origin);
    return url.hostname.includes('.') ? origin : undefined;
  } catch {
    return undefined;
  }
}
