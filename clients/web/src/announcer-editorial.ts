import type { AnnouncerReelItem } from './announcer-reel';
import { agentName, type AnnouncedStep, announcerStep, HEADLINE_CATEGORIES } from './announcer-speech';

/**
 * The announcer's editorial policy (HS2-RKYCTP; `docs/15` §15.6). Like a sports announcer it calls
 * the plays and summarizes the footwork:
 *
 * - **Headlines** are said on their own, as soon as they arrive: an event recorded with `high`
 *   importance; a test or check run (with its pass or fail when the tool reports it); a build or
 *   dependency install; a commit, push, merge, or pull request; claiming, creating, releasing,
 *   completing, or changing the status of a ticket; a sub-agent, a question, or a finished plan; a
 *   turn end, decision, plan, block, permission request, or ticket status event; a failed command;
 *   and any other event whose prose reports a failure or an error.
 * - **Routine steps** (reading, searching, listing, Git status and diffs, edits, other programs and
 *   tool calls, notes) fold into one entry per agent (project, tool, and session) for up to
 *   {@link ANNOUNCER_GROUP_WINDOW_MS}, shown once as what was done ("codex looked through the code for
 *   overlay and edited reel.ts.") with every step still listed on screen. A headline from the same
 *   agent closes its open group first. Live narration does not say routine entries (HS2-G3HJZH,
 *   {@link announcerSpeaks}).
 *
 * The edit is pure and deterministic: the same reel, clock, and arrival times give the same entries.
 * Entries keep the reel's time order, and a group keeps its first step's key as it grows.
 */
export const ANNOUNCER_GROUP_WINDOW_MS = 45_000;
/** A group lists at most this many steps on screen; the rest are counted. */
const LISTED_STEPS = 12;

export type AnnouncerLevel = 'headline' | 'routine';

export interface AnnouncerEntry extends AnnouncerReelItem {
  /** The reel items this entry tells: one for a headline or a lone routine step. */
  members: readonly AnnouncerReelItem[];
  level: AnnouncerLevel;
  /** A settled entry can no longer grow, so live narration may say it. */
  settled: boolean;
}

export interface EditorialOptions {
  /** The current time in epoch milliseconds. */
  now: number;
  windowMs?: number;
  /** When this device received a live item; retained items fall back to their event time. */
  receivedAt?: (key: string) => number | undefined;
}

/** Prose that reports a failure: a headline whatever its kind. */
const FAILURE = /\b(?:fail(?:ed|s|ure)?|errors?|errored|crash(?:ed)?|panick?ed)\b/i;

