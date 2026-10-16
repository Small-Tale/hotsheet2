import type { ConversationActivity, ConversationMessage, ConversationState } from './ai-conversation';

export const CONVERSATION_STORAGE_KEY = 'hotsheet.ai-conversations.v1';

interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function message(value: unknown): value is ConversationMessage {
  if (!value || typeof value !== 'object') return false;
  const item = value as Partial<ConversationMessage>;
  return (
    typeof item.id === 'string' &&
    (item.role === 'user' || item.role === 'assistant') &&
    typeof item.content === 'string'
  );
}

function activity(value: unknown): value is ConversationActivity {
  if (!value || typeof value !== 'object') return false;
  const item = value as Partial<ConversationActivity>;
  return (
    typeof item.id === 'string' &&
    typeof item.tool === 'string' &&
    typeof item.kind === 'string' &&
    typeof item.summary === 'string' &&
    ['low', 'normal', 'high'].includes(item.importance ?? '')
  );
}

function state(value: unknown): value is ConversationState {
  if (!value || typeof value !== 'object') return false;
  const item = value as Partial<ConversationState>;
  return (
    Array.isArray(item.messages) &&
    item.messages.every(message) &&
    (!item.activity || (Array.isArray(item.activity) && item.activity.every(activity)))
  );
}

/** Drop a corrupt skipped-message map instead of the whole conversation (HS2-BZB93V). */
function withValidSkips(value: ConversationState): ConversationState {
  const skips: unknown = value.skippedBefore;
  if (skips === undefined) return value;
  const valid =
    skips !== null &&
    typeof skips === 'object' &&
    !Array.isArray(skips) &&
    Object.values(skips).every((count) => typeof count === 'number' && Number.isSafeInteger(count) && count > 0);
  if (valid) return value;
  const rest = { ...value };
  delete rest.skippedBefore;
  return rest;
}

const optionalFinite = (value: unknown) => value === undefined || (typeof value === 'number' && Number.isFinite(value));

/** Drop corrupt running-turn timing instead of the whole conversation (HS2-XV1VCR). */
function withValidTurnTiming(value: ConversationState): ConversationState {
  const timing: unknown = value.turnTiming;
  if (timing === undefined) return value;
  const record =
      timing !== null && typeof timing === 'object' && !Array.isArray(timing)
        ? (timing as Record<string, unknown>)
        : undefined,
    tool: unknown = record?.inFlightTool,
    fields = tool !== null && typeof tool === 'object' ? (tool as Record<string, unknown>) : undefined,
    validTool =
      tool === undefined ||
      (fields !== undefined &&
        typeof fields.name === 'string' &&
        typeof fields.startedAt === 'number' &&
        Number.isFinite(fields.startedAt) &&
        typeof fields.count === 'number' &&
        Number.isFinite(fields.count));
  if (
    record &&
    (record.turnId === undefined || typeof record.turnId === 'string') &&
    optionalFinite(record.startedAt) &&
    optionalFinite(record.lastEventAt) &&
    validTool
  )
    return value;
  const rest = { ...value };
  delete rest.turnTiming;
  return rest;
}

export function loadConversationStates(storage: Pick<StorageLike, 'getItem'>): Record<string, ConversationState> {
  try {
    const parsed: unknown = JSON.parse(storage.getItem(CONVERSATION_STORAGE_KEY) ?? '{}');
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
    const valid: Record<string, ConversationState> = {};
    for (const [key, value] of Object.entries(parsed))
      if (key && state(value)) valid[key] = withValidTurnTiming(withValidSkips(value));
    return valid;
  } catch {
    return {};
  }
}

export function saveConversationStates(
  storage: Pick<StorageLike, 'setItem'>,
  states: Record<string, ConversationState>,
): void {
  storage.setItem(CONVERSATION_STORAGE_KEY, JSON.stringify(states));
}

/** Keep storage serialization off the per-event path while bounding crash-recovery lag. */
export function createConversationPersistence(flushLatest: () => void) {
  let quietTimer: ReturnType<typeof setTimeout> | undefined;
  let deadlineTimer: ReturnType<typeof setTimeout> | undefined;
  const flush = () => {
    if (quietTimer === undefined && deadlineTimer === undefined) return;
    if (quietTimer !== undefined) clearTimeout(quietTimer);
    if (deadlineTimer !== undefined) clearTimeout(deadlineTimer);
    quietTimer = undefined;
    deadlineTimer = undefined;
    flushLatest();
  };
  const schedule = () => {
    if (quietTimer !== undefined) clearTimeout(quietTimer);
    quietTimer = setTimeout(flush, 250);
    deadlineTimer ??= setTimeout(flush, 1_000);
  };
  return { schedule, flush };
}
