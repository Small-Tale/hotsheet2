import type { FullTicket, Note } from './api';
import type { TicketTimelineEntry } from './components/ticket-timeline';

export interface TimestampedTimelineEntry extends TicketTimelineEntry {
  timestamp: string;
}

const statusTransition = /^Status changed from (.+) to (.+)$/;
const startedPhaseTransition = /^Started phase changed from (.+) to (.+)$/;

function statusTransitionTitle(source: string, destination: string): string {
  if (destination === 'Not Started') return source === 'Backlog' ? 'Moved out of backlog' : 'Re-enqueued';
  if (destination === 'Backlog') return 'Moved to backlog';
  if (destination === 'Archive') return 'Archived';
  if (destination === 'Deleted') return 'Deleted';
  if (destination === 'Moved') return 'Moved';
  if (['Started', 'Completed', 'Verified'].includes(destination)) return destination;
  return `Moved to ${destination.toLowerCase()}`;
}

function noteEntry(note: Note, confidence?: number): TimestampedTimelineEntry {
  const [title] = note.text.split('\n');
  const transition = title.match(statusTransition);
  const phaseTransition = title.match(startedPhaseTransition);
  const conciseStatus = transition ? statusTransitionTitle(transition[1], transition[2]) : undefined;
  const concisePhase = phaseTransition?.[2] === 'Unspecified' ? 'Phase cleared' : phaseTransition?.[2];
  const headline = conciseStatus || concisePhase || timelineHeadline(note.summary?.trim() || title);
  return {
    id: note.id,
    timestamp: note.created_at,
    time: note.created_at,
    title: confidence === undefined ? headline : `${headline} · ${confidence}% confidence`,
    emphasized: note.kind === 'status' || Boolean(conciseStatus || phaseTransition),
  };
}

const chronological = (left: Note, right: Note) =>
  left.created_at.localeCompare(right.created_at) || left.id.localeCompare(right.id);

/** Keep the gap more precise than the event's coarse relative timestamp. */
export function timelineDuration(start: string, end: string): string | undefined {
  const elapsed = Date.parse(end) - Date.parse(start);
  if (!Number.isFinite(elapsed) || elapsed < 0) return undefined;
  if (elapsed < 180_000) return `${Math.floor(elapsed / 1_000)}s`;
  if (elapsed < 10_800_000) return `${Math.floor(elapsed / 60_000)}m`;
  if (elapsed < 259_200_000) return `${Math.floor(elapsed / 3_600_000)}h`;
  return `${Math.floor(elapsed / 86_400_000)}d`;
}

function transitionDestination(note: Note): string | undefined {
  if (note.kind !== 'activity') return undefined;
  return note.text.split('\n')[0].match(statusTransition)?.[2];
}

/** A reopen starts a new completion cycle: an automatic move back to active work or a
 * Not Working report (mirrors `ops::latest_confidence`, HS2-DWTJ43). */
function reopensTicket(note: Note): boolean {
  if (note.kind !== 'activity') return false;
  if (note.summary === 'Reported as not working') return true;
  const destination = transitionDestination(note);
  return destination === 'Not Started' || destination === 'Started';
}

/** Map each completion cycle's first Completed/Verified transition note to that cycle's
 * newest confidence score, so the timeline headline carries the score it was completed with. */
export function completionConfidenceByNote(notes: readonly Note[]): Map<string, number> {
  const scores = new Map<string, number>();
  let completion: Note | undefined;
  let score: number | undefined;
  const close = () => {
    if (completion && score !== undefined) scores.set(completion.id, score);
  };
  for (const note of [...notes].sort(chronological)) {
    if (reopensTicket(note)) {
      close();
      completion = undefined;
      score = undefined;
      continue;
    }
    if (note.confidence !== undefined) score = note.confidence;
    const destination = transitionDestination(note);
    if (!completion && (destination === 'Completed' || destination === 'Verified')) completion = note;
  }
  close();
  return scores;
}

function timelineHeadline(text: string): string {
  const plain =
    text
      .trim()
      .replace(/^#{1,6}\s+/, '')
      .replace(/[*_`~]/g, '')
      .replace(/\s+/g, ' ') || 'Ticket updated';
  if (plain.length <= 80) return plain;
  const clipped = plain.slice(0, 80);
  const boundary = clipped.lastIndexOf(' ');
  return `${clipped.slice(0, boundary >= 48 ? boundary : 80).trimEnd()}…`;
}

/** Build durable ticket history and backfill the lifecycle timestamps available on
 * legacy tickets that predate automatic status-transition activity notes. */
export function ticketTimelineEntries(ticket: FullTicket): TimestampedTimelineEntry[] {
  const verifiedAt = (ticket as FullTicket & { verified_at?: string }).verified_at;
  const confidence = completionConfidenceByNote(ticket.notes);
  const noteOrder = new Map(ticket.notes.map((note, index) => [note.id, index]));
  const noteById = new Map(ticket.notes.map((note) => [note.id, note]));
  const tiePriority = (entry: TimestampedTimelineEntry) => {
    const note = noteById.get(entry.id);
    if (!note) return -1;
    if (note.text.startsWith('Status changed from ')) return 0;
    if (note.text.startsWith('Started phase changed from ')) return 1;
    return 2;
  };
  const entries: TimestampedTimelineEntry[] = ticket.notes
    .filter((note) => note.kind === 'activity' || note.kind === 'status')
    .map((note) => noteEntry(note, confidence.get(note.id)));
  const recordedTransitionTimestamps = new Set(
    ticket.notes
      .filter((note) => note.kind === 'activity' && note.text.startsWith('Status changed from '))
      .map((note) => note.created_at),
  );
  const addLifecycle = (id: string, timestamp: string | undefined, title: string, dedupeTransition = false) => {
    if (!timestamp || (dedupeTransition && recordedTransitionTimestamps.has(timestamp))) return;
    entries.push({ id, timestamp, time: timestamp, title, emphasized: true });
  };
  addLifecycle(`${ticket.id}-created`, ticket.created_at, 'Ticket created');
  addLifecycle(`${ticket.id}-completed`, ticket.completed_at, 'Completed', true);
  addLifecycle(`${ticket.id}-verified`, verifiedAt, 'Verified', true);
  const sorted = entries.sort(
    (left, right) =>
      left.timestamp.localeCompare(right.timestamp) ||
      tiePriority(left) - tiePriority(right) ||
      (noteOrder.get(left.id) ?? -1) - (noteOrder.get(right.id) ?? -1) ||
      left.id.localeCompare(right.id),
  );
  return sorted.map((entry, index) => ({
    ...entry,
    durationToNext: sorted[index + 1] && timelineDuration(entry.timestamp, sorted[index + 1].timestamp),
  }));
}
