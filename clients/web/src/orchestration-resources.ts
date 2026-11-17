/**
 * HS2-5GRXFN: resource contention (docs/26 §26.20) in words, from the scheduler's own answers only:
 * `scheduler explain`'s `resources` for a card and `scheduler plan`'s `contention` for the overview.
 * Pure. A claim's holder is never reported, so "held" says until when; a step given a resource in a pass
 * is named by its slug from the timeline.
 */
import { stepLabel } from './orchestration-scheduler';
import type { OrchestrationTimeline } from './orchestration-timeline';
import type { ClockFormat } from './orchestration-timeline-board';

export interface ResourceStandingView {
  namespace: string;
  resource: string;
  /** `free`, `held`, `taken`, `unreadable`, `unverified` or `not_read`. */
  state: string;
  expires_at_ms?: number;
  unreadable?: string;
  taken_by?: string;
}

export interface ResourceContentionView extends ResourceStandingView {
  /** The steps deferred for it, best first. */
  waiting: string[];
}

const text = (value: unknown) => (typeof value === 'string' ? value : undefined);
const words = (code: string) => code.replaceAll('_', ' ');

function standing(value: Record<string, unknown>): ResourceStandingView | undefined {
  if (typeof value.namespace !== 'string' || typeof value.resource !== 'string' || typeof value.state !== 'string')
    return undefined;
  return {
    namespace: value.namespace,
    resource: value.resource,
    state: value.state,
    expires_at_ms: typeof value.expires_at_ms === 'number' ? value.expires_at_ms : undefined,
    unreadable: text(value.unreadable),
    taken_by: text(value.taken_by),
  };
}

/** `scheduler explain`'s `resources`. */
export function parseRequiredResources(explanation: Record<string, unknown> | undefined): ResourceStandingView[] {
  const list = Array.isArray(explanation?.resources) ? (explanation.resources as Array<Record<string, unknown>>) : [];
  return list.flatMap((entry) => standing(entry) ?? []);
}

/** `scheduler plan`'s `contention`. */
export function parseContention(plan: Record<string, unknown> | undefined): ResourceContentionView[] {
  const list = Array.isArray(plan?.contention) ? (plan.contention as Array<Record<string, unknown>>) : [];
  return list.flatMap((entry) => {
    const found = standing(entry);
    if (!found) return [];
    const waiting = Array.isArray(entry.waiting) ? entry.waiting.filter((step) => typeof step === 'string') : [];
    return [{ ...found, waiting }];
  });
}

const name = (resource: ResourceStandingView) => `${resource.namespace}/${resource.resource}`;

/** How one resource stands, as a phrase: "held until about 15:40", "given to API-12 in this pass". */
export function resourceStateText(
  resource: ResourceStandingView,
  timeline: OrchestrationTimeline | undefined,
  clock: ClockFormat,
): string {
  switch (resource.state) {
    case 'free':
      return 'free';
    case 'held':
      return resource.expires_at_ms === undefined
        ? 'held'
        : `held until about ${clock(resource.expires_at_ms)} unless renewed`;
    case 'taken':
      return `given to ${stepLabel(resource.taken_by, timeline)} in this pass`;
    case 'unreadable':
      return `its claims could not be read${resource.unreadable ? ` (${words(resource.unreadable)})` : ''}`;
    case 'unverified':
      return 'its claims could not be read; work may start on a provisional claim';
    case 'not_read':
      return 'its machine was not read';
    default:
      return words(resource.state);
  }
}

/** Why It Is Not Running's resource rows: what the card needs, and anything that is not free. */
export function resourceRows(
  resources: readonly ResourceStandingView[],
  timeline: OrchestrationTimeline | undefined,
  clock: ClockFormat,
): Array<{ label: string; value: string }> {
  if (resources.length === 0) return [];
  const rows = [{ label: 'Needs', value: resources.map(name).join(', ') }];
  for (const resource of resources.filter((entry) => entry.state !== 'free'))
    rows.push({ label: name(resource), value: resourceStateText(resource, timeline, clock) });
  return rows;
}

/** The overview's Resources rows: each contended resource, how it stands and who waits. */
export function contentionRows(
  contention: readonly ResourceContentionView[],
  timeline: OrchestrationTimeline | undefined,
  clock: ClockFormat,
): Array<{ label: string; value: string }> {
  return contention.map((resource) => {
    const state = resourceStateText(resource, timeline, clock);
    const waiting = resource.waiting.map((step) => stepLabel(step, timeline));
    const waits = waiting.length === 0 ? '' : ` · ${waiting.join(', ')} ${waiting.length === 1 ? 'waits' : 'wait'}`;
    return { label: name(resource), value: `${state.charAt(0).toUpperCase()}${state.slice(1)}${waits}` };
  });
}
