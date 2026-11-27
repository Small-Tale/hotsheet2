/**
 * HS2-77SRHC: the throughput scheduler (docs/26 §26.17) in words for the Orchestration view: why a ready
 * card is not running (`scheduler explain`), the overview's Scheduler and Latest Decisions sections, and
 * the Show All Decisions… dialog (`scheduler decisions` with `newest`). Pure; codes it does not know read
 * as their words. Identifiers and codes only, as the decision log holds.
 */
import { parseRequiredResources, type ResourceStandingView } from './orchestration-resources';
import type { OrchestrationTimeline, TimelineItem } from './orchestration-timeline';
import {
  type ClockFormat,
  machinePhrase,
  signedOutText,
  signedOutTool,
  toolLabel,
} from './orchestration-timeline-board';

/** One decision log record (`DecisionRecord`), as much of it as the view shows. */
export interface SchedulerDecision {
  seq: number;
  at_ms: number;
  tick_id: string;
  mode: string;
  kind: string;
  step_id?: string;
  /** HS2-65AD6J: the site a `role_unplaced` decision is about. */
  target_id?: string;
  tool?: string;
  reason?: string;
  detail?: string;
  outcome?: string;
}

/** A page of the decision log, newest first. */
export interface SchedulerDecisionPage {
  entries: SchedulerDecision[];
  /** The `before` of the page before this one. */
  cursor: number;
  more: boolean;
}

/** HS2-9FKTB3: one other site of the step's project and why the scheduler passed it over (`SitePass`). */
export interface SchedulerSitePass {
  target_id: string;
  reason: string;
  detail?: string;
}

/** What `scheduler explain` says about one step (`Explanation`), as much of it as the view shows. */
export interface SchedulerExplanation {
  standing: string;
  reason: string;
  detail?: string;
  retry_at_ms?: number;
  /** HS2-5GRXFN: every resource the work requires and how this pass saw it. */
  resources?: ResourceStandingView[];
  /** HS2-9FKTB3: the site of its project the work was placed on, when it was placed. */
  site?: string;
  /** HS2-9FKTB3: the project's other sites and why each was passed over. */
  sites?: SchedulerSitePass[];
}

const words = (code: string) => code.replaceAll('_', ' ');
const sentence = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

/** Why the scheduler did not place work, as a phrase. */
const REASONS: Readonly<Record<string, string>> = {
  target_cap: 'the project’s session limit is reached',
  machine_refused: 'its machine has no room',
  no_tool_available: 'no AI tool account can take it now',
  resource_held: 'a resource it needs is held',
  resource_unknown: 'a resource’s claims could not be read',
  cooldown: 'its project is cooling down after a recent start',
  refusal_backoff: 'its machine refused recently and is asked again later',
  policy_denied: 'the policy does not allow it',
  target_unavailable: 'its project could not be reached',
  provisional_not_dispatched: 'provisional work is not started under this policy',
  speculation_refused: 'the provisional rule refuses it',
  // HS2-ZY4Z4G: the detail names the requirement, such as `os:windows`.
  requirement_unmet: 'its project’s machine does not meet a requirement of the step',
  requirement_unknown: 'its project’s machine has not said whether it meets a requirement of the step',
  // HS2-ADASY5: the sessions started for its run ended before they took up the work, until the replacements ran out.
  never_joined: 'the sessions started for it ended before they took up the work, so none starts for a while',
  // HS2-36XDDX: its template waits for a person to accept the planner's proposed plan.
  template_plan_not_accepted: 'it waits for someone to accept the template’s plan',
  permission_required: 'it waits for a person’s permission',
  actor_required: 'the scheduler has not looked at it yet',
  // HS2-2Y535C: the explain answer's own codes for a step the pass did not rank.
  // HS2-D9KP86: Start Orchestration saves the default policy when none is saved.
  policy_absent: 'no policy is saved yet; Start Orchestration saves the default one',
  not_considered: 'the scheduler has not looked at it yet',
};

export function schedulerReasonText(code: string): string {
  return REASONS[code] ?? words(code);
}

/**
 * HS2-747WQA: why a pattern-only Models list resolved to no model at launch (HS2-0VTQFF), the detail of a
 * `no_tool_available` block on a step, a passed-over site or a role session the pass does not start. It says
 * the cause itself, so it stands in for the reason's own phrase.
 */
