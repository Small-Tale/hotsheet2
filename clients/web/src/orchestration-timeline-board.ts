/**
 * HS2-J89JC7: how the orchestration timeline reads on the production ticket board. Pure: it turns the
 * server's waves (oldest first) into board columns with the newest on the left and Done on the right
 * (maintainer: "oldest (like done) should be rightmost and newest should be leftmost"), and each item
 * into the shared ticket-summary props with its orchestration line, grouped by project.
 */
import type { SemanticTone } from '@kerfjs/ui/badge';

import type { TicketStatus } from './components/status-badge';
import type { TicketBoardColumnProps } from './components/ticket-board-column';
import type { TicketRowOrchestration, TicketRowProps } from './components/ticket-row';
import type {
  OrchestrationTargetLine,
  OrchestrationTimeline,
  TimelineItem,
  TimelineWave,
} from './orchestration-timeline';
import { priorityFromWire } from './priority-wire';
import { provisionalLine } from './worker-provisional';

/** Formats a time of day, such as "15:00". Injected so tests are independent of the locale. */
export type ClockFormat = (ms: number) => string;

export const clockTime: ClockFormat = (ms) =>
  new Date(ms).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });

/** The prefix every orchestration card's qualified id carries, so selection never meets a ticket of this project. */
export const ORCHESTRATION_CARD_PREFIX = 'orchestration:';
export const orchestrationCardId = (stepId: string) => `${ORCHESTRATION_CARD_PREFIX}${stepId}`;
export const stepOfCard = (qualifiedId: string | undefined) =>
  qualifiedId?.startsWith(ORCHESTRATION_CARD_PREFIX) ? qualifiedId.slice(ORCHESTRATION_CARD_PREFIX.length) : undefined;

const ORDINALS = ['First', 'Second', 'Third', 'Fourth', 'Fifth', 'Sixth', 'Seventh', 'Eighth', 'Ninth', 'Tenth'];

const TOOL_NAMES: Record<string, string> = { claude: 'Claude', codex: 'Codex', gemini: 'Gemini' };
export const toolLabel = (tool: string | undefined) =>
  tool ? (TOOL_NAMES[tool] ?? tool.charAt(0).toUpperCase() + tool.slice(1)) : undefined;

/** The machine an item runs on: the paired server's label, this machine, or not bound here. */
export function machineLabel(item: Pick<TimelineItem, 'location' | 'machine_label'>): string {
  if (item.location === 'local') return 'This machine';
  if (item.location === 'remote') return item.machine_label ?? 'Remote server';
  return 'Not bound on this machine';
}

/** The ticket's display identity: its slug, or a short form of its native id. */
export const itemSlug = (item: TimelineItem) => item.slug ?? item.target.native_id.slice(-6);

/** A prerequisite named on a card: its slug, or the step's ticket. */
const firstWait = (item: TimelineItem) => item.waits_on?.[0]?.slug ?? item.waits_on?.[0]?.native_id.slice(-6);

/** The state words and tone of an item's orchestration line; running and done say it through the status badge. */
export function stateWords(item: TimelineItem, proposed: boolean): { label: string; tone: SemanticTone } | undefined {
  switch (item.state) {
    case 'running':
    case 'done':
      return undefined;
    case 'provisional':
      return { label: firstWait(item) ? `Provisional on ${firstWait(item)}` : 'Provisional', tone: 'info' };
    case 'ready':
      return { label: 'Ready', tone: 'success' };
    case 'waits_for_prerequisite': {
      const wait = firstWait(item);
      if (proposed) return { label: wait ? `After ${wait}` : 'After its prerequisites', tone: 'neutral' };
      return { label: wait ? `Waits on ${wait}` : 'Waits on a prerequisite', tone: 'neutral' };
    }
    case 'waits_for_permission':
      return { label: 'Waits for permission', tone: 'warning' };
    case 'waits_for_decision':
      return {
        label: item.goal_slug ? `Waits for your decision on ${item.goal_slug}` : 'Waits for your decision',
        tone: 'warning',
      };
    default:
      return item.state satisfies never;
  }
}

/**
 * HS2-H5H9QP: work started ahead of a pending gate words its ledger state exactly as a worker's card does
 * (HS2-N8H97X), through the same `provisionalLine`: provisional with each gate's CI result, at risk naming
 * the failed prerequisites to repair first, or reopened. Confirmed work, or no record, says nothing here.
 */
function ledgerWords(item: TimelineItem): { label: string; tone: SemanticTone } | undefined {
  const record = item.provisional;
  if (!record) return undefined;
  const gates = record.gates.map((gate) => ({ slug: gate.slug ?? gate.native_id.slice(-6), result: gate.result })),
    line = provisionalLine(
      {
        qualified: `${item.target.connection_id}:${item.target.native_id}`,
        slug: itemSlug(item),
        state: record.state,
        gates,
      },
      gates.filter((gate) => gate.result === 'red').map((gate) => gate.slug),
    );
  return line?.state && line.stateTone ? { label: line.state, tone: line.stateTone } : undefined;
}