/** Whether a reel item is a headline or a routine step (see the policy above). */
export function announcerLevel(
  item: AnnouncerReelItem,
  step: AnnouncedStep = announcerStep(item.event),
): AnnouncerLevel {
  if (item.event.importance === 'high') return 'headline';
  if (HEADLINE_CATEGORIES.has(step.category) || step.outcome === 'failed') return 'headline';
  // Inline code is a command or a name, not a report, so only the prose around it is read.
  if (item.event.kind !== 'command' && FAILURE.test(item.event.summary.replace(/`[^`]*`/g, ' '))) return 'headline';
  return 'routine';
}

interface Group {
  members: AnnouncerReelItem[];
  steps: AnnouncedStep[];
  start: number;
  closed: boolean;
}

/** The reel as the announcer tells it: headlines alone, routine steps folded per agent and window. */
export function editAnnouncerReel(reel: readonly AnnouncerReelItem[], options: EditorialOptions): AnnouncerEntry[] {
  const windowMs = options.windowMs ?? ANNOUNCER_GROUP_WINDOW_MS;
  const timeOf = (item: AnnouncerReelItem) => {
    const time = options.receivedAt?.(item.key) ?? Date.parse(item.event.ts);
    return Number.isFinite(time) ? time : 0;
  };
  const order: (Group | AnnouncerEntry)[] = [];
  const open = new Map<string, Group>();
  for (const item of reel) {
    const agent = [item.projectId, item.event.tool, item.event.session ?? ''].join('\u0000');
    const step = announcerStep(item.event);
    const current = open.get(agent);
    if (announcerLevel(item, step) === 'headline') {
      if (current) current.closed = true;
      open.delete(agent);
      order.push({ ...item, members: [item], level: 'headline', settled: true });
      continue;
    }
    const time = timeOf(item);
    if (current && time - current.start < windowMs) {
      current.members.push(item);
      current.steps.push(step);
      continue;
    }
    if (current) current.closed = true;
    const group: Group = { members: [item], steps: [step], start: time, closed: false };
    open.set(agent, group);
    order.push(group);
  }
  return order.map((slot) => {
    if (!('steps' in slot)) return slot;
    const settled = slot.closed || options.now - slot.start >= windowMs;
    return groupEntry(slot, settled);
  });
}

/** Turn boundaries: shown in the reel, never said, since "I finished a turn" says nothing (HS2-G3HJZH). */
const UNSPOKEN_KINDS: ReadonlySet<string> = new Set(['turn_start', 'turn_end']);

/**
 * Whether live narration says an entry aloud (HS2-G3HJZH). Routine steps, folded or alone, and turn
 * boundaries are minutiae: they stay in the reel to read and step through, and Play still says them
 * on request, but live narration passes over them and says only headlines.
 */
export function announcerSpeaks(entry: Pick<AnnouncerEntry, 'level' | 'members'>): boolean {
  if (entry.level !== 'headline') return false;
  return !entry.members.every((member) => UNSPOKEN_KINDS.has(member.event.kind));
}

/** When the earliest open group settles, for a runtime timer; undefined when none is open. */
export function nextSettleAt(entries: readonly AnnouncerEntry[], options: EditorialOptions): number | undefined {
  const windowMs = options.windowMs ?? ANNOUNCER_GROUP_WINDOW_MS;
  let earliest: number | undefined;
  for (const entry of entries) {
    if (entry.settled) continue;
    const first = entry.members[0];
    const start = options.receivedAt?.(first.key) ?? Date.parse(first.event.ts);
    const at = (Number.isFinite(start) ? start : 0) + windowMs;
    if (earliest === undefined || at < earliest) earliest = at;
  }
  return earliest;
}

function groupEntry(group: Group, settled: boolean): AnnouncerEntry {
  const [first] = group.members;
  if (group.members.length === 1) return { ...first, members: group.members, level: 'routine', settled };
  const last = group.members[group.members.length - 1];
  const phrase = groupPhrase(group.steps);
  // Spoken in the first person (HS2-G3HJZH); shown with the tool's name.
  const spoken = `I ${phrase}.`;
  const shown = `${agentName(first.event)} ${phrase}.`;
  const listed = group.members
    .slice(0, LISTED_STEPS)
    .map((member) => `- ${member.narration.replace(/\s+/g, ' ').trim()}`);
  const more = group.members.length - LISTED_STEPS;
  if (more > 0) listed.push(`- and ${more} more`);
  return {
    key: first.key,
    projectId: first.projectId,
    projectName: first.projectName,
    storeId: first.storeId,
    event: last.event,
    narration: `${shown}\n\n${listed.join('\n')}`,
    spoken,
    members: group.members,
    level: 'routine',
    settled,
  };
}

const EXPLORING = new Set(['search', 'read', 'inspect']);
const count = (amount: number, one: string, many: string) => (amount === 1 ? one : `${amount} ${many}`);

/** Join spoken parts as a list: "a", "a and b", "a, b and c". */
export function spokenList(parts: readonly string[]): string {
  if (parts.length <= 1) return parts.join('');
  return `${parts.slice(0, -1).join(', ')} and ${parts.at(-1)}`;
}

/** What a run of routine steps did, by intent: changes first, then exploring, then the rest. */
export function groupPhrase(steps: readonly AnnouncedStep[]): string {
  const parts: string[] = [];
  const edits = steps.filter((step) => step.category === 'edit');
  if (edits.length) {
    const files = [...new Set(edits.flatMap((step) => step.files ?? []))];
    const unnamed = edits.some((step) => !step.files?.length);
    if (!unnamed && files.length <= 2) parts.push(`edited ${spokenList(files)}`);
    else if (files.length > 2) parts.push(`edited ${files.length} files`);
    else if (edits.length === 1) parts.push(edits[0].phrase);
    else parts.push('changed files');
  }
  const exploring = steps.filter((step) => EXPLORING.has(step.category));
  if (exploring.length === 1) parts.push(exploring[0].phrase);
  else if (exploring.length > 1) {
    const term = exploring.find((step) => step.term)?.term;
    parts.push(term ? `looked through the code for ${term}` : 'looked through the code');
  }
  const tickets = [...new Set(steps.filter((step) => step.category === 'ticket-work').map((step) => step.phrase))];
  if (tickets.length === 1) parts.push(tickets[0]);
  else if (tickets.length > 1) parts.push('worked on tickets');
  const ran = [
    ...new Set(
      steps
        .filter((step) => ['run', 'web', 'tool', 'other'].includes(step.category) && step.phrase)
        .map((step) => step.phrase),
    ),
  ];
  parts.push(...ran.slice(0, 2));
  // Steps known only by their prose (a custom tool-call summary) are counted, never read out.
  const leftover = Math.max(0, ran.length - 2) + steps.filter((step) => !step.phrase).length;
  if (leftover)
    parts.push(
      `took ${count(leftover, parts.length ? 'one other step' : 'a step', parts.length ? 'other steps' : 'steps')}`,
    );
  return spokenList(parts) || 'kept working';
}
