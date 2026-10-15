import { describe, expect, it } from 'vitest';

import {
  applyConversationActivity,
  applyConversationEvent,
  beginConversationTurn,
  conversationError,
  conversationTimeline,
  conversationUsage,
  EMPTY_CONVERSATION,
  formatConversationCost,
  formatConversationTokens,
  reconcileConversationConnection,
} from './ai-conversation';

describe('AI conversation transcript', () => {
  it('walks a turn through output, permission, activity, and completion', () => {
    let state = beginConversationTurn(EMPTY_CONVERSATION, 'turn-1', 'Please review this.');
    expect(state.messages.map((message) => message.role)).toEqual(['user', 'assistant']);
    state = applyConversationEvent(state, { type: 'output', content: 'First ', truncated: false });
    state = applyConversationEvent(state, { type: 'output', content: 'answer.', truncated: false });
    expect(state.messages[1].content).toBe('First answer.');
    expect(state.progress).toBe('Responding…');
    state = applyConversationEvent(state, { type: 'permission_asked', tool: 'bash', summary: 'run tests' });
    expect(state.progress).toBe('Waiting for permission…');
    state = applyConversationEvent(state, {
      type: 'native_activity',
      source: 'codex',
      payload: { summary: 'Running focused tests' },
    });
    expect(state.progress).toBe('Running focused tests');
    state = applyConversationEvent(state, {
      type: 'usage',
      model: 'codex-5.6',
      tokens_in: 12_000,
      tokens_out: 800,
      cost_usd: 0.0412,
    });
    expect(state.messages[1].usage).toEqual({ model: 'codex-5.6', tokensIn: 12_000, tokensOut: 800, costUsd: 0.0412 });
    state = applyConversationEvent(state, { type: 'done', reason: 'completed' });
    expect(state.messages[1].status).toBe('completed');
    expect(state.activeAssistantId).toBeUndefined();
    expect(conversationUsage(state)).toEqual({ tokensIn: 12_000, tokensOut: 800, costUsd: 0.0412 });
  });

  it('keeps activity before the result that resumes after a permission pause', () => {
    let state = beginConversationTurn(EMPTY_CONVERSATION, 'turn-paused', 'What time is it?');
    state = applyConversationActivity(state, {
      id: 'command',
      ts: '2026-09-14T07:23:40Z',
      tool: 'Claude',
      kind: 'command',
      summary: 'claude ran `date`',
      importance: 'normal',
    });
    state = applyConversationEvent(state, { type: 'permission_asked', tool: 'Bash', summary: 'Run date' });
    state = applyConversationEvent(state, { type: 'output', content: 'It is 3:23 PM.', truncated: false });
    state = applyConversationEvent(state, { type: 'done', reason: 'completed' });
    expect(
      conversationTimeline(state.messages, state.activity).map((group) =>
        group.kind === 'message' ? group.message.id : group.activity.map((item) => item.id).join(','),
      ),
    ).toEqual(['turn-paused', 'command', 'turn-paused-assistant']);
  });

  it('preserves output-before-activity order and groups adjacent activity without reordering duplicates', () => {
    let state = beginConversationTurn(EMPTY_CONVERSATION, 'turn-streaming', 'Inspect this.');
    state = applyConversationEvent(state, { type: 'output', content: 'Starting.', truncated: false });
    state = applyConversationActivity(state, {
      id: 'read',
      ts: '2026-09-14T07:23:40Z',
      tool: 'Codex',
      kind: 'read',
      summary: 'Read the file',
      importance: 'normal',
    });
    state = applyConversationActivity(state, {
      id: 'test',
      ts: '2026-09-14T07:23:41Z',
      tool: 'Codex',
      kind: 'command',
      summary: 'Ran the test',
      importance: 'normal',
    });
    const duplicate = applyConversationActivity(state, {
      id: 'read',
      ts: '2026-09-14T07:23:42Z',
      tool: 'Codex',
      kind: 'read',
      summary: 'Duplicate replay',
      importance: 'normal',
    });
    expect(duplicate).toBe(state);
    expect(
      conversationTimeline(state.messages, state.activity).map((group) =>
        group.kind === 'message' ? group.message.id : group.activity.map((item) => item.id).join(','),
      ),
    ).toEqual(['turn-streaming', 'turn-streaming-assistant', 'read,test']);
  });

  it('preserves prior transcript activity and clears a stale error when retrying', () => {
    const prior = {
      messages: [{ id: 'old', role: 'assistant' as const, content: 'Earlier answer.', status: 'failed' as const }],
      activity: [
        {
          id: 'activity-1',
          tool: 'Codex',
          kind: 'command',
          summary: 'Inspected the project',
          importance: 'normal' as const,
        },
      ],
      error: 'The prior turn failed.',
    };
    const retried = beginConversationTurn(prior, 'turn-retry', 'Try again.');
    expect(retried.messages.map((message) => message.id)).toEqual(['old', 'turn-retry', 'turn-retry-assistant']);
    expect(retried.activity).toEqual([expect.objectContaining(prior.activity[0])]);
    expect(
      conversationTimeline(retried.messages, retried.activity).map((group) =>
        group.kind === 'message' ? group.message.id : group.activity[0].id,
      ),
    ).toEqual(['old', 'activity-1', 'turn-retry', 'turn-retry-assistant']);
    expect(retried.error).toBeUndefined();
  });

  it('retains provider failure text, clears halt on retry, and keeps success/interruption clean (HS2-AZVE3P)', () => {
    let state = beginConversationTurn(EMPTY_CONVERSATION, 'failed-1', 'Work');
    state = applyConversationEvent(state, {
      type: 'done',
      reason: 'failed',
      message: '  Selected model is at capacity  ',
    });
    expect(state.error).toBe('Selected model is at capacity');
    expect(state.messages.at(-1)?.status).toBe('failed');
    const stale = { last_error: state.error, busy: false };
    expect(conversationError(state, stale)).toBe(state.error);
    expect(conversationError(state, { ...stale, busy: true })).toBeUndefined();
    state = beginConversationTurn(state, 'retry-1', 'Try again');
    expect(conversationError(state, stale)).toBeUndefined();
    state = applyConversationEvent(state, { type: 'output', content: 'Recovered', truncated: false });
    state = applyConversationEvent(state, { type: 'done', reason: 'completed' });
    expect(state.error).toBeUndefined();
    expect(conversationError(state, { busy: false })).toBeUndefined();
    state = beginConversationTurn(state, 'failed-2', 'Work again');
    state = applyConversationEvent(state, { type: 'done', reason: 'failed', message: '  ' });
    expect(state.error).toBe('The tool turn failed.');
    state = beginConversationTurn(state, 'stop-1', 'Another try');
    state = applyConversationEvent(state, { type: 'done', reason: 'interrupted', message: 'ignored' });
    expect(state.error).toBeUndefined();
    expect(
      applyConversationEvent(EMPTY_CONVERSATION, { type: 'done', reason: 'failed', message: 'Restored failure' }).error,
    ).toBe('Restored failure');
    expect(conversationError(EMPTY_CONVERSATION, { last_error: 'Restored failure', busy: false })).toBe(
      'Restored failure',
    );
    expect(conversationError(EMPTY_CONVERSATION, { last_error: 'Old failure', busy: true })).toBeUndefined();
  });

  it('retains stable structured file references emitted with assistant output', () => {
    let state = beginConversationTurn(EMPTY_CONVERSATION, 'turn-files', 'Inspect the files');
    state = applyConversationEvent(state, {
      type: 'output',
      content: 'Attached.',
      truncated: false,
      files: [{ id: 'proof-1', filename: 'proof.png', mime_type: 'image/png', kind: 'media', url: '/files/proof.png' }],
    });
    state = applyConversationEvent(state, {
      type: 'output',
      content: ' Done.',
      truncated: false,
      files: [{ id: 'proof-1', filename: 'proof.png', mime_type: 'image/png', kind: 'media', url: '/files/proof.png' }],
    });
    expect(state.messages[1].files).toEqual([
      { id: 'proof-1', filename: 'proof.png', mime_type: 'image/png', kind: 'media', url: '/files/proof.png' },
    ]);
  });

  it('retains unknown events and gives interrupted empty turns a useful result', () => {
    const started = beginConversationTurn(EMPTY_CONVERSATION, 'turn-2', 'Stop soon.');
    expect(applyConversationEvent(started, { type: 'future', value: 1 })).toBe(started);
    const stopped = applyConversationEvent(started, { type: 'done', reason: 'interrupted' });
    expect(stopped.messages[1]).toMatchObject({
      status: 'interrupted',
      content: 'Stopped before a response was completed.',
    });
  });

  it('deduplicates and bounds normalized activity while keeping AI-safe usage precision', () => {
    let state = EMPTY_CONVERSATION;
    for (let index = 0; index < 55; index += 1)
      state = applyConversationActivity(state, {
        id: `event-${index}`,
        ts: '2026-09-08T00:00:00Z',
        tool: 'Codex',
        session: 'session-1',
        kind: 'edit',
        summary: `Edited file ${index}`,
        importance: 'normal',
      });
    const duplicate = applyConversationActivity(state, {
      id: 'event-54',
      ts: '2026-09-08T00:00:00Z',
      tool: 'Codex',
      kind: 'edit',
      summary: 'Duplicate',
      importance: 'normal',
    });
    expect(duplicate).toBe(state);
    expect(state.activity).toHaveLength(50);
    expect(state.activity?.[0].id).toBe('event-5');
    expect(formatConversationTokens(12_840)).toBe('12.8K');
    expect(formatConversationCost(0.0042)).toBe('≈$0.0042');
    expect(formatConversationCost()).toBe('Cost unavailable');
  });
});

