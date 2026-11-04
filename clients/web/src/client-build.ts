/**
 * The web client build id (HS2-KPJWBK). The build stamps it into `index.html` as a
 * `<meta name="hotsheet-client-build">` tag, so the page knows which bundle it loaded and the host
 * knows which bundle it serves. A production build derives it from the emitted, content-hashed bundle
 * file names; the development server uses one id per server process.
 */
export const CLIENT_BUILD_META_NAME = 'hotsheet-client-build';

const VALID_ID = /^[A-Za-z0-9._:-]{1,128}$/u;

/** Accept only a short, printable id; anything else is treated as unknown. */
export function validClientBuildId(value: unknown): string | undefined {
  return typeof value === 'string' && VALID_ID.test(value) ? value : undefined;
}

/** Read the build id from an `index.html` document's markup (host side). */
export function clientBuildIdFromHtml(html: string): string | undefined {
  for (const tag of html.match(/<meta\b[^>]*>/giu) ?? []) {
    const name = /\bname\s*=\s*["']([^"']*)["']/iu.exec(tag)?.[1];
    if (name !== CLIENT_BUILD_META_NAME) continue;
    return validClientBuildId(/\bcontent\s*=\s*["']([^"']*)["']/iu.exec(tag)?.[1]);
  }
  return undefined;
}

/** The build id of the bundle this page loaded, or undefined for a page without one. */
export function loadedClientBuildId(
  root: Pick<Document, 'querySelector'> | undefined = typeof document === 'undefined' ? undefined : document,
): string | undefined {
  return validClientBuildId(
    root?.querySelector<HTMLMetaElement>(`meta[name="${CLIENT_BUILD_META_NAME}"]`)?.getAttribute('content'),
  );
}
