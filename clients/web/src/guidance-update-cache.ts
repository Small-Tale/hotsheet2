import { type CompatibilityAssessment } from './compatibility';

const CHECK_INTERVAL_MS = 86_400_000;

export interface GuidanceUpdateCacheEntry {
  checkedAt: number;
  buildIdentity?: string;
  updates: unknown;
}

/** The host and server may upgrade independently while a checkout remains openable. */
export function guidanceUpdateBuildIdentity(compatibility: CompatibilityAssessment): string | undefined {
  const client = compatibility.clientRevision || null;
  const server = compatibility.server?.build_revision || null;
  return client || server ? JSON.stringify([client, server]) : undefined;
}

/** A missing build identity cannot prove that a result predates no app upgrade. */
export function guidanceUpdateCacheIsFresh(
  entry: GuidanceUpdateCacheEntry,
  buildIdentity: string | undefined,
  now: number,
): boolean {
  return Boolean(
    buildIdentity &&
    entry.buildIdentity === buildIdentity &&
    Number.isFinite(entry.checkedAt) &&
    entry.checkedAt >= 0 &&
    entry.checkedAt <= now &&
    now - entry.checkedAt < CHECK_INTERVAL_MS,
  );
}