it('settles persisted or disconnected active turns from idle server failures without ending optimistic starts (HS2-AZVE3P)', () => {
  const active = beginConversationTurn(EMPTY_CONVERSATION, 'restored', 'work');
  const failed = { busy: false, last_error: 'Model unavailable' };
  expect(reconcileConversationConnection(active, failed, true)).toBe(active);
  expect(reconcileConversationConnection(active, { busy: true })).toBe(active);
  const settled = reconcileConversationConnection(active, failed);
  expect(settled.activeAssistantId).toBeUndefined();
  expect(settled.messages.at(-1)?.status).toBe('failed');
  expect(conversationError(settled, failed)).toBe('Model unavailable');
  expect(reconcileConversationConnection(settled, failed)).toBe(settled);
  const recovered = reconcileConversationConnection(active, { busy: false });
  expect(recovered.messages.at(-1)?.status).toBe('interrupted');
  expect(recovered.error).toBeUndefined();
});

describe('turn correlation (HS2-14X4AF)', () => {
  const output = (content: string) => ({ type: 'output', content, truncated: false }) as const;
  const done = { type: 'done', reason: 'completed' } as const;
  const usage = { type: 'usage', tokens_in: 10, tokens_out: 5, cost_usd: 0.01 } as const;

  it('ignores a delayed Done and output from an earlier turn after a new turn starts', () => {
    let state = beginConversationTurn(EMPTY_CONVERSATION, 'turn-a', 'first');
    state = applyConversationEvent(state, output('A answer'), 'turn-a');
    // The page settles turn A from a reconnect before its Done is delivered, then the user sends B.
    state = reconcileConversationConnection(state, { busy: false });
    state = beginConversationTurn(state, 'turn-b', 'second');
    const beforeLate = state;
    state = applyConversationEvent(state, output(' late A'), 'turn-a');
    state = applyConversationEvent(state, usage, 'turn-a');
    state = applyConversationEvent(state, { type: 'done', reason: 'failed', message: 'A failed late' }, 'turn-a');
    expect(state).toBe(beforeLate);
    expect(state.activeTurnId).toBe('turn-b');
    state = applyConversationEvent(state, output('B answer'), 'turn-b');
    state = applyConversationEvent(state, usage, 'turn-b');
    state = applyConversationEvent(state, done, 'turn-b');
    const b = state.messages.find((message) => message.id === 'turn-b-assistant');
    expect(b).toMatchObject({ content: 'B answer', status: 'completed', usage: { tokensIn: 10, tokensOut: 5 } });
    expect(state.messages.find((message) => message.id === 'turn-a-assistant')?.content).toBe('A answer');
    expect(state.error).toBeUndefined();
  });

  it('drops repeated deliveries of a correlated Done and events after completion', () => {
    let state = beginConversationTurn(EMPTY_CONVERSATION, 'turn-r', 'work');
    state = applyConversationEvent(state, output('Done.'), 'turn-r');
    state = applyConversationEvent(state, done, 'turn-r');
    const settled = state;
    for (const event of [done, output(' again'), usage, { type: 'done', reason: 'failed', message: 'x' } as const])
      state = applyConversationEvent(state, event, 'turn-r');
    expect(state).toBe(settled);
    expect(state.error).toBeUndefined();
  });

  it('keeps a retry isolated from the failed attempt it replaces', () => {
    let state = beginConversationTurn(EMPTY_CONVERSATION, 'attempt-1', 'work');
    state = applyConversationEvent(state, { type: 'done', reason: 'failed', message: 'capacity' }, 'attempt-1');
    state = beginConversationTurn(state, 'attempt-2', 'work');
    state = applyConversationEvent(state, { type: 'done', reason: 'failed', message: 'capacity' }, 'attempt-1');
    expect(state.activeAssistantId).toBe('attempt-2-assistant');
    expect(state.error).toBeUndefined();
    state = applyConversationEvent(state, output('Recovered'), 'attempt-2');
    state = applyConversationEvent(state, done, 'attempt-2');
    expect(state.messages.at(-1)).toMatchObject({ content: 'Recovered', status: 'completed' });
  });

  it('applies uncorrelated events from older servers to the active turn', () => {
    let state = beginConversationTurn(EMPTY_CONVERSATION, 'legacy', 'work');
    state = applyConversationEvent(state, output('Legacy answer'));
    state = applyConversationEvent(state, done);
    expect(state.messages.at(-1)).toMatchObject({ content: 'Legacy answer', status: 'completed' });
    // A turn restored from storage written before turn ids existed accepts correlated events.
    const restored = { ...beginConversationTurn(EMPTY_CONVERSATION, 'old', 'work'), activeTurnId: undefined };
    expect(applyConversationEvent(restored, output('ok'), 'server-ulid').messages.at(-1)?.content).toBe('ok');
  });

  it('settles a reconnected turn when the server reports a different running turn', () => {
    const active = beginConversationTurn(EMPTY_CONVERSATION, 'mine', 'work');
    expect(reconcileConversationConnection(active, { busy: true, turn_id: 'mine' })).toBe(active);
    expect(reconcileConversationConnection(active, { busy: true })).toBe(active);
    expect(reconcileConversationConnection(active, { busy: true, turn_id: 'other' }, true)).toBe(active);
    const settled = reconcileConversationConnection(active, { busy: true, turn_id: 'other' });
    expect(settled.activeAssistantId).toBeUndefined();
    expect(settled.activeTurnId).toBeUndefined();
    expect(settled.messages.at(-1)?.status).toBe('interrupted');
    // The other turn's stream does not reopen or fill the settled bubble.
    expect(applyConversationEvent(settled, output('foreign'), 'other')).toBe(settled);
  });
});