const MODEL_DETAILS: Readonly<Record<string, string>> = {
  model_unresolved: 'its machine has not reported the AI tool’s models, so no allowed model can be picked',
  no_matching_model: 'no model its machine reports is an allowed model',
};

/**
 * HS2-747WQA: a scheduler reason as a phrase, or its model-resolution detail in its place when it has one.
 * HS2-ZRPZJJ: a signed-out tool is named with its machine (`machine`, "its machine" when not known).
 */
export function schedulerReasonWithModel(code: string, detail: string | undefined, machine = 'its machine'): string {
  if (detail && MODEL_DETAILS[detail]) return MODEL_DETAILS[detail];
  const signedOut = signedOutTool(detail);
  if (signedOut) return signedOutText(signedOut, machine);
  // HS2-2Y535C: a policy block names the limit that denied it.
  const limit = code === 'policy_denied' && detail ? SITE_PASS_DETAILS[detail] : undefined;
  return limit ? `${schedulerReasonText(code)} (${limit})` : schedulerReasonText(code);
}

/** HS2-9FKTB3: why a site that could have taken the work was not chosen. */
const NOT_CHOSEN: Readonly<Record<string, string>> = {
  free_session: 'another site has a free session for it',
  preferred_tool: 'another site can start its preferred tool',
  fewer_sessions: 'another site’s machine runs fewer sessions',
  site_order: 'another site was as good and comes first',
};

/** HS2-9FKTB3: why one site of the project was passed over, as a phrase; `machine` names the site's machine. */
export function sitePassText(pass: SchedulerSitePass, machine = 'its machine'): string {
  if (pass.reason === 'not_chosen') return (pass.detail && NOT_CHOSEN[pass.detail]) || 'another site was chosen';
  if (pass.detail && MODEL_DETAILS[pass.detail]) return MODEL_DETAILS[pass.detail];
  const signedOut = signedOutTool(pass.detail);
  if (signedOut) return signedOutText(signedOut, machine);
  const reason = schedulerReasonText(pass.reason);
  const detail = sitePassDetail(pass);
  return detail ? `${reason} (${detail})` : reason;
}

/**
 * HS2-MFS7FY: the policy limit behind a `policy_denied` pass (`PolicyDenial::code`) and the other detail codes
 * a passed-over site can carry, in words. Row copy never shows an internal code (Kerf "Lists and selection").
 */
const SITE_PASS_DETAILS: Readonly<Record<string, string>> = {
  policy_absent: 'there is no policy',
  policy_target_not_permitted: 'the site is not one the policy lists',
  policy_target_disabled: 'the site is turned off in the policy',
  policy_tool_not_permitted: 'no AI tool it may use is allowed there',
  policy_model_not_permitted: 'its model is not allowed there',
  policy_load_unknown: 'the sessions already running could not be counted',
  policy_target_cap: 'the site’s session limit is reached',
  policy_machine_cap: 'the machine’s session limit is reached',
  policy_tool_cap: 'the AI tool’s session limit is reached',
  policy_account_not_permitted: 'no AI tool account it may use is allowed there',
  policy_account_cap: 'the AI tool account’s session limit is reached',
  policy_speculation_not_allowed: 'provisional work is not allowed there',
  policy_project_software: 'the project is a Software project',
  capacity_not_read: 'its machine has not been read yet',
  ...MODEL_DETAILS,
};

/**
 * HS2-MFS7FY: a pass's detail as words, or nothing. A requirement or resource reason's detail is the name the
 * step itself gives (such as `os:windows`), so it stays; any other code the view cannot word is left out.
 */
export function sitePassDetail(pass: SchedulerSitePass): string | undefined {
  if (!pass.detail) return undefined;
  const worded = SITE_PASS_DETAILS[pass.detail];
  if (worded) return worded;
  if (pass.reason.startsWith('requirement_') || pass.reason.startsWith('resource_')) return pass.detail;
  return undefined;
}

/** A site by its project label on the timeline; a short id when the timeline does not show it. */
export function siteLabel(targetId: string, timeline: OrchestrationTimeline | undefined): string {
  return timeline?.targets.find((target) => target.target_id === targetId)?.label ?? `site ${targetId.slice(-6)}`;
}

