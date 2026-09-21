import type { ActivityEvent, ClientFileReference, ClientTurnEvent } from './api';

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
export interface ConversationState {
  messages: ConversationMessage[];
  activity?: ConversationActivity[];
  activeAssistantId?: string;
  progress?: string;
  error?: string;
  nextSequence?: number;
}
export type ConversationTimelineGroup =
  { kind: 'message'; message: ConversationMessage } | { kind: 'activity'; activity: ConversationActivity[] };

export const EMPTY_CONVERSATION: ConversationState = { messages: [] };

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

export function beginConversationTurn(state: ConversationState, id: string, content: string): ConversationState {
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
    progress: 'Reviewing the project and planning the next steps…',
    error: undefined,
  };
}

function nativeProgress(payload: unknown): string | undefined {
  if (!payload || typeof payload !== 'object') return;
  const value = payload as Record<string, unknown>;
  for (const key of ['summary', 'message', 'title', 'kind'])
    if (typeof value[key] === 'string' && value[key].trim()) return value[key].trim().slice(0, 160);
}

export function applyConversationEvent(state: ConversationState, event: ClientTurnEvent): ConversationState {
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
    if (index < 0) return { ...ordered, activeAssistantId: undefined, progress: undefined };
    const reason = event.reason === 'failed' || event.reason === 'interrupted' ? event.reason : 'completed';
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
                  : 'The turn ended without output.'),
            }
          : message,
      );
    return {
      ...ordered,
      messages,
      nextSequence: ordered.messages[index]?.sequence === undefined ? sequence + 1 : ordered.nextSequence,
      activeAssistantId: undefined,
      progress: undefined,
      error: reason === 'failed' ? 'The tool turn failed.' : undefined,
    };
  }
  return state;
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
