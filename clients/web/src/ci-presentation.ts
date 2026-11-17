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

/** "main CI · 1a2b3c4 · 12m ago". */
export function ciDetail(ci: RightNowCi): string {
  const age = ciAge(ci);
  return [`${ci.branch} CI`, ...(ci.shortSha ? [ci.shortSha] : []), ...(age ? [age] : [])].join(' · ');
}
