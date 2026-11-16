/**
 * HS2-15G5T4: the readiness plan of one Orchestration view card in words (docs/26 §26.15), for the card
 * inspector's Plan section (design HS2-2XJSJK). Pure: it reads the timeline the view already long-polls,
 * so the section adds no request. Reason codes are stable codes; unknown ones read as their words.
 */
import type { OrchestrationTimeline, TimelineItem } from './orchestration-timeline';

const LEVELS: Readonly<Record<string, string>> = {
  confirmed: 'Confirmed',
  provisional: 'Provisional',
  blocked: 'Blocked',
  stale: 'Not confirmed',
  cyclic: 'In a cycle',
  in_progress: 'In progress',
  finished: 'Finished',
};

/** What a reason code says about the prerequisite it names, as a phrase after the prerequisite's slug. */
const REASONS: Readonly<Record<string, string>> = {
  prerequisite_not_finished: 'not finished yet',
  prerequisite_ticket_missing: 'its ticket no longer exists',
  prerequisite_in_cycle: 'it is in a cycle',
  gate_pending: 'integrated; its CI is still running',
  gate_failed: 'its CI failed',
  gate_not_linked: 'finished, but no CI gate is linked',
  gate_unknown: 'its CI result is unknown',
  target_unreachable: 'its project could not be reached',
  not_observed: 'not read yet',
  deployment_not_requested: 'no deployment was requested',
  deployment_pending: 'its deployment is pending',
  deployment_failed: 'its deployment failed',
  deployment_unknown: 'its deployment result is unknown',
  validation_step_missing: 'the validation step is missing',
  validation_not_run: 'the validation step has not run',
  validation_step_changed: 'the validation step changed since it ran',
  validation_failed: 'the validation step failed',
  validation_running: 'the validation step is running',
  validation_skipped: 'the validation step was skipped',
  validation_unavailable: 'the validation step could not run',
  validation_cancelled: 'the validation step was cancelled',
  validation_unverified: 'the validation result could not be confirmed',
  plan_cycle: 'its prerequisites lead back to this step',
};

const words = (code: string) => code.replaceAll('_', ' ');
const sentence = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

/** A readiness level in words: "Confirmed", "In a cycle", or the code's words for a newer level. */
export function readinessLevelText(level: string): string {
  return LEVELS[level] ?? sentence(words(level));
}

/** A reason code as a phrase, such as "its CI failed". */
export function readinessReasonText(code: string): string {
  return REASONS[code] ?? words(code);
}

/**
 * Why the step stands at its level: each prerequisite it waits on with its reason ("API-12: its CI is
 * still running"), then the step's own reasons that name no prerequisite ("Its prerequisites lead back
 * to this step" for a cycle). With `omitPrerequisites` the prerequisites are left out because another
 * section of the inspector already lists them. Empty when nothing stands against the step.
 */
export function readinessWhy(item: TimelineItem, options: { omitPrerequisites?: boolean } = {}): string[] {
  const waits = item.waits_on ?? [];
  const lines = options.omitPrerequisites
    ? []
    : waits.map((wait) => `${wait.slug ?? 'A prerequisite'}: ${readinessReasonText(wait.reason)}`);
  const named = new Set(waits.map((wait) => wait.reason));
  for (const code of item.reasons ?? [])
    if (!named.has(code) && !lines.includes(sentence(readinessReasonText(code))))
      lines.push(sentence(readinessReasonText(code)));
  return lines;
}

/** The unfinished steps that wait on this step's ticket, by their tickets' slugs (titles when no slug). */
export function readinessUnblocks(item: TimelineItem, timeline: OrchestrationTimeline): string[] {
  const labels: string[] = [];
  for (const other of timeline.waves.flatMap((wave) => wave.items)) {
    if (other.step_id === item.step_id || other.state === 'done') continue;
    if (!(other.waits_on ?? []).some((wait) => wait.step_id === item.step_id)) continue;
    const label = other.slug ?? other.title ?? `${other.project_label} ticket`;
    if (!labels.includes(label)) labels.push(label);
  }
  return labels;
}
