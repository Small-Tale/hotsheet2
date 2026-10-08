import { describe, expect, it } from 'vitest';

import {
  AI_CONNECTION_GRACE_MS,
  aiConnectionLabel,
  aiConnectionState,
  deriveAiConnectionStates,
} from './terminal-ai-connection';

const connection = { agent: 'codex', at: '2026-10-05T08:00:00Z' };

describe('terminal AI connection state (HS2-EV1XK3)', () => {
  it('classifies connected, pending, missing, and not-applicable sessions', () => {
    const ai = { id: 'a', kind: 'ai' as const, alive: true };
    expect(aiConnectionState({ ...ai, ai_connection: connection }, 0, 0)).toBe('connected');
    // A plain shell running codex by hand is connected too, once its hook reports in.
    expect(aiConnectionState({ id: 's', kind: 'shell', alive: true, ai_connection: connection }, 0, 0)).toBe(
      'connected',
    );
    expect(aiConnectionState(ai, 0, AI_CONNECTION_GRACE_MS - 1)).toBe('pending');
    expect(aiConnectionState(ai, 0, AI_CONNECTION_GRACE_MS)).toBe('missing');
    expect(aiConnectionState({ ...ai, alive: false }, 0, AI_CONNECTION_GRACE_MS)).toBeUndefined();
    expect(aiConnectionState({ id: 's', kind: 'shell', alive: true }, 0, AI_CONNECTION_GRACE_MS)).toBeUndefined();
    expect(aiConnectionState({ id: 'o', alive: true }, 0, AI_CONNECTION_GRACE_MS)).toBeUndefined();
  });

  it('walks pending → missing → connected → disconnected → removed → re-added across derivations', () => {
    const firstSeen = new Map<string, number>(),
      group = (sessions: Array<Record<string, unknown>>) => [
        { projectId: 'p', sessions: sessions as Array<{ id: string; alive: boolean; kind?: 'ai' | 'shell' }> },
      ],
      ai = { id: 'codex-1', kind: 'ai' as const, alive: true },
      shell = { id: 'shell-1', kind: 'shell' as const, alive: true };

    // First sight starts the grace period; the caller re-checks when it ends.
    let result = deriveAiConnectionStates(group([ai, shell]), firstSeen, 1000);
    expect(result.groups[0].sessions.map((session) => (session as { aiConnection?: string }).aiConnection)).toEqual([
      undefined,
      undefined,
    ]);
    expect(result.nextCheckInMs).toBe(AI_CONNECTION_GRACE_MS);

    // A later derivation inside the grace keeps the original start.
    result = deriveAiConnectionStates(group([ai, shell]), firstSeen, 6000);
    expect(result.nextCheckInMs).toBe(AI_CONNECTION_GRACE_MS - 5000);

    // The grace ends without a report: missing, and nothing left to re-check.
    result = deriveAiConnectionStates(result.groups, firstSeen, 1000 + AI_CONNECTION_GRACE_MS);
    expect((result.groups[0].sessions[0] as { aiConnection?: string }).aiConnection).toBe('missing');
    expect(result.nextCheckInMs).toBeUndefined();

    // The hook reports in: connected; then the session ends: missing again, with no new grace.
    result = deriveAiConnectionStates(group([{ ...ai, ai_connection: connection }]), firstSeen, 20_000);
    expect((result.groups[0].sessions[0] as { aiConnection?: string }).aiConnection).toBe('connected');
    result = deriveAiConnectionStates(group([ai]), firstSeen, 21_000);
    expect((result.groups[0].sessions[0] as { aiConnection?: string }).aiConnection).toBe('missing');

    // A removed terminal is forgotten, so the same id re-added gets a fresh grace period.
    deriveAiConnectionStates(group([]), firstSeen, 22_000);
    expect(firstSeen.size).toBe(0);
    result = deriveAiConnectionStates(group([ai]), firstSeen, 23_000);
    expect((result.groups[0].sessions[0] as { aiConnection?: string }).aiConnection).toBeUndefined();
    expect(result.nextCheckInMs).toBe(AI_CONNECTION_GRACE_MS);
  });

  it('keys the grace period by project, so the same terminal id in two projects is tracked separately', () => {
    const firstSeen = new Map<string, number>(),
      ai = { id: 'codex-1', kind: 'ai' as const, alive: true };
    deriveAiConnectionStates([{ projectId: 'a', sessions: [ai] }], firstSeen, 0);
    const result = deriveAiConnectionStates(
      [
        { projectId: 'a', sessions: [ai] },
        { projectId: 'b', sessions: [ai] },
      ],
      firstSeen,
      AI_CONNECTION_GRACE_MS,
    );
    expect(result.groups.map((group) => (group.sessions[0] as { aiConnection?: string }).aiConnection)).toEqual([
      'missing',
      undefined,
    ]);
    expect(result.nextCheckInMs).toBe(AI_CONNECTION_GRACE_MS);
  });

  it('labels who is connected and how to connect an unconnected tool', () => {
    expect(aiConnectionLabel('connected', 'claude')).toContain(
      'Claude is connected to Hot Sheet: its permission prompts come to the app',
    );
    expect(aiConnectionLabel('connected', undefined, 'codex')).toContain('Codex is connected');
    expect(aiConnectionLabel('connected')).toContain('The AI session is connected');
    expect(aiConnectionLabel('missing', undefined, 'codex')).toContain('Run /hooks in Codex');
    expect(aiConnectionLabel('missing', undefined, 'claude')).toContain('hotsheet-cli setup --refresh');
    expect(aiConnectionLabel('missing', undefined, 'opencode')).toMatch(/^opencode is not connected/);
    expect(aiConnectionLabel('missing')).toMatch(/^This AI session is not connected/);
    expect(aiConnectionLabel('missing', undefined, 'codex', { ...connection, source: 'permission_request' })).toContain(
      'Last trusted hook report: PermissionRequest at 2026-10-05T08:00:00Z',
    );
    expect(aiConnectionLabel('missing')).toContain('MCP connection does not prove terminal hooks');
    expect(aiConnectionLabel('missing')).toContain('hotsheet-cli hook-diagnose');
  });
});