/** The badge an inspector shows for an item's state. */
export function stateBadge(item: TimelineItem): { label: string; tone: SemanticTone } {
  switch (item.state) {
    case 'running':
      return { label: 'Running', tone: 'pop' };
    case 'provisional':
      return { label: 'Provisional', tone: 'info' };
    case 'ready':
      return { label: 'Ready', tone: 'success' };
    case 'waits_for_prerequisite':
      return { label: 'Waits on a prerequisite', tone: 'neutral' };
    case 'waits_for_permission':
      return { label: 'Waits for permission', tone: 'warning' };
    case 'waits_for_decision':
      return { label: 'Waits for your decision', tone: 'warning' };
    case 'done':
      return { label: 'Done', tone: 'neutral' };
    default:
      return item.state satisfies never;
  }
}

function relative(ms: number, from: number): string {
  const minutes = Math.round((ms - from) / 60_000);
  if (minutes < 60) return `About +${Math.max(minutes, 1)} min`;
  return `About +${Math.round(minutes / 60)} h`;
}

const sameDay = (a: number, b: number) => new Date(a).toDateString() === new Date(b).toDateString();

function about(ms: number, now: number, clock: ClockFormat): string {
  return sameDay(ms, now) ? `About ${clock(ms)}` : `Tomorrow ${clock(ms)}`;
}

/** A column title: the wave's name and its estimated start. */
export function waveTitle(wave: TimelineWave, timeline: OrchestrationTimeline, clock: ClockFormat = clockTime): string {
  const at = wave.estimated_start_ms,
    now = timeline.now_ms;
  switch (wave.kind) {
    case 'done':
      return 'Done';
    case 'working_now':
      return 'Working Now';
    case 'next':
      return at === undefined ? 'Next' : `Next · ${about(at, now, clock)}`;
    case 'then':
      return at === undefined ? 'Then' : `Then · ${about(at, now, clock)}`;
    case 'later':
      return at === undefined ? 'Later' : `Later · ${about(at, now, clock)}`;
    case 'planned': {
      const name = ORDINALS[wave.ordinal] ?? `Wave ${wave.ordinal + 1}`;
      return wave.ordinal === 0 || at === undefined ? `${name} · On Start` : `${name} · ${relative(at, now)}`;
    }
    default:
      return wave.kind satisfies never;
  }
}

const STATUSES: readonly TicketStatus[] = ['not_started', 'started', 'completed', 'verified'];

function cardStatus(item: TimelineItem): TicketStatus {
  if (item.state === 'done') return item.status === 'verified' ? 'verified' : 'completed';
  if (item.state === 'running' || item.state === 'provisional') return 'started';
  const known = STATUSES.find((status) => status === item.status);
  return known === 'completed' || known === 'verified' ? 'not_started' : (known ?? 'not_started');
}

/**
 * The orchestration line of an item, as every ticket row shows it: the state in words with a tone,
 * where it runs (machine · tool) and the goal ticket. Shared by the timeline's cards and, through
 * {@link targetLineOrchestration}, a target project's List and Columns (HS2-S3FKJ7).
 */
export function orchestrationLine(item: TimelineItem, proposed: boolean): TicketRowOrchestration {
  const words = ledgerWords(item) ?? stateWords(item, proposed),
    tool = toolLabel(item.tool);
  return {
    state: words?.label,
    stateTone: words?.tone,
    placement: tool ? `${machineLabel(item)} · ${tool}` : machineLabel(item),
    goal: item.goal_slug,
    goalTitle: item.goal_title,
  };
}

/** HS2-S3FKJ7: the key of a target ticket in its own project, `connection_id:native_id`. */
export const targetLineKey = (connectionId: string, nativeId: string) => `${connectionId}:${nativeId}`;

/**
 * HS2-S3FKJ7: a target ticket's orchestration line in its own project. The goal ticket belongs to the
 * orchestration project, so its tooltip names that project.
 */
export function targetLineOrchestration(line: OrchestrationTargetLine): TicketRowOrchestration {
  const base = orchestrationLine(line.item, line.proposed),
    goalTitle = line.item.goal_title ? `${line.item.goal_title}, in ${line.owner_label}` : `In ${line.owner_label}`;
  return { ...base, goalTitle };
}

/** HS2-S3FKJ7: a target project's lines by ticket key. */
export function targetLineMap(lines: readonly OrchestrationTargetLine[]): ReadonlyMap<string, TicketRowOrchestration> {
  return new Map(
    lines.map((line) => [
      targetLineKey(line.item.target.connection_id, line.item.target.native_id),
      targetLineOrchestration(line),
    ]),
  );
}

