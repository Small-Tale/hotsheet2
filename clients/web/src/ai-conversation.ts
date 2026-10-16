import type {
  ActivityEvent,
  ClientFileReference,
  ClientTurnEvent,
  InFlightTool,
  ToolConnection,
  TurnStreamEnvelope,
} from './api';

export type ConversationMessageRole = 'user' | 'assistant';
export type ConversationMessageStatus = 'streaming' | 'completed' | 'failed' | 'interrupted';
export interface ConversationUsage {
  tokensIn: number;
  tokensOut: number;
  costUsd?: number;
  model?: string;
}
export type ConversationFileReference = ClientFileReference;
export interface ConversationMessage {
  id: string;
  role: ConversationMessageRole;
  content: string;
  status?: ConversationMessageStatus;
  usage?: ConversationUsage;
  files?: ConversationFileReference[];
  sequence?: number;
}
export interface ConversationActivity {
  id: string;
  tool: string;
  kind: string;
  summary: string;
  importance: 'low' | 'normal' | 'high';
  sequence?: number;
}
/** The tool call a running turn waits on (HS2-XV1VCR): its oldest open call and the open count. */
export interface ConversationInFlightTool {
  name: string;
  startedAt: number;
  count: number;
}
/**
 * A running turn's timing (HS2-XV1VCR), in Unix milliseconds. Local turns start it when sent;
 * stream events advance `lastEventAt` and replace `inFlightTool`; a turn another client started
 * is tracked from its first event. Cleared when the turn ends.
 */
export interface ConversationTurnTiming {
  turnId?: string;
  startedAt?: number;
  lastEventAt?: number;
  inFlightTool?: ConversationInFlightTool;
  /**
   * Whether `lastEventAt` came from a server-stamped event (HS2-GA5ASE). Until one arrives it is the
   * send time this client recorded, which a skewed remote clock would put ahead of the server's.
   */
  serverStamped?: boolean;
}
export interface ConversationState {
  messages: ConversationMessage[];
  activity?: ConversationActivity[];
  activeAssistantId?: string;
  /** Correlation id of the running turn (HS2-14X4AF): stream events naming another turn are stale. */
  activeTurnId?: string;
  progress?: string;
  /** The running turn's elapsed time, last activity, and in-flight tool (HS2-XV1VCR). */
  turnTiming?: ConversationTurnTiming;
  error?: string;
  nextSequence?: number;
  /** Messages a reopened non-contiguous saved selection left out before each message (HS2-BZB93V). */
  skippedBefore?: Readonly<Record<string, number>>;
}
export type ConversationTimelineGroup =
  { kind: 'message'; message: ConversationMessage } | { kind: 'activity'; activity: ConversationActivity[] };

export const EMPTY_CONVERSATION: ConversationState = { messages: [] };

/** Local and remote retries clear stale halt feedback while the next turn is running. */
export function conversationError(
  state: ConversationState,
  connection?: { busy?: boolean; last_error?: string },
): string | undefined {
  if (state.activeAssistantId || connection?.busy) return;
  return state.error ?? connection?.last_error;
}

/**
 * Settle a turn whose terminal stream event was missed while disconnected or reloading. A
 * connection that is busy with a different correlated turn (HS2-14X4AF) also proves this turn
 * ended: the server runs one turn per connection at a time.
 */
export function reconcileConversationConnection(
  state: ConversationState,
  connection: { busy?: boolean; last_error?: string; turn_id?: string },
  pendingStart = false,
): ConversationState {
  if (!state.activeAssistantId || pendingStart) return state;
  const replaced =
    connection.busy === true &&
    connection.turn_id !== undefined &&
    state.activeTurnId !== undefined &&
    connection.turn_id !== state.activeTurnId;
  if (replaced) return applyConversationEvent(state, { type: 'done', reason: 'interrupted' });
  if (connection.busy !== false) return state;
  return applyConversationEvent(
    state,
    connection.last_error
      ? { type: 'done', reason: 'failed', message: connection.last_error }
      : { type: 'done', reason: 'interrupted' },
  );
}

