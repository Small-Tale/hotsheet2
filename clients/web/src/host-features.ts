/**
 * Opt-in features of the local production host (HS2-ZF70Y9). The host, not the reusable client
 * bundle, decides which ones it serves, as with the browser auth mode (HS2-7DBJMS): one built `dist/`
 * runs with or without them.
 *
 * - `ux-demo`: serve the `/ux-demo` component catalog, which development servers always serve. The
 *   production client loads the catalog as a separate on-demand chunk, so a host without the feature
 *   answers `/ux-demo` with its ordinary 404 and no other page downloads the catalog.
 *
 * A host turns features on with `--feature <name>` (also spelled `--feature=<name>` or
 * `--feature:<name>`, repeatable, each value a comma list) or the `HOTSHEET_FEATURES` comma list.
 * An unknown name is an error, so a misspelled flag fails at startup instead of silently doing nothing.
 *
 * This module is shared by the Node host and the browser entry, so it imports nothing.
 */
export const HOST_FEATURES = ['ux-demo'] as const;
export type HostFeature = (typeof HOST_FEATURES)[number];

/** The environment equivalent of `--feature`, a comma- or whitespace-separated list. */
export const HOST_FEATURES_ENVIRONMENT = 'HOTSHEET_FEATURES';
/** The `<meta name>` through which the host tells a served document which features it enabled. */
export const HOST_FEATURES_META_NAME = 'hotsheet-features';

const FEATURE_FLAG = '--feature';

function isHostFeature(name: string): name is HostFeature {
  return (HOST_FEATURES as readonly string[]).includes(name);
}

function featureNames(list: string, source: string): HostFeature[] {
  const names = list.split(/[\s,]+/u).filter((name) => name !== '');
  if (names.length === 0) throw new Error(`${source} names no feature. Known features: ${HOST_FEATURES.join(', ')}.`);
  return names.map((name) => {
    const normalized = name.toLowerCase();
    if (!isHostFeature(normalized))
      throw new Error(`Unknown Hot Sheet feature "${name}" in ${source}. Known features: ${HOST_FEATURES.join(', ')}.`);
    return normalized;
  });
}

/**
 * The features a host turns on from its own arguments (`process.argv.slice(2)`) and environment.
 * Arguments other than `--feature` are ignored; they belong to the host's other options.
 */
export function parseHostFeatures(
  argv: readonly string[],
  environment: Readonly<Record<string, string | undefined>> = {},
): Set<HostFeature> {
  const features = new Set<HostFeature>();
  const fromEnvironment = environment[HOST_FEATURES_ENVIRONMENT];
  if (fromEnvironment !== undefined && fromEnvironment.trim() !== '')
    for (const name of featureNames(fromEnvironment, HOST_FEATURES_ENVIRONMENT)) features.add(name);
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    let value: string | undefined;
    if (argument === FEATURE_FLAG) {
      value = argv.at(index + 1);
      if (value === undefined || value.startsWith('--'))
        throw new Error(`${FEATURE_FLAG} needs a feature name. Known features: ${HOST_FEATURES.join(', ')}.`);
      index += 1;
    } else if (argument.startsWith(`${FEATURE_FLAG}=`) || argument.startsWith(`${FEATURE_FLAG}:`)) {
      value = argument.slice(FEATURE_FLAG.length + 1);
    } else continue;
    for (const name of featureNames(value, argument)) features.add(name);
  }
  return features;
}

/** The `/ux-demo` catalog route, with or without a trailing path. A query string is not part of a path. */
export function isUxDemoPath(pathname: string): boolean {
  return /^\/ux-demo(?:\/|$)/u.test(pathname);
}

/** Add or replace the feature meta tag in a served document; no features means no tag. */
export function stampHostFeatures(html: string, features: ReadonlySet<HostFeature>): string {
  const existing = /<meta\s+name=["']hotsheet-features["']\s+content=["'][^"']*["']\s*\/?\s*>/iu;
  const enabled = HOST_FEATURES.filter((feature) => features.has(feature));
  const tag = enabled.length === 0 ? '' : `<meta name="${HOST_FEATURES_META_NAME}" content="${enabled.join(' ')}">`;
  if (existing.test(html)) return html.replace(existing, tag);
  return tag === '' ? html : html.replace(/<\/head>/iu, `${tag}</head>`);
}

/** The features the serving host stamped into this document. */
export function documentHostFeatures(document: Pick<Document, 'querySelector'>): Set<HostFeature> {
  const content =
    document.querySelector<HTMLMetaElement>(`meta[name="${HOST_FEATURES_META_NAME}"]`)?.getAttribute('content') ?? '';
  return new Set(content.split(/\s+/u).filter(isHostFeature));
}

/**
 * Whether the production entry boots the component catalog instead of the app: only on the catalog
 * route, and only when the serving host turned the feature on.
 */
export function shouldBootUxDemo(pathname: string, features: ReadonlySet<HostFeature>): boolean {
  return features.has('ux-demo') && isUxDemoPath(pathname);
}