const STANDINGS: Readonly<Record<string, string>> = {
  start: 'It would start on the next pass',
  keep: 'A session keeps it',
  running: 'It is running',
  finished: 'It is finished',
  not_planned: 'It is not in the plan',
  not_ready: 'It is not ready',
};

/** HS2-ADASY5: the next step for a person when the sessions started for a step never take up its work. */
const NEVER_JOINED_NEXT =
  'Open the AI tool in a terminal of its project to check that it starts and is signed in, or work the ticket by hand';

/**
 * The Why It Is Not Running rows from `scheduler explain`, with the time of the scheduler's last pass. `item`
 * names the machine of the step's own site (HS2-ZRPZJJ); a passed-over site's machine comes from the timeline.
 */
export function explanationRows(
  explanation: SchedulerExplanation,
  clock: ClockFormat,
  lastPassMs?: number,
  timeline?: OrchestrationTimeline,
  item?: Pick<TimelineItem, 'location' | 'machine_label'>,
): Array<{ label: string; value: string }> {
  const machine = machinePhrase(item);
  const standing =
    explanation.standing === 'not_placed'
      ? `Waits: ${schedulerReasonWithModel(explanation.reason, explanation.detail, machine)}`
      : (STANDINGS[explanation.standing] ?? sentence(words(explanation.standing)));
  const rows = [
    { label: 'Scheduler', value: lastPassMs === undefined ? standing : `${standing} (last pass ${clock(lastPassMs)})` },
  ];
  // HS2-ZY4Z4G: name the requirement the machine misses, such as `os:windows`.
  if (explanation.reason.startsWith('requirement_') && explanation.detail)
    rows.push({ label: 'Requirement', value: explanation.detail });
  // HS2-9FKTB3: the site chosen among the project's sites, and why each other one was passed over.
  const passed = explanation.sites ?? [];
  if (explanation.site && passed.length > 0) rows.push({ label: 'Site', value: siteLabel(explanation.site, timeline) });
  for (const pass of passed) {
    const site = timeline?.targets.find((target) => target.target_id === pass.target_id);
    rows.push({
      label: `Not on ${siteLabel(pass.target_id, timeline)}`,
      value: sentence(sitePassText(pass, machinePhrase(site))),
    });
  }
  // HS2-ZRPZJJ: what a person can do while the step's AI tool is signed out on its machine.
  const signedOut = explanation.standing === 'not_placed' ? signedOutTool(explanation.detail) : undefined;
  if (signedOut)
    rows.push({
      label: 'What to do',
      value: `Sign in to ${toolLabel(signedOut) ?? signedOut} on ${machine}, or allow another AI tool for its project`,
    });
  // HS2-ADASY5: what a person can do while the step's sessions do not take up its work.
  if (explanation.reason === 'never_joined') rows.push({ label: 'What to do', value: NEVER_JOINED_NEXT });
  if (explanation.retry_at_ms !== undefined)
    rows.push({ label: 'Next', value: `Looked at again by ${clock(explanation.retry_at_ms)}` });
  else if (explanation.standing === 'not_placed')
    rows.push({ label: 'Next', value: 'Looked at again when something it waits on changes' });
  return rows;
}

/** A plan step by its ticket's slug on the timeline; a short step id when the timeline does not show it. */
export function stepLabel(stepId: string | undefined, timeline: OrchestrationTimeline | undefined): string {
  if (!stepId) return 'a step';
  const item = timeline?.waves.flatMap((wave) => wave.items).find((candidate) => candidate.step_id === stepId);
  return item?.slug ?? item?.title ?? `step ${stepId.slice(-6)}`;
}

/**
 * HS2-2YCHRC: the role of a role session's start, keep or wind-down decision, which names no step. An
 * integrator's decisions have reasons of their own; a dispatcher's (`role_needed`) name the role as the detail.
 */
function decisionRole(decision: SchedulerDecision): string | undefined {
  if (decision.reason?.startsWith('integrator_')) return 'integrator';
  if (decision.reason === 'role_needed' && decision.detail && BY_HAND[decision.detail]) return decision.detail;
  return undefined;
}