function normalizeConversationSequence(state: ConversationState): ConversationState {
  const maximum = Math.max(
    -1,
    ...state.messages.map((item) => item.sequence ?? -1),
    ...(state.activity ?? []).map((item) => item.sequence ?? -1),
  );
  let next = Math.max(state.nextSequence ?? 0, maximum + 1),
    changed = state.nextSequence !== next;
  const messages = state.messages.map((message) => {
    if (message.sequence !== undefined || (message.id === state.activeAssistantId && !message.content)) return message;
    changed = true;
    return { ...message, sequence: next++ };
  });
  const activity = (state.activity ?? []).map((item) => {
    if (item.sequence !== undefined) return item;
    changed = true;
    return { ...item, sequence: next++ };
  });
  return changed || state.nextSequence !== next
    ? { ...state, messages, ...(state.activity ? { activity } : {}), nextSequence: next }
    : state;
}

/** Merge transcript records by receipt order while grouping adjacent activity summaries. */
export function conversationTimeline(
  messages: readonly ConversationMessage[],
  activity: readonly ConversationActivity[] = [],
): ConversationTimelineGroup[] {
  const maximum = Math.max(
    -1,
    ...messages.map((item) => item.sequence ?? -1),
    ...activity.map((item) => item.sequence ?? -1),
  );
  let fallback = maximum + 1,
    index = 0;
  const entries = [
    ...messages.map((message) => ({
      kind: 'message' as const,
      message,
      order: message.sequence ?? fallback++,
      index: index++,
    })),
    ...activity.map((item) => ({
      kind: 'activity' as const,
      activity: item,
      order: item.sequence ?? fallback++,
      index: index++,
    })),
  ].sort((left, right) => left.order - right.order || left.index - right.index);
  const groups: ConversationTimelineGroup[] = [];
  for (const entry of entries) {
    const previous = groups.at(-1);
    if (entry.kind === 'activity' && previous?.kind === 'activity') previous.activity.push(entry.activity);
    else
      groups.push(
        entry.kind === 'message'
          ? { kind: 'message', message: entry.message }
          : { kind: 'activity', activity: [entry.activity] },
      );
  }
  return groups;
}

/**
 * Whether a stream event correlated to `turnId` belongs to this conversation's running turn.
 * Uncorrelated events (older servers) always fall back to the active turn; a correlated event
 * for a turn that is no longer running here, or for another turn, is a late or repeated
 * delivery and must not touch the transcript (HS2-14X4AF).
 */
export function acceptsConversationTurnEvent(state: ConversationState, turnId?: string): boolean {
  if (turnId === undefined) return true;
  if (!state.activeAssistantId) return false;
  return state.activeTurnId === undefined || state.activeTurnId === turnId;
}

/** Begin a local turn. `id` is also the turn's correlation id sent with the request. */
export function beginConversationTurn(
  state: ConversationState,
  id: string,
  content: string,
  now = Date.now(),
): ConversationState {
  const ordered = normalizeConversationSequence(state),
    assistantId = `${id}-assistant`,
    sequence = ordered.nextSequence ?? 0;
  return {
    ...ordered,
    messages: [
      ...ordered.messages,
      { id, role: 'user', content, sequence },
      { id: assistantId, role: 'assistant', content: '', status: 'streaming' },
    ],
    nextSequence: sequence + 1,
    activeAssistantId: assistantId,
    activeTurnId: id,
    progress: 'Reviewing the project and planning the next steps…',
    turnTiming: { turnId: id, startedAt: now, lastEventAt: now },
    error: undefined,
  };
}

export const EMPTY_TURN_CONTENT = 'The turn ended without output.';

/**
 * What an assistant bubble says when its turn produced no text (HS2-M5GN19). A turn whose
 * tools acted (activity recorded after its prompt) summarizes those actions; only a turn with
 * neither text nor activity keeps the empty-turn placeholder.
 */
