/**
 * HS2-HDHT5E: labels of remote target tickets on the Orchestration timeline. The timeline read labels a
 * target ticket from its store only when the target is a checkout on this machine; for a remote target
 * it has the slug the plan last read and no title. While the view is open, the client reads the missing
 * labels through the orchestration project's gated ticket read (the target's hosted ticket route) and
 * keeps them in memory only: nothing is written to this machine's records. Pure helpers.
 */
import type { OrchestrationTimeline, TimelineItem } from './orchestration-timeline';

/** What a remote target ticket's read gives the card: identifiers and display text only. */
export interface RemoteTicketLabel {
  slug?: string;
  title?: string;
}

/** The target ticket an item works, as the gated ticket read names it. */
export interface RemoteTicketRef {
  target_id: string;
  connection_id: string;
  native_id: string;
}

/** At most this many labels are read for one timeline version. */
export const MAX_REMOTE_LABEL_READS = 24;

export const remoteLabelKey = (ref: RemoteTicketRef) => `${ref.target_id}|${ref.connection_id}|${ref.native_id}`;

const needsLabel = (item: TimelineItem) => item.location === 'remote' && item.title === undefined;

/**
 * The remote target tickets whose title the timeline lacks and that are not `known` (read, failed or in
 * flight), each once, in timeline order, at most `limit`.
 */
export function missingRemoteLabels(
  timeline: OrchestrationTimeline | undefined,
  known: ReadonlySet<string>,
  limit = MAX_REMOTE_LABEL_READS,
): RemoteTicketRef[] {
  const wanted = new Map<string, RemoteTicketRef>();
  for (const wave of timeline?.waves ?? [])
    for (const item of wave.items) {
      if (wanted.size >= limit) return [...wanted.values()];
      const key = remoteLabelKey(item.target);
      if (needsLabel(item) && !known.has(key) && !wanted.has(key)) wanted.set(key, { ...item.target });
    }
  return [...wanted.values()];
}

/** The timeline with each remote item's missing slug and title filled from `labels`; the rest untouched. */
export function withRemoteLabels(
  timeline: OrchestrationTimeline | undefined,
  labels: Readonly<Record<string, RemoteTicketLabel>>,
): OrchestrationTimeline | undefined {
  if (!timeline || Object.keys(labels).length === 0) return timeline;
  const labelFor = (item: TimelineItem) => (needsLabel(item) ? labels[remoteLabelKey(item.target)] : undefined);
  if (!timeline.waves.some((wave) => wave.items.some((item) => labelFor(item)?.title))) return timeline;
  return {
    ...timeline,
    waves: timeline.waves.map((wave) =>
      wave.items.some((item) => labelFor(item)?.title)
        ? {
            ...wave,
            items: wave.items.map((item) => {
              const label = labelFor(item);
              return label?.title ? { ...item, title: label.title, slug: item.slug ?? label.slug } : item;
            }),
          }
        : wave,
    ),
  };
}

/** The label of a gated ticket read's answer (the provider-neutral ticket); `undefined` when it has none. */
export function remoteTicketLabel(ticket: unknown): RemoteTicketLabel | undefined {
  if (!ticket || typeof ticket !== 'object') return undefined;
  const { slug, title } = ticket as Record<string, unknown>;
  if (typeof title !== 'string' || title.length === 0) return undefined;
  return { title, slug: typeof slug === 'string' ? slug : undefined };
}

/** At most this many label reads are in flight at once. */
export const REMOTE_LABEL_CONCURRENCY = 4;

/**
 * Reads the missing labels of one open view: each remote ticket once, at most
 * {@link REMOTE_LABEL_CONCURRENCY} at a time and {@link MAX_REMOTE_LABEL_READS} per timeline. Labels
 * live in memory only; a failed read is not retried until `reset` (the view's project changes or it
 * closes). `onLabels` receives the whole label map after each read that found one.
 */
export function createRemoteLabelReader(options: {
  read: (ref: RemoteTicketRef, signal: AbortSignal) => Promise<unknown>;
  onLabels: (labels: Readonly<Record<string, RemoteTicketLabel>>) => void;
}) {
  let labels: Record<string, RemoteTicketLabel> = {};
  let known = new Set<string>();
  let queue: RemoteTicketRef[] = [];
  let active = 0;
  let controller = new AbortController();
  const pump = () => {
    while (active < REMOTE_LABEL_CONCURRENCY && queue.length > 0) {
      const ref = queue.shift()!,
        key = remoteLabelKey(ref),
        signal = controller.signal;
      active += 1;
      void options
        .read(ref, signal)
        .then((answer) => {
          if (signal.aborted) return;
          const label = remoteTicketLabel(answer);
          if (!label) return;
          labels = { ...labels, [key]: label };
          options.onLabels(labels);
        })
        .catch(() => {
          // Content-free: the card keeps its slug and the fallback title.
        })
        .finally(() => {
          if (signal.aborted) return;
          active -= 1;
          pump();
        });
    }
  };
  return {
    /** Read the labels `timeline` still lacks. */
    fill(timeline: OrchestrationTimeline | undefined) {
      const wanted = missingRemoteLabels(timeline, known);
      for (const ref of wanted) known.add(remoteLabelKey(ref));
      queue.push(...wanted);
      pump();
    },
    /** Forget every label and stop the reads in flight. */
    reset() {
      controller.abort();
      controller = new AbortController();
      labels = {};
      known = new Set();
      queue = [];
      active = 0;
    },
  };
}