/** "the integrator of API: a handoff waits to be integrated", for a role session's decision. */
function roleDecisionText(decision: SchedulerDecision, role: string, timeline: OrchestrationTimeline | undefined) {
  const site = decision.target_id ? siteLabel(decision.target_id, timeline) : 'a project';
  // A dispatcher's `role_needed` detail is its role, not a reason to word.
  const why =
    decision.reason === 'role_needed'
      ? undefined
      : roleReasonPhrase({ standing: decision.kind, reason: decision.reason, detail: decision.detail });
  return { subject: `the ${role} of ${site}`, why: why ? `: ${why}` : '' };
}

/** One decision as a person reads it: "Started API-12 on Claude", "Deferred DOC-9: a resource it needs is held". */
export function decisionText(decision: SchedulerDecision, timeline: OrchestrationTimeline | undefined): string {
  const step = stepLabel(decision.step_id, timeline),
    dry = decision.mode === 'dry_run',
    role = decisionRole(decision);
  const forRole = role ? roleDecisionText(decision, role, timeline) : undefined;
  switch (decision.kind) {
    case 'tick':
      return dry ? 'Planned a pass (dry run)' : 'Ran a scheduling pass';
    case 'start': {
      const subject = forRole?.subject ?? step;
      if (decision.outcome === 'permission_required') return `${sentence(subject)} waits for permission`;
      const tool = toolLabel(decision.tool);
      return `${dry ? 'Would start' : 'Started'} ${subject}${tool ? ` on ${tool}` : ''}${forRole?.why ?? ''}`;
    }
    case 'keep':
      if (forRole) return `Kept ${forRole.subject}${forRole.why}`;
      return `Kept the session of ${step}${decision.reason ? `: ${words(decision.reason)}` : ''}`;
    case 'wind_down':
      if (forRole) return `${dry ? 'Would wind down' : 'Wound down'} ${forRole.subject}${forRole.why}`;
      return `${dry ? 'Would wind down' : 'Wound down'} a session${decision.reason ? `: ${words(decision.reason)}` : ''}`;
    case 'unplaced':
      return `Deferred ${step}${decision.reason ? `: ${schedulerReasonWithModel(decision.reason, decision.detail)}` : ''}`;
    case 'assignment':
      return `${step}: ${words(decision.outcome ?? decision.reason ?? 'assignment')}`;
    case 'reconcile':
      return `Reconciled ${step}: ${words(decision.outcome ?? decision.reason ?? 'acted')}`;
    case 'role_unplaced': {
      // HS2-65AD6J: a project's integrator or dispatcher this pass does not start, and why.
      const role = decision.reason === 'dispatcher_unavailable' ? 'dispatcher' : 'integrator';
      const site = decision.target_id ? siteLabel(decision.target_id, timeline) : 'a project';
      const why = roleReasonPhrase({ standing: 'not_started', reason: decision.reason, detail: decision.detail });
      return `Did not start the ${role} of ${site}${why ? `: ${why}` : ''}`;
    }
    default:
      return `${sentence(words(decision.kind))} ${step}`;
  }
}

/** The overview's Last Pass: when the newest pass ran and what it started and deferred. */
export function lastPassText(page: SchedulerDecisionPage, clock: ClockFormat): string | undefined {
  const tick = page.entries.find((entry) => entry.kind === 'tick');
  if (!tick) return undefined;
  const of = (kind: string) =>
    page.entries.filter((entry) => entry.tick_id === tick.tick_id && entry.kind === kind).length;
  const started = of('start'),
    deferred = of('unplaced');
  return `${clock(tick.at_ms)} · started ${started}, deferred ${deferred}${tick.mode === 'dry_run' ? ' (dry run)' : ''}`;
}

/** The latest decisions the overview lists: decisions about work, not the pass records themselves. */
export function latestDecisions(page: SchedulerDecisionPage, count = 3): SchedulerDecision[] {
  return page.entries.filter((entry) => entry.kind !== 'tick').slice(0, count);
}

