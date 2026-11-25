import { CircleCheck, CircleDashed, CircleSlash, CircleX, Clock, type IconNode, LoaderCircle } from 'lucide';

import type { LiveCiStatus } from './api';
import { formatDuration, type RightNowCi } from './stats-right-now';

/**
 * The `main` CI word and glyph per reported status, shared by the Stats Right now tile (HS2-E16ZEK)
 * and the project sidebar Live chip and row (HS2-57BYEA), in the circle family of the status badges.
 */
export const CI_PRESENTATION: Readonly<Record<LiveCiStatus, { word: string; icon: IconNode; name: string }>> = {
  success: { word: 'Passed', icon: CircleCheck, name: 'circle-check' },
  failure: { word: 'Failed', icon: CircleX, name: 'circle-x' },
  in_progress: { word: 'Running', icon: LoaderCircle, name: 'loader-circle' },
  queued: { word: 'Queued', icon: Clock, name: 'clock' },
  cancelled: { word: 'Cancelled', icon: CircleSlash, name: 'circle-slash' },
  unavailable: { word: 'No CI reported', icon: CircleDashed, name: 'circle-dashed' },
};

/** How long ago the run finished, or how long it has run or waited: "12m ago", "running 4m". */
export function ciAge(ci: RightNowCi): string | undefined {
  if (ci.ageMs === undefined) return undefined;
  if (ci.status === 'in_progress') return `running ${formatDuration(ci.ageMs)}`;
  if (ci.status === 'queued') return `waiting ${formatDuration(ci.ageMs)}`;
  return `${formatDuration(ci.ageMs)} ago`;
}

/**
 * A run's elapsed time as a clock face (HS2-505PHC): `0:05`, `1:05`, `59:59`, then `1:02:03`. A
 * negative span (a start reported ahead of the display clock) reads `0:00`.
 */
export function formatCiElapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000)),
    seconds = String(total % 60).padStart(2, '0'),
    minutes = Math.floor(total / 60);
  if (minutes < 60) return `${minutes}:${seconds}`;
  return `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, '0')}:${seconds}`;
}

/**
 * The compact Live chip's face for a non-trivial `main` CI run (HS2-505PHC, docs/34-glance-surfaces.md
 * §34.4): `CI · 1:05` while it runs (`CI · running` before a start is reported), `CI · queued`, and
 * `CI failed`; `undefined` for a run the compact line does not show.
 */
export function ciChipText(ci: RightNowCi): string | undefined {
  switch (ci.status) {
    case 'in_progress':
      return ci.ageMs === undefined ? 'CI · running' : `CI · ${formatCiElapsed(ci.ageMs)}`;
    case 'queued':
      return 'CI · queued';
    case 'failure':
      return 'CI failed';
    case 'success':
    case 'cancelled':
    case 'unavailable':
      return undefined;
    default:
      return ci.status satisfies never;
  }
}

/** Whether the compact chip's clock face moves each second: the run is running with a known start. */
export function ciChipTicks(ci: RightNowCi): boolean {
  return ci.status === 'in_progress' && ci.ageMs !== undefined;
}

/** "main CI · 1a2b3c4 · 12m ago". */
export function ciDetail(ci: RightNowCi): string {
  const age = ciAge(ci);
  return [`${ci.branch} CI`, ...(ci.shortSha ? [ci.shortSha] : []), ...(age ? [age] : [])].join(' · ');
}
