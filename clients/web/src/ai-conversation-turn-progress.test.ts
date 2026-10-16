import { describe, expect, it } from 'vitest';

import {
  applyConversationEvent,
  beginConversationTurn,
  type ConversationState,
  conversationTurnProgress,
  conversationTurnSummary,
  EMPTY_CONVERSATION,
  formatTurnDuration,
  formatTurnDurationShort,
  TOOL_STUCK_AFTER_MS,
  TURN_STUCK_AFTER_MS,
} from './ai-conversation';
import type { ToolConnection } from './api';

const T0 = 1_700_000_000_000;
const connection = (fields: Partial<ToolConnection> = {}): ToolConnection => ({
  id: 'chat',
  tool: 'claude',
  project: '/p',
  role: 'main',
  busy: true,
  turn_id: 'turn-1',
  ...fields,
});
const output = { type: 'output', content: 'x', truncated: false } as const;
const bash = { name: 'Bash', started_at_ms: T0 + 10_000, count: 1 };

describe('running turn timing (HS2-XV1VCR)', () => {
  it('formats elapsed spans for the line and the summaries', () => {
    expect(formatTurnDuration(-5)).toBe('0s');
    expect(formatTurnDuration(8_400)).toBe('8s');
    expect(formatTurnDuration(192_000)).toBe('3m 12s');
    expect(formatTurnDuration(3_840_000)).toBe('1h 4m');
    expect(formatTurnDurationShort(8_000)).toBe('8s');
    expect(formatTurnDurationShort(192_000)).toBe('3m');
    expect(formatTurnDurationShort(3_840_000)).toBe('1h 4m');
  });

  it('walks a local turn: start → event → tool open → tool closed → done', () => {
    let state = beginConversationTurn(EMPTY_CONVERSATION, 'turn-1', 'Run the tests', T0);
    expect(state.turnTiming).toEqual({ turnId: 'turn-1', startedAt: T0, lastEventAt: T0 });
    expect(conversationTurnProgress(state, connection(), T0 + 2_000)).toMatchObject({
      state: 'working',
      label: 'Working · 2s · last activity just now',
      short: 'Working · 2s',
    });

    state = applyConversationEvent(state, output, 'turn-1', { at_ms: T0 + 9_000 });
    expect(state.turnTiming?.lastEventAt).toBe(T0 + 9_000);
    state = applyConversationEvent(state, { type: 'native_activity', source: 'claude-hooks', payload: {} }, 'turn-1', {
      at_ms: T0 + 10_000,
      in_flight_tool: bash,
    });
    expect(state.turnTiming?.inFlightTool).toEqual({ name: 'Bash', startedAt: T0 + 10_000, count: 1 });
    expect(conversationTurnProgress(state, connection(), T0 + 135_000)).toMatchObject({
      state: 'tool',
      label: 'Running Bash · 2m 5s · last activity 2m 5s ago',
      short: 'Running Bash · 2m',
    });

    // A stamped event without a tool means the call finished.
    state = applyConversationEvent(state, output, 'turn-1', { at_ms: T0 + 140_000 });
    expect(state.turnTiming?.inFlightTool).toBeUndefined();
    expect(conversationTurnProgress(state, connection(), T0 + 148_000)?.label).toBe(
      'Working · 2m 28s · last activity 8s ago',
    );

    state = applyConversationEvent(state, { type: 'done', reason: 'completed' }, 'turn-1');
    expect(state.turnTiming).toBeUndefined();
    expect(conversationTurnProgress(state, connection({ busy: false }), T0 + 150_000)).toBeUndefined();
  });

  it('shows parallel calls and keeps last activity monotonic for late events', () => {
    let state = beginConversationTurn(EMPTY_CONVERSATION, 'turn-1', 'Research', T0);
    state = applyConversationEvent(state, output, 'turn-1', {
      at_ms: T0 + 20_000,
      in_flight_tool: { name: 'Task', started_at_ms: T0 + 1_000, count: 4 },
    });
    // A late, earlier-stamped event never moves last activity backwards.
    state = applyConversationEvent(state, output, 'turn-1', {
      at_ms: T0 + 5_000,
      in_flight_tool: { name: 'Task', started_at_ms: T0 + 1_000, count: 4 },
    });
    expect(state.turnTiming?.lastEventAt).toBe(T0 + 20_000);
    expect(conversationTurnProgress(state, connection(), T0 + 61_000)?.short).toBe('Running Task +3 · 1m');
  });

  it('refreshes last activity at receipt time for older servers and keeps their tool unknown', () => {
    let state = beginConversationTurn(EMPTY_CONVERSATION, 'turn-1', 'Hi', T0);
    state = applyConversationEvent(state, output, undefined, undefined, T0 + 30_000);
    expect(state.turnTiming).toMatchObject({ lastEventAt: T0 + 30_000 });
    expect(state.turnTiming?.inFlightTool).toBeUndefined();
  });

  it('becomes stuck after a bounded silence, longer while a tool is in flight', () => {
    const state = beginConversationTurn(EMPTY_CONVERSATION, 'turn-1', 'Hi', T0);
    expect(conversationTurnProgress(state, connection(), T0 + TURN_STUCK_AFTER_MS - 1)?.state).toBe('working');
    const stuck = conversationTurnProgress(state, connection(), T0 + TURN_STUCK_AFTER_MS);
    expect(stuck).toMatchObject({
      state: 'stuck',
      label: 'No activity for 2m — may be stuck',
      short: 'May be stuck · no activity for 2m',
      detail: 'Working · 2m 0s',
    });

    const withTool = applyConversationEvent(state, output, 'turn-1', { at_ms: T0, in_flight_tool: bash });
    expect(conversationTurnProgress(withTool, connection(), T0 + TURN_STUCK_AFTER_MS)?.state).toBe('tool');
    expect(conversationTurnProgress(withTool, connection(), T0 + TOOL_STUCK_AFTER_MS)).toMatchObject({
      state: 'stuck',
      detail: 'Running Bash · 4m 50s',
    });
    // A new event clears the stuck state.
    const resumed = applyConversationEvent(withTool, output, 'turn-1', { at_ms: T0 + TOOL_STUCK_AFTER_MS });
    expect(conversationTurnProgress(resumed, connection(), T0 + TOOL_STUCK_AFTER_MS + 1_000)?.state).toBe('working');
  });

  it('waits for permission instead of becoming stuck', () => {
    const state = beginConversationTurn(EMPTY_CONVERSATION, 'turn-1', 'Hi', T0);
    expect(
      conversationTurnProgress(state, connection(), T0 + 10 * TURN_STUCK_AFTER_MS, { waitingForPermission: true }),
    ).toMatchObject({ state: 'waiting', label: 'Waiting for permission · 20m 0s' });
  });

  it('seeds a reload or reconnect mid-turn from the connection snapshot', () => {
    // A reloaded client has no local timing for the running turn.
    const reloaded: ConversationState = { messages: [] },
      snapshot = connection({
        turn_started_at_ms: T0,
        last_event_at_ms: T0 + 50_000,
        in_flight_tool: { name: 'Bash', started_at_ms: T0 + 40_000, count: 1 },
      });
    expect(conversationTurnProgress(reloaded, snapshot, T0 + 60_000)).toMatchObject({
      state: 'tool',
      label: 'Running Bash · 20s · last activity 10s ago',
    });
    // Events after the snapshot win; the snapshot's start time still dates the turn.
    const later = applyConversationEvent(reloaded, output, 'turn-1', { at_ms: T0 + 70_000 });
    expect(later.turnTiming).toEqual({ turnId: 'turn-1', lastEventAt: T0 + 70_000, serverStamped: true });
    expect(conversationTurnProgress(later, snapshot, T0 + 75_000)?.label).toBe(
      'Working · 1m 15s · last activity 5s ago',
    );
    // An older stored record loses to a fresher snapshot.
    const stale: ConversationState = {
      messages: [],
      turnTiming: {
        turnId: 'turn-1',
        lastEventAt: T0 + 1_000,
        inFlightTool: { name: 'Read', startedAt: T0, count: 1 },
      },
    };
    expect(conversationTurnProgress(stale, snapshot, T0 + 60_000)?.short).toBe('Running Bash · 20s');
    // Timing kept for a different turn is ignored.
    const other: ConversationState = { messages: [], turnTiming: { turnId: 'turn-0', lastEventAt: T0 + 59_000 } };
    expect(conversationTurnProgress(other, snapshot, T0 + 60_000)?.label).toBe(
      'Running Bash · 20s · last activity 10s ago',
    );
    // An older server's snapshot without timing still reads as working.
    expect(conversationTurnProgress(reloaded, connection(), T0)?.label).toBe('Working · last activity just now');
  });

  it('keeps a skewed device send time out of server-stamped timing (HS2-GA5ASE)', () => {
    // This device runs three minutes ahead of the server: its send time is in the server's future.
    const ahead = 180_000;
    let state = beginConversationTurn({ messages: [] }, 'turn-9', 'Run it', T0 + ahead);
    const running = connection({ turn_id: 'turn-9', turn_started_at_ms: T0, last_event_at_ms: T0 });
    // The server's snapshot of the turn wins over the unstamped local send time.
    expect(conversationTurnProgress(state, running, T0 + 130_000)?.state).toBe('stuck');
    // The first stamped event replaces the local send time instead of losing to it.
    state = applyConversationEvent(state, output, 'turn-9', { at_ms: T0 + 10_000 });
    expect(state.turnTiming).toMatchObject({ lastEventAt: T0 + 10_000, serverStamped: true });
    // Later stamps advance it as usual, and an out-of-order earlier stamp does not rewind it.
    state = applyConversationEvent(state, output, 'turn-9', { at_ms: T0 + 20_000 });
    state = applyConversationEvent(state, output, 'turn-9', { at_ms: T0 + 15_000 });
    expect(state.turnTiming?.lastEventAt).toBe(T0 + 20_000);
    expect(conversationTurnProgress(state, running, T0 + 30_000)?.label).toBe('Working · 30s · last activity 10s ago');
  });

  it("times another client's turn from its events and forgets it when it ends", () => {
    let state: ConversationState = { messages: [] };
    state = applyConversationEvent(state, output, 'remote-1', { at_ms: T0 + 1_000, in_flight_tool: bash });
    expect(state.turnTiming).toMatchObject({ turnId: 'remote-1', lastEventAt: T0 + 1_000 });
    expect(state.messages).toEqual([]);
    // A different remote turn replaces it.
    state = applyConversationEvent(state, output, 'remote-2', { at_ms: T0 + 2_000 });
    expect(state.turnTiming).toEqual({ turnId: 'remote-2', lastEventAt: T0 + 2_000, serverStamped: true });
    // The previous remote turn's late Done does not clear the newer one; its own Done does.
    state = applyConversationEvent(state, { type: 'done', reason: 'completed' }, 'remote-1');
    expect(state.turnTiming?.turnId).toBe('remote-2');
    state = applyConversationEvent(state, { type: 'done', reason: 'completed' }, 'remote-2');
    expect(state.turnTiming).toBeUndefined();
  });

  it('ignores a late event from an earlier local turn after a new one starts', () => {
    let state = beginConversationTurn(EMPTY_CONVERSATION, 'turn-1', 'One', T0);
    state = applyConversationEvent(state, { type: 'done', reason: 'completed' }, 'turn-1');
    state = beginConversationTurn(state, 'turn-2', 'Two', T0 + 10_000);
    const late = applyConversationEvent(state, output, 'turn-1', { at_ms: T0 + 20_000, in_flight_tool: bash });
    expect(late.turnTiming).toEqual({ turnId: 'turn-2', startedAt: T0 + 10_000, lastEventAt: T0 + 10_000 });
  });

  it('ignores malformed tool snapshots', () => {
    const state = beginConversationTurn(EMPTY_CONVERSATION, 'turn-1', 'Hi', T0);
    for (const in_flight_tool of [{ name: ' ', started_at_ms: T0, count: 1 }, { name: 'Bash', count: 1 } as never]) {
      const next = applyConversationEvent(state, output, 'turn-1', { at_ms: T0 + 1, in_flight_tool });
      expect(next.turnTiming?.inFlightTool).toBeUndefined();
    }
    const counted = applyConversationEvent(state, output, 'turn-1', {
      at_ms: T0 + 1,
      in_flight_tool: { name: 'Bash', started_at_ms: T0, count: 0 },
    });
    expect(counted.turnTiming?.inFlightTool?.count).toBe(1);
    expect(conversationTurnProgress(state, undefined, T0)).toBeUndefined();
  });

  it('summarizes a timed or stuck turn and leaves an untimed one to the specific progress text', () => {
    const local = beginConversationTurn(EMPTY_CONVERSATION, 'turn-1', 'Hi', T0);
    expect(conversationTurnSummary(conversationTurnProgress(local, connection(), T0 + 61_000))).toBe('Working · 1m');
    // An older server's snapshot without a start time: the caller's progress text is more specific.
    const untimed = conversationTurnProgress({ messages: [] }, connection(), T0);
    expect(untimed?.timed).toBe(false);
    expect(conversationTurnSummary(untimed)).toBeUndefined();
    const stuck = conversationTurnProgress(
      { messages: [], turnTiming: { lastEventAt: T0 } },
      connection(),
      T0 + TURN_STUCK_AFTER_MS,
    );
    expect(conversationTurnSummary(stuck)).toBe('May be stuck · no activity for 2m');
    expect(conversationTurnSummary(undefined)).toBeUndefined();
  });
});
