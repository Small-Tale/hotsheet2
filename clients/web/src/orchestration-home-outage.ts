/**
 * HS2-EX594V: a multi-site project's home site that cannot coordinate its project, as the Orchestration view
 * warns about it (docs/26 §26.30 "One pool per project"). There is no failover: no dispatcher or integrator
 * starts for the project anywhere until the home is back, and work on the other sites keeps the bounded
 * dispatcher wait and then claims its ticket for its own session. Pure: it words the timeline's
 * `home_outages`.
 */
import type { OrchestrationTimeline } from './orchestration-timeline';
import type { ClockFormat } from './orchestration-timeline-board';

export type HomeOutageReason = 'disabled' | 'unbound' | 'unreachable' | 'pool_unreadable';

/** One unavailable home, as `GET .../orchestration/timeline` answers it in `home_outages`. */
export interface TimelineHomeOutage {
  home_target_id: string;
  /** The project's other sites, where work still runs. */
  sites: string[];
  since_ms?: number;
  reason?: HomeOutageReason;
  /** The failed read's code, such as `unreachable`. */
  detail?: string;
  announced?: boolean;
  returned_at_ms?: number;
}

export const HOME_OUTAGE_REASONS: readonly HomeOutageReason[] = [
  'disabled',
  'unbound',
  'unreachable',
  'pool_unreadable',
];

/** One warning banner of the view. */
export interface HomeOutageBanner {
  key: string;
  title: string;
  detail: string;
}

const SETTINGS = 'Settings › Orchestration';

/** Why the home cannot coordinate, and what a person can do about it. */
const WORDS: Readonly<Record<HomeOutageReason, { why: string; next: string }>> = {
  disabled: {
    why: 'The orchestration policy disables it.',
    next: `Enable it in ${SETTINGS} to coordinate the project again.`,
  },
  unbound: {
    why: 'It has no usable binding on this machine.',
    next: `Bind it again in ${SETTINGS}.`,
  },
  unreachable: {
    why: 'Its machine cannot be reached.',
    next: 'Bring its machine back online, or check that the machines are still paired.',
  },
  pool_unreadable: {
    why: 'Its machine answered, but its coordination pool could not be read.',
    next: 'Check the coordination pool of its checkout on that machine.',
  },
};

/** Codes that say nothing a person can act on beyond the reason itself. */
const QUIET_DETAILS: ReadonlySet<string> = new Set(['refusal_backoff']);

const list = (labels: readonly string[]) =>
  labels.length <= 1
    ? (labels[0] ?? 'no other site')
    : `${labels.slice(0, -1).join(', ')} and ${labels[labels.length - 1]}`;

/** A target's label on the timeline, else its id. */
const labelOf = (timeline: OrchestrationTimeline, targetId: string) =>
  timeline.targets.find((target) => target.target_id === targetId)?.label ?? targetId;

/** The view's banner for each unavailable home, in the order the timeline lists them. */
export function homeOutageBanners(timeline: OrchestrationTimeline, clock: ClockFormat): HomeOutageBanner[] {
  return (timeline.home_outages ?? [])
    .filter((outage) => outage.since_ms !== undefined)
    .map((outage) => {
      const home = labelOf(timeline, outage.home_target_id),
        sites = outage.sites.map((site) => labelOf(timeline, site)),
        words = WORDS[outage.reason ?? 'unreachable'],
        code = outage.detail && !QUIET_DETAILS.has(outage.detail) ? ` (${outage.detail})` : '';
      return {
        key: outage.home_target_id,
        title: `Home Site of ${home} Is Unavailable`,
        detail: [
          `Since ${clock(outage.since_ms ?? timeline.now_ms)}. ${words.why}`,
          'No dispatcher or integrator starts for this project until it is back; there is no failover to another site.',
          `Work on ${list(sites)} still runs: each run waits a bounded time for a dispatcher, then claims its ticket for its own session.`,
          `${words.next}${code}`,
        ].join(' '),
      };
    });
}