/** Read a `scheduler decisions` page (newest first) leniently: a malformed entry is left out. */
export function parseDecisionPage(value: Record<string, unknown>): SchedulerDecisionPage {
  const entries = Array.isArray(value.entries) ? (value.entries as Array<Record<string, unknown>>) : [];
  const text = (field: unknown) => (typeof field === 'string' ? field : undefined);
  return {
    cursor: typeof value.cursor === 'number' ? value.cursor : 0,
    more: value.more === true,
    entries: entries.flatMap((entry) => {
      const record = (entry.record ?? entry) as Record<string, unknown>;
      if (typeof entry.seq !== 'number' || typeof record.at_ms !== 'number' || typeof record.kind !== 'string')
        return [];
      return [
        {
          seq: entry.seq,
          at_ms: record.at_ms,
          tick_id: text(record.tick_id) ?? '',
          mode: text(record.mode) ?? 'applied',
          kind: record.kind,
          step_id: text(record.step_id),
          target_id: text(record.target_id),
          tool: text(record.tool),
          reason: text(record.reason),
          detail: text(record.detail),
          outcome: text(record.outcome),
        },
      ];
    }),
  };
}

/** Read `scheduler explain`, or undefined when it is not that shape. */
/**
 * HS2-ADASY5: a card with a run whose session is gone and that a fresh pass does not start again; undefined for
 * one that runs or will start.
 */
export function heldExplanation(value: Record<string, unknown> | undefined): SchedulerExplanation | undefined {
  const explanation = parseExplanation(value);
  return explanation?.standing === 'not_placed' ? explanation : undefined;
}

export function parseExplanation(value: Record<string, unknown> | undefined): SchedulerExplanation | undefined {
  if (!value || typeof value.standing !== 'string' || typeof value.reason !== 'string') return undefined;
  const placement = value.placement as Record<string, unknown> | undefined;
  const sites = Array.isArray(value.sites) ? (value.sites as Array<Record<string, unknown>>) : [];
  return {
    standing: value.standing,
    reason: value.reason,
    detail: typeof value.detail === 'string' ? value.detail : undefined,
    retry_at_ms: typeof value.retry_at_ms === 'number' ? value.retry_at_ms : undefined,
    resources: parseRequiredResources(value),
    site: typeof placement?.target_id === 'string' ? placement.target_id : undefined,
    sites: sites.flatMap((pass) =>
      typeof pass.target_id === 'string' && typeof pass.reason === 'string'
        ? [
            {
              target_id: pass.target_id,
              reason: pass.reason,
              detail: typeof pass.detail === 'string' ? pass.detail : undefined,
            },
          ]
        : [],
    ),
  };
}

/*
 * HS2-65AD6J: why integration is or is not running (docs/26 §26.30, "Explainability"). A running card's
 * Integration section reads `scheduler explain`'s `integrator`, the project's integrator in a fresh pass; the
 * overview's Coordination Sessions section reads `scheduler plan`: each home's integrator placement and every
 * role session the pass does not start (`roles_unplaced`).
 */

/** A project's integrator, or a role session a project waits on, as one pass decides it. */
export interface RoleStandingView {
  /** The project's home site, where its role sessions start. */
  target_id: string;
  /** `integrator` or `dispatcher`. */
  role: string;
  /** `start`, `keep`, `wind_down`, `not_started` or `none`; another code reads as its words. */
  standing: string;
  reason?: string;
  detail?: string;
  session_id?: string;
  retry_at_ms?: number;
  /** `dry_run` when a start or wind-down shown here would not be applied by a pass. */
  mode?: string;
}

const textField = (field: unknown) => (typeof field === 'string' ? field : undefined);
const timeField = (field: unknown) => (typeof field === 'number' && field >= 0 ? field : undefined);

/** Read `scheduler explain`'s `integrator`, or undefined when the explanation has none or it is not that shape. */
export function parseIntegratorStanding(value: Record<string, unknown> | undefined): RoleStandingView | undefined {
  const found = value?.integrator as Record<string, unknown> | undefined;
  if (!found || typeof found.target_id !== 'string' || typeof found.standing !== 'string') return undefined;
  return {
    target_id: found.target_id,
    role: 'integrator',
    standing: found.standing,
    reason: textField(found.reason),
    detail: textField(found.detail),
    session_id: textField(found.session_id),
    retry_at_ms: timeField(found.retry_at_ms),
    mode: textField(value?.mode),
  };
}

/**
 * Read `scheduler plan`'s role sessions leniently: each home's integrator placement (start, keep or wind
 * down), then every role session the pass does not start, as the server's `IntegratorStanding::of` reads
 * them. A malformed entry is left out.
 */
