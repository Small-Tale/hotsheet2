import { CircleCheck, CircleDashed, CircleSlash, CircleX, Clock, type IconNode, LoaderCircle } from 'lucide';

import { type CiDisplayStatus, formatDuration, type RightNowCi } from './stats-right-now';

/**
 * The `main` CI word and glyph per reported status, shared by the Stats Right now tile (HS2-E16ZEK)
 * and the project sidebar Live chips and rows (HS2-57BYEA), in the circle family of the status badges.
 * A run that cannot be verified has no presentation: it is not shown (HS2-D3JJ49).
 */
export const CI_PRESENTATION: Readonly<Record<CiDisplayStatus, { word: string; icon: IconNode; name: string }>> = {
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

/** The longest workflow or branch part of a chip's name before it is cut with an ellipsis. */
const NAME_PART_MAX = 16;

function namePart(text: string): string {
  return text.length <= NAME_PART_MAX ? text : `${text.slice(0, NAME_PART_MAX - 1)}…`;
}

/**
 * What tells several shown runs apart (HS2-D3JJ49): the workflow when the runs' workflows differ, otherwise
 * `CI`, after the branch when their branches differ. One run, or runs that differ in nothing, read `CI`.
 */
export interface CiRunNaming {
  workflows: boolean;
  branches: boolean;
}

/** Which parts tell `runs` apart (HS2-D3JJ49). */
export function ciRunNaming(runs: readonly RightNowCi[]): CiRunNaming {
  const distinct = (part: (ci: RightNowCi) => string) => new Set(runs.map(part)).size > 1;
  return {
    workflows: distinct((ci) => ci.workflow ?? 'CI'),
    branches: distinct((ci) => ci.branch),
  };
}

/** A shown run's short name on its chip (HS2-D3JJ49): `CI`, `Platforms`, `docs CI`, or `docs Platforms`. */
export function ciRunName(ci: RightNowCi, naming: CiRunNaming): string {
  const workflow = naming.workflows ? namePart(ci.workflow ?? 'CI') : 'CI';
  return naming.branches ? `${namePart(ci.branch)} ${workflow}` : workflow;
}

/** A run's full name in titles, rows and menus: `main CI`, or `main Platforms` when workflows differ. */
export function ciRunLabel(ci: RightNowCi, naming: CiRunNaming): string {
  return `${ci.branch} ${naming.workflows ? (ci.workflow ?? 'CI') : 'CI'}`;
}

/**
 * The compact Live chip's face for a non-trivial CI run (HS2-505PHC, docs/34-glance-surfaces.md §34.4):
 * `CI · 1:05` while it runs (`CI · running` before a start is reported), `CI · queued`, and `CI failed`;
 * `undefined` for a run the compact line does not show. With several chips, `name` tells them apart
 * (HS2-D3JJ49): `Platforms · 3:10`.
 */
export function ciChipText(ci: RightNowCi, name = 'CI'): string | undefined {
  switch (ci.status) {
    case 'in_progress':
      return ci.ageMs === undefined ? `${name} · running` : `${name} · ${formatCiElapsed(ci.ageMs)}`;
    case 'queued':
      return `${name} · queued`;
    case 'failure':
      return `${name} failed`;
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

/**
 * The project sidebar's expanded CI row (HS2-57BYEA): `main CI passed · 1a2b3c4` with `12m ago` trailing.
 * With several runs whose workflows differ, each row names its workflow (HS2-D3JJ49): `main Platforms running`.
 */
export function ciRow(
  ci: RightNowCi,
  naming: CiRunNaming = { workflows: false, branches: false },
): { label: string; trailing: string } {
  const word = CI_PRESENTATION[ci.status].word.toLowerCase();
  // HS2-D3JJ49: beside other runs the row reads like its chip (`Platforms running`), without the branch or
  // sha, so the longer name still fits the sidebar's narrow row.
  if (naming.workflows || naming.branches)
    return { label: `${ciRunName(ci, naming)} ${word}`, trailing: ciAge(ci) ?? '' };
  const sha = ci.shortSha ? ` · ${ci.shortSha}` : '';
  return {
    label: `${ciRunLabel(ci, naming)} ${word}${sha}`,
    trailing: ciAge(ci) ?? '',
  };
}

/** One stored run Clear CI Status forgets (HS2-7PSBHQ, HS2-D3JJ49): its checkout, report time, and pair. */
export interface CiClearTarget {
  checkout: string;
  reportedAtMs: number;
  branch?: string;
  workflow?: string;
  /** The run's name in the menu item, e.g. `main Platforms`. */
  label: string;
}

/**
 * What Clear CI Status would clear for a shown run (HS2-7PSBHQ, HS2-D3JJ49, docs/34-glance-surfaces.md): any
 * shown run that names its checkout and report time, so the server never clears a newer report than the one
 * the user saw. The branch and workflow name it among the checkout's runs.
 */
export function ciClearTarget(ci: RightNowCi, naming: CiRunNaming): CiClearTarget | undefined {
  if (ci.status === 'unavailable' || ci.checkout === undefined || ci.reportedAtMs === undefined) return undefined;
  return {
    checkout: ci.checkout,
    reportedAtMs: ci.reportedAtMs,
    ...(ci.reportedBranch === undefined ? {} : { branch: ci.reportedBranch }),
    ...(ci.workflow === undefined ? {} : { workflow: ci.workflow }),
    label: ciRunLabel(ci, naming),
  };
}

/** The data attributes a chip, row or menu item carries for its clear target (HS2-D3JJ49). */
export function ciClearAttributes(target: CiClearTarget): Record<string, string> {
  return {
    'data-ci-clear-checkout': target.checkout,
    'data-ci-clear-reported-at-ms': String(target.reportedAtMs),
    ...(target.branch === undefined ? {} : { 'data-ci-clear-branch': target.branch }),
    ...(target.workflow === undefined ? {} : { 'data-ci-clear-workflow': target.workflow }),
    'data-ci-clear-label': target.label,
  };
}

/**
 * The clear target an element carries in its data attributes (HS2-7PSBHQ, HS2-D3JJ49), or `undefined` when
 * the checkout or time is missing or the time is not a whole non-negative number.
 */
export function ciClearTargetOf(dataset: {
  ciClearCheckout?: string;
  ciClearReportedAtMs?: string;
  ciClearBranch?: string;
  ciClearWorkflow?: string;
  ciClearLabel?: string;
}): CiClearTarget | undefined {
  const { ciClearCheckout: checkout, ciClearReportedAtMs: reportedAtMs } = dataset;
  if (!checkout || reportedAtMs === undefined || !/^\d+$/u.test(reportedAtMs)) return undefined;
  return {
    checkout,
    reportedAtMs: Number(reportedAtMs),
    ...(dataset.ciClearBranch === undefined ? {} : { branch: dataset.ciClearBranch }),
    ...(dataset.ciClearWorkflow === undefined ? {} : { workflow: dataset.ciClearWorkflow }),
    label: dataset.ciClearLabel || 'CI',
  };
}

/** The menu item's label: `Clear CI Status`, naming the run when the menu lists several (HS2-D3JJ49). */
export function ciClearItemLabel(target: CiClearTarget, several: boolean): string {
  return several ? `Clear ${target.label} Status` : 'Clear CI Status';
}