/** The card's trailing time: when it started or finished, or when it is expected to start. */
export function cardTime(item: TimelineItem, timeline: OrchestrationTimeline, clock: ClockFormat = clockTime): string {
  if (item.state === 'done') return item.finished_at_ms === undefined ? 'Done' : `Done ${clock(item.finished_at_ms)}`;
  if (item.started_at_ms !== undefined && (item.state === 'running' || item.state === 'provisional'))
    return `Since ${clock(item.started_at_ms)}`;
  const at = item.estimated_start_ms;
  if (at === undefined) return 'Not estimated';
  if (timeline.phase === 'proposed') return at <= timeline.now_ms ? 'On start' : relative(at, timeline.now_ms);
  return about(at, timeline.now_ms, clock);
}

/** The shared ticket-summary props of one timeline card. */
export function timelineCard(
  item: TimelineItem,
  timeline: OrchestrationTimeline,
  selectedStep: string | undefined,
  clock: ClockFormat = clockTime,
): TicketRowProps {
  const working = item.state === 'running' || item.state === 'provisional',
    tool = toolLabel(item.tool),
    slug = itemSlug(item),
    orchestration = orchestrationLine(item, timeline.phase === 'proposed');
  return {
    slug,
    qualifiedId: orchestrationCardId(item.step_id),
    title: item.title ?? `${slug} in ${item.project_label}`,
    status: cardStatus(item),
    startedPhase: working ? 'working' : undefined,
    priority: priorityFromWire(item.priority),
    category: item.category ?? 'task',
    tags: [],
    upNextEligible: false,
    blocked: item.state === 'waits_for_prerequisite' && timeline.phase !== 'proposed',
    busy: item.state === 'running' || item.state === 'provisional',
    agentName: tool,
    remoteClaim: working && item.location === 'remote',
    updatedLabel: cardTime(item, timeline, clock),
    selected: item.step_id === selectedStep,
    draggable: false,
    // HS2-6GE7WM: cards gather by the site the work runs on, which the group name describes.
    group: item.site_id ?? item.target.target_id,
    groupName: item.project_label,
    orchestration,
  };
}

const waveId = (wave: TimelineWave) => `orchestration-${wave.kind}${wave.kind === 'planned' ? `-${wave.ordinal}` : ''}`;

/**
 * The board's columns, newest on the left and Done on the right. In each column the cards are in the
 * order of the declared projects, so the shared group segments gather them by project.
 */
export function timelineColumns(
  timeline: OrchestrationTimeline,
  {
    selectedStep,
    collapsedGroups = [],
    clock = clockTime,
  }: { selectedStep?: string; collapsedGroups?: readonly string[]; clock?: ClockFormat } = {},
): TicketBoardColumnProps[] {
  return timeline.waves
    .map((wave) => ({
      id: waveId(wave),
      title: waveTitle(wave, timeline, clock),
      collapsedGroups,
      tickets: wave.items.map((item) => timelineCard(item, timeline, selectedStep, clock)),
    }))
    .reverse();
}

/** The id of the column a phone opens on: Working Now, or the first proposed wave. */
export function startColumnId(timeline: OrchestrationTimeline): string | undefined {
  const wave =
    timeline.waves.find((candidate) => candidate.kind === 'working_now') ??
    timeline.waves.find((candidate) => candidate.kind === 'planned' && candidate.ordinal === 0);
  return wave && waveId(wave);
}

export function findTimelineItem(timeline: OrchestrationTimeline | undefined, stepId: string | undefined) {
  if (!timeline || !stepId) return undefined;
  for (const wave of timeline.waves) {
    const found = wave.items.find((item) => item.step_id === stepId);
    if (found) return { item: found, wave };
  }
  return undefined;
}

/** The toolbar's account of where orchestration stands. */
export function timelineStatus(timeline: OrchestrationTimeline, clock: ClockFormat = clockTime): string {
  const sessions = `${timeline.sessions} ${timeline.sessions === 1 ? 'Session' : 'Sessions'}`;
  switch (timeline.phase) {
    case 'no_projects':
      return 'No Projects Yet';
    case 'no_plan':
      return 'No Plan Yet';
    case 'proposed':
      return 'Proposed Plan · Not Started';
    case 'running':
      return timeline.running_since_ms === undefined
        ? `Running · ${sessions}`
        : `Running Since ${clock(timeline.running_since_ms)} · ${sessions}`;
    case 'paused':
      return `Paused · ${sessions}`;
    default:
      return timeline.phase satisfies never;
  }
}

/** Why Start Orchestration is unavailable, in words. */
export function startBlockedReason(timeline: OrchestrationTimeline): string | undefined {
  switch (timeline.kick_off.start_blocked) {
    case undefined:
    case 'running':
      return undefined;
    case 'no_projects':
      return 'Add projects in Orchestration settings first';
    case 'no_plan':
      return 'There is no plan to start yet';
    case 'policy_absent':
      return 'Set the orchestration limits in Orchestration settings first';
    case 'project_software':
      return 'Change the Project Type to Orchestration in General settings first';
    default:
      return timeline.kick_off.start_blocked satisfies never;
  }
}