export function parseRoleStandings(plan: Record<string, unknown> | undefined): RoleStandingView[] {
  const list = (field: unknown) => (Array.isArray(field) ? (field as Array<Record<string, unknown>>) : []);
  const mode = textField(plan?.mode);
  const placed = list(plan?.placements).flatMap((placement) =>
    placement.role === 'integrator' && typeof placement.target_id === 'string' && typeof placement.action === 'string'
      ? [
          {
            target_id: placement.target_id,
            role: 'integrator',
            standing: placement.action,
            reason: textField(placement.reason),
            detail: textField(placement.detail),
            session_id: textField(placement.session_id),
            retry_at_ms: timeField(placement.retry_at_ms),
            mode,
          },
        ]
      : [],
  );
  const unplaced = list(plan?.roles_unplaced).flatMap((item) =>
    typeof item.target_id === 'string' && typeof item.role === 'string' && typeof item.reason === 'string'
      ? [
          {
            target_id: item.target_id,
            role: item.role,
            standing: 'not_started',
            reason: item.reason,
            detail: textField(item.detail),
            retry_at_ms: timeField(item.retry_at_ms),
            mode,
          },
        ]
      : [],
  );
  return [...placed, ...unplaced];
}

/** What a role session is doing, as a short status. */
export function roleStatusText(standing: Pick<RoleStandingView, 'standing' | 'reason' | 'mode'>): string {
  const dry = standing.mode === 'dry_run';
  switch (standing.standing) {
    case 'start':
      return dry ? 'Would start on the next pass (dry run)' : 'Starts on the next pass';
    case 'keep':
      return 'Running';
    case 'wind_down':
      return dry ? 'Would wind down (dry run)' : 'Winding down';
    case 'not_started':
      return standing.reason?.endsWith('_unavailable') ? 'Unavailable' : 'Not started';
    case 'none':
      return 'Not running';
    default:
      return sentence(words(standing.standing));
  }
}

/** Why an integrator is still needed (`integrator_kept`) or was started (`integrator_started`). */
const NEEDS: Readonly<Record<string, string>> = {
  handoff_pending: 'a handoff waits to be integrated',
  queue: 'handoffs wait to be integrated',
  ci_pending: 'an integrated commit waits on its CI gate',
  worker_run: 'a worker it serves still has a run',
};

/** Why a role session that runs is kept or wound down, for the placement reasons that are not its role's own. */
const PLACEMENTS: Readonly<Record<string, string>> = {
  busy: 'it is busy',
  idle_grace: 'it is idle and kept a while in case work arrives',
  cooldown: 'its project started a session too recently to wind one down',
  awaiting_safe_point: 'it stops at its next safe point',
  winding_down: 'a wind-down was already asked for',
  human_takeover: 'a person took it over',
  not_owned: 'another orchestrator started it',
  idle: 'nothing has needed it for a while',
  policy_not_permitted: 'the policy no longer allows it',
  over_cap: 'its project runs more sessions than its limit allows',
  role_prompt_failed: 'its role prompt failed, so a new one starts on a later pass',
  never_joined: 'it did not join the project’s pool in time, so a new one starts on a later pass',
  run_cancelled: 'a person cancelled the run it works',
  role_needed: 'its project still needs it',
};

/** Why a role session is not started (`integrator_not_started`'s detail), beyond the scheduler's own reasons. */
const NOT_STARTED: Readonly<Record<string, string>> = {
  target_disabled: 'its project is turned off in the policy',
  policy_opt_out: 'the project’s Coordination Sessions leave the integrator out, so it integrates by hand',
};

/**
 * HS2-91AF8K: what a person can do while a role is unavailable, beside waiting for the scheduler to look again: see
 * why the AI tool does not take up its role (a sign-in or a first-run question shows in its terminal), or do the
 * role's work themselves.
 */
const BY_HAND: Readonly<Record<string, string>> = {
  integrator: 'integrate handoffs by hand',
  dispatcher: 'assign ready work by hand',
};

const unavailable = (role: string) =>
  `the ${role}s the scheduler started did not take up their role, so none starts for a while. ` +
  `Open the AI tool in a terminal of this project to check that it starts and is signed in, or ${BY_HAND[role]} until then`;