describe('turns without written output (HS2-M5GN19)', () => {
  const activity = (id: string, summary: string) => ({
    id,
    ts: '2026-10-15T02:43:00Z',
    tool: 'claude',
    kind: 'tool',
    summary,
    importance: 'normal' as const,
  });

  it('summarizes the actions of a tool-only turn instead of reporting no output', () => {
    let state = applyConversationActivity(EMPTY_CONVERSATION, activity('before', 'claude called WebFetch'));
    state = beginConversationTurn(state, 'act', 'work on HS2-XMJGFJ now');
    state = applyConversationActivity(state, activity('claim', 'claude called hotsheet_claim'));
    state = applyConversationActivity(state, activity('read', 'claude read lecture-outlines.md'));
    state = applyConversationActivity(state, activity('claim-again', 'claude called hotsheet_claim'));
    state = applyConversationEvent(state, { type: 'usage', tokens_in: 900, tokens_out: 60, cost_usd: 0.04 });
    state = applyConversationEvent(state, { type: 'done', reason: 'completed' });
    expect(state.messages.at(-1)).toMatchObject({
      status: 'completed',
      content:
        'Finished without a written reply after 2 actions: claude called hotsheet_claim; claude read lecture-outlines.md.',
      usage: { tokensIn: 900, tokensOut: 60, costUsd: 0.04 },
    });
  });

  it('bounds a long action summary and keeps the placeholder for a genuinely empty turn', () => {
    let state = beginConversationTurn(EMPTY_CONVERSATION, 'many', 'work');
    for (const index of [1, 2, 3, 4, 5])
      state = applyConversationActivity(state, activity(`a${index}`, `step ${index}`));
    state = applyConversationEvent(state, { type: 'done', reason: 'failed', message: 'capacity' });
    expect(state.messages.at(-1)?.content).toBe(
      'Finished without a written reply after 5 actions: step 1; step 2; step 3; and 2 more.',
    );
    let empty = applyConversationActivity(EMPTY_CONVERSATION, activity('old', 'an earlier turn acted'));
    empty = beginConversationTurn(empty, 'empty', 'hello');
    empty = applyConversationEvent(empty, { type: 'done', reason: 'completed' });
    expect(empty.messages.at(-1)?.content).toBe('The turn ended without output.');
    expect(empty.messages.at(-1)?.usage).toBeUndefined();
    let stopped = beginConversationTurn(EMPTY_CONVERSATION, 'stop', 'work');
    stopped = applyConversationActivity(stopped, activity('s', 'claude ran `date`'));
    stopped = applyConversationEvent(stopped, { type: 'done', reason: 'interrupted' });
    expect(stopped.messages.at(-1)?.content).toBe('Stopped before a response was completed.');
  });

  it('never records zero-token usage, while real usage still accumulates', () => {
    let state = beginConversationTurn(EMPTY_CONVERSATION, 'usage', 'work');
    const started = state;
    state = applyConversationEvent(state, { type: 'usage', tokens_in: 0, tokens_out: 0, cost_usd: 0 });
    state = applyConversationEvent(state, { type: 'usage', tokens_in: 0, tokens_out: 0 });
    expect(state).toBe(started);
    state = applyConversationEvent(state, { type: 'usage', tokens_in: 800, tokens_out: 150 });
    state = applyConversationEvent(state, { type: 'usage', tokens_in: 0, tokens_out: 0, cost_usd: 0 });
    state = applyConversationEvent(state, { type: 'output', content: 'Done.', truncated: false });
    state = applyConversationEvent(state, { type: 'done', reason: 'completed' });
    expect(state.messages.at(-1)).toMatchObject({ content: 'Done.', usage: { tokensIn: 800, tokensOut: 150 } });
    // A priced event with no token counts is still real usage.
    const priced = applyConversationEvent(beginConversationTurn(EMPTY_CONVERSATION, 'p', 'x'), {
      type: 'usage',
      tokens_in: 0,
      tokens_out: 0,
      cost_usd: 0.002,
    });
    expect(priced.messages.at(-1)?.usage).toMatchObject({ costUsd: 0.002 });
  });
});