export function emptyTurnContent(state: ConversationState, assistantIndex: number): string {
  const prompt = assistantIndex > 0 ? state.messages[assistantIndex - 1] : undefined,
    since = prompt?.role === 'user' ? (prompt.sequence ?? -1) : Number.POSITIVE_INFINITY,
    summaries = [
      ...new Set(
        (state.activity ?? [])
          .filter((item) => (item.sequence ?? -1) > since)
          .map((item) => item.summary.trim())
          .filter(Boolean),
      ),
    ];
  if (!summaries.length) return EMPTY_TURN_CONTENT;
  const shown = summaries.slice(0, 3),
    more = summaries.length - shown.length,
    count = summaries.length === 1 ? '1 action' : `${summaries.length} actions`;
  return `Finished without a written reply after ${count}: ${shown.join('; ')}${more ? `; and ${more} more` : ''}.`;
}

function nativeProgress(payload: unknown): string | undefined {
  if (!payload || typeof payload !== 'object') return;
  const value = payload as Record<string, unknown>;
  for (const key of ['summary', 'message', 'title', 'kind'])
    if (typeof value[key] === 'string' && value[key].trim()) return value[key].trim().slice(0, 160);
}

function inFlightTool(value: unknown): ConversationInFlightTool | undefined {
  if (!value || typeof value !== 'object') return;
  const tool = value as Partial<InFlightTool>;
  if (typeof tool.name !== 'string' || !tool.name.trim() || typeof tool.started_at_ms !== 'number') return;
  return {
    name: tool.name.trim(),
    startedAt: tool.started_at_ms,
    count: typeof tool.count === 'number' && tool.count > 0 ? tool.count : 1,
  };
}

/**
 * Advance a turn's timing for one stream event (HS2-XV1VCR). A server that stamps `at_ms` also
 * reports the in-flight tool after the event, so its absence there means no tool is open; an
 * older server's events only refresh the last-activity time, at their receipt time. Events of a
 * turn this client never began track that turn from here, so another client's running turn is
 * timed too, while a late event for an earlier local turn changes nothing.
 */
function recordTurnEventTiming(
  state: ConversationState,
  event: ClientTurnEvent,
  turnId: string | undefined,
  timing: TurnEventTiming | undefined,
  receivedAt: number,
): ConversationState {
  const local = Boolean(state.activeAssistantId);
  if (local && !acceptsConversationTurnEvent(state, turnId)) return state;
  // A local turn's user message carries its turn id: a late event of a finished local turn is
  // not another client's running turn.
  if (!local && turnId !== undefined && state.messages.some((message) => message.id === turnId)) return state;
  const current = state.turnTiming,
    same = Boolean(current) && (turnId === undefined || current?.turnId === undefined || current.turnId === turnId);
  if (event.type === 'done') {
    if (local || !same) return state;
    const rest = { ...state };
    delete rest.turnTiming;
    return rest;
  }
  const stampedAt = typeof timing?.at_ms === 'number' ? timing.at_ms : undefined,
    at = stampedAt ?? receivedAt,
    base: ConversationTurnTiming = same && current ? current : { ...(turnId === undefined ? {} : { turnId }) },
    // The first server stamp replaces the locally recorded send time rather than racing it, so a
    // remote client whose clock runs ahead cannot hold the last activity in the server's future.
    previous = stampedAt !== undefined && !base.serverStamped ? undefined : base.lastEventAt,
    next: ConversationTurnTiming = { ...base, lastEventAt: Math.max(previous ?? at, at) };
  if (stampedAt !== undefined) {
    next.serverStamped = true;
    const tool = inFlightTool(timing?.in_flight_tool);
    if (tool) next.inFlightTool = tool;
    else delete next.inFlightTool;
  }
  return { ...state, turnTiming: next };
}

/** The server's per-event timing carried on a turn stream envelope (HS2-XV1VCR). */
export type TurnEventTiming = Pick<TurnStreamEnvelope, 'at_ms' | 'in_flight_tool'>;