/** Why a role session stands as it does, as a phrase; undefined when the scheduler gives no reason. */
export function roleReasonPhrase(standing: Pick<RoleStandingView, 'standing' | 'reason' | 'detail'>) {
  const { reason, detail } = standing;
  if (standing.standing === 'none') return 'no handoff waits to be integrated';
  switch (reason) {
    case undefined:
      return undefined;
    case 'integrator_started':
    case 'integrator_kept':
      return (detail && NEEDS[detail]) || 'its project still needs it';
    case 'integrator_wound_down':
      return PLACEMENTS.idle;
    case 'integrator_not_started':
      if (!detail) return 'this pass could not start one';
      // HS2-747WQA: a model-resolution detail says why no AI tool can start it.
      if (MODEL_DETAILS[detail]) return MODEL_DETAILS[detail];
      return NOT_STARTED[detail] ?? schedulerReasonText(detail);
    case 'integrator_unavailable':
      return unavailable('integrator');
    case 'dispatcher_unavailable':
      return unavailable('dispatcher');
    case 'never_joined':
      // HS2-NPTKS8: kept once its replacements are used up; a person looks at it.
      return detail === 'replacements_exhausted'
        ? 'it did not join the project’s pool and its replacements are used up, so it is left for a person'
        : PLACEMENTS.never_joined;
    default:
      return PLACEMENTS[reason] ?? schedulerReasonText(reason);
  }
}

/**
 * The Integration rows of a running card: the integrator's status, why, the home site when it is not the
 * card's own, its session, and when the scheduler looks again.
 */
export function integratorRows(
  standing: RoleStandingView,
  clock: ClockFormat,
  timeline?: OrchestrationTimeline,
  cardSite?: string,
): Array<{ label: string; value: string }> {
  const rows = [{ label: 'Integrator', value: roleStatusText(standing) }];
  const why = roleReasonPhrase(standing);
  if (why) rows.push({ label: 'Why', value: sentence(why) });
  if (cardSite !== undefined && standing.target_id !== cardSite)
    rows.push({ label: 'Home Site', value: siteLabel(standing.target_id, timeline) });
  if (standing.session_id) rows.push({ label: 'Session', value: standing.session_id });
  if (standing.retry_at_ms !== undefined)
    rows.push({ label: 'Next', value: `Looked at again by ${clock(standing.retry_at_ms)}` });
  return rows;
}

/** The overview's Coordination Sessions rows: one per project role session, "Web Integrator: Not started: …". */
export function roleStandingRows(
  standings: readonly RoleStandingView[],
  clock: ClockFormat,
  timeline?: OrchestrationTimeline,
): Array<{ label: string; value: string }> {
  return standings.map((standing) => {
    const why = roleReasonPhrase(standing);
    const next = standing.retry_at_ms === undefined ? '' : `; looked at again by ${clock(standing.retry_at_ms)}`;
    return {
      label: `${siteLabel(standing.target_id, timeline)} ${sentence(words(standing.role))}`,
      value: `${roleStatusText(standing)}${why ? `: ${why}` : ''}${next}`,
    };
  });
}

/** HS2-PR984R: the site Where It Runs names, and the step's own site when the scheduler chose another. */
export interface WhereItRunsSite {
  label: string;
  location: TimelineItem['location'];
  machine_label?: string;
  own?: { label: string; location: TimelineItem['location'] };
}

/**
 * HS2-PR984R: the site the card's Where It Runs names. Once the scheduler placed or would place the step on another
 * site of its project than the one the card shows (`scheduler explain`'s `site`), that chosen site and its machine
 * lead, and `own` keeps the step's own site for a secondary row. Otherwise the card's own site, unchanged.
 */
export function chosenSite(
  item: Pick<TimelineItem, 'site_id' | 'target' | 'project_label' | 'location' | 'machine_label'>,
  timeline: OrchestrationTimeline | undefined,
  explanation: SchedulerExplanation | undefined,
): WhereItRunsSite {
  const shown = { label: item.project_label, location: item.location, machine_label: item.machine_label };
  const site = explanation?.site;
  if (!site || site === (item.site_id ?? item.target.target_id)) return shown;
  const chosen = timeline?.targets.find((target) => target.target_id === site);
  if (!chosen) return shown;
  return {
    label: chosen.label,
    location: chosen.location,
    machine_label: chosen.machine_label,
    own: { label: item.project_label, location: item.location },
  };
}
