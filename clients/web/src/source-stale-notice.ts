import { signal } from 'kerfjs';

import type { CompatibilityAssessment } from './compatibility';

/**
 * The development "newer local source" notice (HS2-B84P1S). A server whose build lags the
 * workspace source, with no client build that contradicts it, is the same client/server pair that
 * was launched: nothing is wrong, the next rebuild and restart simply picks up the newer source.
 * That case is informational and dismissible; a real build mismatch keeps the alert banner.
 */
export function isStaleSourceOnly(assessment: CompatibilityAssessment | undefined): boolean {
  return Boolean(
    assessment &&
    assessment.kind === 'compatible' &&
    assessment.sourceStale &&
    (!assessment.clientRevision || assessment.clientRevision === assessment.server?.build_revision),
  );
}

/**
 * The dismissal key: the running build and the source it lags. A later rebuild or source change
 * produces a new key, so the notice can announce the new pair.
 */
export function staleSourceNoticeKey(assessment: CompatibilityAssessment): string {
  return `${assessment.server?.build_revision ?? ''}|${assessment.server?.source_revision ?? ''}`;
}

const STORAGE_KEY = 'hotsheet.source-stale-notice.dismissed';
const MAX_DISMISSED_PAIRS = 32;

function readDismissed(storage: Pick<Storage, 'getItem'> | undefined): string[] {
  try {
    const saved = storage?.getItem(STORAGE_KEY);
    if (!saved) return [];
    try {
      const parsed: unknown = JSON.parse(saved);
      if (Array.isArray(parsed))
        return [...new Set(parsed.filter((value): value is string => typeof value === 'string'))].slice(
          -MAX_DISMISSED_PAIRS,
        );
    } catch {
      // The original HS2-B84P1S format stored one pair as plain text.
    }
    return [saved];
  } catch {
    return [];
  }
}

function browserStorage(): Storage | undefined {
  try {
    return typeof localStorage === 'undefined' ? undefined : localStorage;
  } catch {
    return undefined;
  }
}

/** Recent dismissed pairs, shared by every open project (HS2-P54NS2). */
export const staleSourceNoticeDismissed = signal<readonly string[]>(readDismissed(browserStorage()));

export function dismissStaleSourceNotice(
  assessment: CompatibilityAssessment,
  storage: Pick<Storage, 'setItem'> | undefined = browserStorage(),
) {
  const key = staleSourceNoticeKey(assessment);
  const dismissed = staleSourceNoticeDismissed.value;
  if (dismissed.includes(key)) return;
  const next = [...dismissed, key].slice(-MAX_DISMISSED_PAIRS);
  staleSourceNoticeDismissed.value = next;
  try {
    storage?.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    // Storage is a convenience: the dismissal still holds for this page.
  }
}

/** Whether the informational notice should show for this assessment. */
export function showsStaleSourceNotice(
  assessment: CompatibilityAssessment | undefined,
  dismissed: readonly string[] = staleSourceNoticeDismissed.value,
): assessment is CompatibilityAssessment {
  return isStaleSourceOnly(assessment) && !dismissed.includes(staleSourceNoticeKey(assessment!));
}