export function applyConversationEvent(
  state: ConversationState,
  event: ClientTurnEvent,
  turnId?: string,
  timing?: TurnEventTiming,
  receivedAt = Date.now(),
): ConversationState {
  state = recordTurnEventTiming(state, event, turnId, timing, receivedAt);
  if (!acceptsConversationTurnEvent(state, turnId)) return state;
  const ordered = normalizeConversationSequence(state),
    active = ordered.activeAssistantId,
    index = active ? ordered.messages.findIndex((message) => message.id === active) : -1;
  if (event.type === 'permission_asked') return { ...state, progress: 'Waiting for permission…' };
  if (event.type === 'native_activity')
    return { ...state, progress: nativeProgress(event.payload) ?? 'Working through the requested changes…' };
  if (event.type === 'usage') {
    if (index < 0) return state;
    const tokensIn = typeof event.tokens_in === 'number' ? event.tokens_in : 0,
      tokensOut = typeof event.tokens_out === 'number' ? event.tokens_out : 0,
      costUsd = typeof event.cost_usd === 'number' ? event.cost_usd : undefined,
      model = typeof event.model === 'string' ? event.model : undefined;
    // A provider result without model work is not usage: never show `0 tokens` (HS2-M5GN19).
    if (tokensIn === 0 && tokensOut === 0 && !(costUsd !== undefined && costUsd > 0)) return state;
    const messages = ordered.messages.map((message, messageIndex) =>
      messageIndex === index
        ? {
            ...message,
            usage: {
              tokensIn: (message.usage?.tokensIn ?? 0) + tokensIn,
              tokensOut: (message.usage?.tokensOut ?? 0) + tokensOut,
              ...(costUsd !== undefined
                ? { costUsd: (message.usage?.costUsd ?? 0) + costUsd }
                : message.usage?.costUsd !== undefined
                  ? { costUsd: message.usage.costUsd }
                  : {}),
              model: model ?? message.usage?.model,
            },
          }
        : message,
    );
    return { ...ordered, messages };
  }
  if (event.type === 'output') {
    if (index < 0) return state;
    const content = typeof event.content === 'string' ? event.content : '',
      sequence = ordered.nextSequence ?? 0;
    const incoming = Array.isArray(event.files) ? (event.files as ClientFileReference[]) : [],
      messages = ordered.messages.map((message, messageIndex) => {
        if (messageIndex !== index) return message;
        const files = new Map((message.files ?? []).map((file) => [file.id, file]));
        for (const file of incoming) files.set(file.id, file);
        return {
          ...message,
          content: `${message.content}${content}`,
          status: 'streaming' as const,
          sequence: message.sequence ?? sequence,
          ...(files.size ? { files: [...files.values()] } : {}),
        };
      });
    return {
      ...ordered,
      messages,
      nextSequence: ordered.messages[index]?.sequence === undefined ? sequence + 1 : ordered.nextSequence,
      progress: 'Responding…',
    };
  }
  if (event.type === 'done') {
    const reason = event.reason === 'failed' || event.reason === 'interrupted' ? event.reason : 'completed';
    const message = typeof event.message === 'string' ? event.message.trim() : '';
    const error = reason === 'failed' ? message || 'The tool turn failed.' : undefined;
    if (index < 0)
      return {
        ...ordered,
        activeAssistantId: undefined,
        activeTurnId: undefined,
        progress: undefined,
        turnTiming: undefined,
        error,
      };
    const status: ConversationMessageStatus = reason;
    const sequence = ordered.nextSequence ?? 0,
      messages = ordered.messages.map((message, messageIndex) =>
        messageIndex === index
          ? {
              ...message,
              status,
              sequence: message.sequence ?? sequence,
              content:
                message.content ||
                (status === 'interrupted'
                  ? 'Stopped before a response was completed.'
                  : emptyTurnContent(ordered, index)),
            }
          : message,
      );
    return {
      ...ordered,
      messages,
      nextSequence: ordered.messages[index]?.sequence === undefined ? sequence + 1 : ordered.nextSequence,
      activeAssistantId: undefined,
      activeTurnId: undefined,
      progress: undefined,
      turnTiming: undefined,
      error,
    };
  }
  return state;
}

