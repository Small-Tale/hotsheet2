/**
 * Whether a terminal's AI session is connected to Hot Sheet (HS2-EV1XK3). An AI tool runs Hot
 * Sheet's project hooks only once they are installed and trusted (Codex reviews each new or changed
 * hook in `/hooks`), so a session whose `SessionStart` or interactive permission hook reported in
 * sends its permission prompts to Hot Sheet. An AI terminal that has not reported in after a short
 * grace period keeps its prompts to itself.
 */
import type { TerminalAiConnection } from './api';

/** How long an AI terminal may run before a missing hook report counts as not connected. */
export const AI_CONNECTION_GRACE_MS = 15_000;

/** `connected`: the session's hook reported in. `missing`: an AI terminal never did. */
export type AiConnectionState = 'connected' | 'missing';

interface ConnectableSession {
  id: string;
  kind?: 'shell' | 'ai';
  alive: boolean;
  ai_connection?: TerminalAiConnection;
  aiConnection?: AiConnectionState;
}

interface ConnectableGroup<S extends ConnectableSession> {
  projectId: string;
  sessions: S[];
}

/** The connection state of one session that has been visible since `firstSeenAt`. */
export function aiConnectionState(
  session: ConnectableSession,
  firstSeenAt: number,
  now: number,
): AiConnectionState | 'pending' | undefined {
  if (session.ai_connection) return 'connected';
  // A plain shell may never run an AI tool, and an exited terminal has nothing left to connect.
  if (session.kind !== 'ai' || !session.alive) return undefined;
  return now - firstSeenAt >= AI_CONNECTION_GRACE_MS ? 'missing' : 'pending';
}

/**
 * Stamp every session with its `aiConnection` state. `firstSeen` remembers when each terminal first
 * appeared (keyed by project and terminal id) and forgets terminals that are gone. `nextCheckInMs`
 * is when the earliest pending grace period ends, so the caller can re-derive locally then.
 */
export function deriveAiConnectionStates<S extends ConnectableSession, G extends ConnectableGroup<S>>(
  groups: readonly G[],
  firstSeen: Map<string, number>,
  now: number,
): { groups: G[]; nextCheckInMs?: number } {
  const present = new Set<string>();
  let nextCheckInMs: number | undefined;
  const derived = groups.map((group) => ({
    ...group,
    sessions: group.sessions.map((session) => {
      const key = `${group.projectId}\u0000${session.id}`;
      present.add(key);
      if (!firstSeen.has(key)) firstSeen.set(key, now);
      const firstSeenAt = firstSeen.get(key)!,
        state = aiConnectionState(session, firstSeenAt, now);
      if (state === 'pending') {
        const remaining = firstSeenAt + AI_CONNECTION_GRACE_MS - now;
        nextCheckInMs = nextCheckInMs === undefined ? remaining : Math.min(nextCheckInMs, remaining);
      }
      const next: S = { ...session };
      if (state === 'connected' || state === 'missing') next.aiConnection = state;
      else delete next.aiConnection;
      return next;
    }),
  }));
  for (const key of [...firstSeen.keys()]) if (!present.has(key)) firstSeen.delete(key);
  return { groups: derived, ...(nextCheckInMs === undefined ? {} : { nextCheckInMs }) };
}

/** The tab label for a terminal's connection state: who is connected, or how to connect. */
export function aiConnectionLabel(state: AiConnectionState, agent?: string, tool?: string): string {
  if (state === 'connected')
    return `${agentName(agent ?? tool) ?? 'The AI session'} is connected to Hot Sheet: its permission prompts come to the app`;
  const name = agentName(tool) ?? 'This AI session';
  const fix =
    tool === 'codex'
      ? 'Run /hooks in Codex to review and trust Hot Sheet’s hooks, then restart the session.'
      : 'Run hotsheet-cli setup --refresh for this project, then restart the session.';
  return `${name} is not connected to Hot Sheet: its permission prompts stay in this terminal. ${fix}`;
}

function agentName(agent?: string): string | undefined {
  if (!agent) return undefined;
  if (agent === 'codex') return 'Codex';
  if (agent === 'claude') return 'Claude';
  return agent;
}
