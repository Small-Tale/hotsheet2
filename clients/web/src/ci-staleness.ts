import type { LiveCi } from './api';

/**
 * How long a `queued` or `in_progress` report stays believable (HS2-CC7TCB, docs/32-notifier-plugins.md
 * §32.8.5). The server keeps only the newest report per checkout and no run history to scale from, so the
 * bound is fixed: 2 h is several times this project's main CI (about 35 minutes), and a GitHub Actions run
 * the server follows gets its real verdict before then. Only a reporter that never reports the end leaves a
 * run in flight this long.
 */
export const CI_STALE_AFTER_MS = 2 * 60 * 60 * 1000;

/** What a stale report last said: the run was running, or waiting to run. */
export type CiLastSeen = 'running' | 'queued';

/**
 * What a stale report last said, or `undefined` while it is believable (HS2-CC7TCB). Only a report of a run
 * still queued or running goes stale, `CI_STALE_AFTER_MS` after it was reported; a newer report of the same
 * run renews it, and a finished run never goes stale.
 */
export function ciStaleness(ci: Pick<LiveCi, 'status' | 'reported_at_ms'>, nowMs: number): CiLastSeen | undefined {
  if (ci.status !== 'in_progress' && ci.status !== 'queued') return undefined;
  if (ci.reported_at_ms === null || nowMs - ci.reported_at_ms <= CI_STALE_AFTER_MS) return undefined;
  return ci.status === 'in_progress' ? 'running' : 'queued';
}