/** A turn with no stream event for this long may be stuck (HS2-XV1VCR). */
export const TURN_STUCK_AFTER_MS = 120_000;
/** A silent open tool call (a build or test run) gets longer before the turn may be stuck. */
export const TOOL_STUCK_AFTER_MS = 300_000;

export type ConversationTurnProgressState = 'working' | 'tool' | 'waiting' | 'stuck';
export interface ConversationTurnProgress {
  state: ConversationTurnProgressState;
  /** The full line, for example `Working · 3m 12s · last activity 8s ago`. */
  label: string;
  /** A tab or tile summary, for example `Running Bash · 2m`. */
  short: string;
  /** For a stuck turn, what it was doing, for example `Running Bash · 6m 2s`. */
  detail?: string;
  /** Milliseconds since the turn's last stream event. */
  idleMs: number;
  /** Whether the turn's start time is known; an older server's snapshot may not report it. */
  timed?: boolean;
}

/**
 * A tab or tile summary for a running turn: its short progress when it is timed or may be
 * stuck, otherwise the caller's more specific progress text.
 */
export function conversationTurnSummary(progress: ConversationTurnProgress | undefined): string | undefined {
  return progress && (progress.timed || progress.state === 'stuck') ? progress.short : undefined;
}

/** `8s`, `3m 12s`, `1h 4m`; a negative span (clock skew) reads as zero. */
export function formatTurnDuration(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ${seconds % 60}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

/** Whole-unit duration for a tab or tile summary: `8s`, `3m`, `1h 4m`. */
export function formatTurnDurationShort(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  return minutes < 60 ? `${minutes}m` : `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

const finite = (value: unknown): number | undefined =>
  typeof value === 'number' && Number.isFinite(value) ? value : undefined;

/**
 * What a running turn is doing and for how long (HS2-XV1VCR), from the conversation's
 * event-advanced timing and the connection's server snapshot, which seeds a reload or a turn
 * another client started. `now` comes from a local render-only clock; nothing here polls.
 * Returns nothing while the connection is idle.
 */
export function conversationTurnProgress(
  state: ConversationState,
  connection:
    Pick<ToolConnection, 'busy' | 'turn_id' | 'turn_started_at_ms' | 'last_event_at_ms' | 'in_flight_tool'> | undefined,
  now: number,
  options: { waitingForPermission?: boolean } = {},
): ConversationTurnProgress | undefined {
  if (!connection?.busy) return;
  let timing: ConversationTurnTiming | undefined = state.turnTiming;
  // Timing kept for another turn (a stale local record) is not this turn's.
  if (timing?.turnId && connection.turn_id && timing.turnId !== connection.turn_id) timing = undefined;
  const serverLast = finite(connection.last_event_at_ms),
    // A locally recorded send time is on this device's clock: once the server reports the turn's
    // last activity, only server-stamped local timing is comparable with it (HS2-GA5ASE).
    localLast = serverLast !== undefined && !timing?.serverStamped ? undefined : finite(timing?.lastEventAt),
    startedAt = finite(connection.turn_started_at_ms) ?? finite(timing?.startedAt),
    lastEventAt = Math.max(localLast ?? -Infinity, serverLast ?? -Infinity, startedAt ?? -Infinity),
    // The fresher record names the in-flight tool: events after the connection snapshot win.
    tool =
      timing && localLast !== undefined && (serverLast === undefined || localLast >= serverLast)
        ? timing.inFlightTool
        : inFlightTool(connection.in_flight_tool);
  const timed = startedAt !== undefined,
    elapsed = startedAt === undefined ? undefined : now - startedAt,
    idleMs = Number.isFinite(lastEventAt) ? Math.max(0, now - lastEventAt) : 0,
    lastActivity = idleMs < 5_000 ? 'last activity just now' : `last activity ${formatTurnDuration(idleMs)} ago`,
    toolName = tool ? `Running ${tool.name}${tool.count > 1 ? ` +${tool.count - 1}` : ''}` : undefined,
    toolElapsed = tool ? now - tool.startedAt : undefined,
    doing = toolName ?? (options.waitingForPermission ? 'Waiting for permission' : 'Working'),
    doingFor = toolElapsed ?? elapsed,
    detail = doingFor === undefined ? doing : `${doing} · ${formatTurnDuration(doingFor)}`,
    short = doingFor === undefined ? doing : `${doing} · ${formatTurnDurationShort(doingFor)}`;
  if (options.waitingForPermission && !tool) return { state: 'waiting', label: detail, short, idleMs, timed };
  if (Number.isFinite(lastEventAt) && idleMs >= (tool ? TOOL_STUCK_AFTER_MS : TURN_STUCK_AFTER_MS))
    return {
      state: 'stuck',
      label: `No activity for ${formatTurnDurationShort(idleMs)} — may be stuck`,
      short: `May be stuck · no activity for ${formatTurnDurationShort(idleMs)}`,
      detail,
      idleMs,
      timed,
    };
  return { state: tool ? 'tool' : 'working', label: `${detail} · ${lastActivity}`, short, idleMs, timed };
}

export function applyConversationActivity(state: ConversationState, event: ActivityEvent): ConversationState {
  const ordered = normalizeConversationSequence(state),
    current = ordered.activity ?? [];
  if (current.some((item) => item.id === event.id)) return state;
  const sequence = ordered.nextSequence ?? 0;
  return {
    ...ordered,
    activity: [
      ...current,
      {
        id: event.id,
        tool: event.tool,
        kind: event.kind,
        summary: event.summary,
        importance: event.importance,
        sequence,
      },
    ].slice(-50),
    nextSequence: sequence + 1,
  };
}

export function conversationUsage(state: ConversationState): ConversationUsage | undefined {
  const events = state.messages.flatMap((message) => (message.usage ? [message.usage] : []));
  if (!events.length) return;
  const priced = events.filter((event) => event.costUsd !== undefined);
  return {
    tokensIn: events.reduce((sum, event) => sum + event.tokensIn, 0),
    tokensOut: events.reduce((sum, event) => sum + event.tokensOut, 0),
    ...(priced.length === events.length ? { costUsd: priced.reduce((sum, event) => sum + event.costUsd!, 0) } : {}),
  };
}

export function formatConversationTokens(value: number): string {
  return new Intl.NumberFormat('en-US', {
    notation: value >= 10_000 ? 'compact' : 'standard',
    maximumFractionDigits: 1,
  }).format(value);
}
export function formatConversationCost(value?: number): string {
  return value === undefined ? 'Cost unavailable' : `≈$${value < 0.01 ? value.toFixed(4) : value.toFixed(2)}`;
}

/** Whole-unit compact token count for a header summary: `950`, `39K`, `1.3M` (HS2-P415F0). */
export function formatConversationTokensCompact(value: number): string {
  return new Intl.NumberFormat('en-US', {
    notation: value >= 1_000 ? 'compact' : 'standard',
    maximumFractionDigits: value >= 1_000_000 ? 1 : 0,
  }).format(value);
}

/** Cent-rounded dollars for a header summary; a nonzero total below one cent reads `<$0.01`. */
export function formatConversationCostShort(value: number): string {
  return value > 0 && value < 0.005 ? '<$0.01' : `$${value.toFixed(2)}`;
}

/**
 * The conversation dialog's subtitle: the message count followed by the conversation's total
 * tokens and cost, for example `2 messages • 39K tokens • $0.23`. Totals that are absent, zero,
 * or unpriced are omitted rather than shown as `0 tokens` or a placeholder (HS2-P415F0).
 */
export function conversationHeaderSummary(messageCount: number, usage?: ConversationUsage): string {
  const parts = [`${messageCount} message${messageCount === 1 ? '' : 's'}`],
    tokens = usage ? usage.tokensIn + usage.tokensOut : 0;
  if (tokens > 0) parts.push(`${formatConversationTokensCompact(tokens)} tokens`);
  if (usage?.costUsd !== undefined && (tokens > 0 || usage.costUsd > 0))
    parts.push(formatConversationCostShort(usage.costUsd));
  return parts.join(' • ');
}
