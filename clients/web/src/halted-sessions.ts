import { conversationError, type ConversationState } from './ai-conversation';
import type { ToolConnection } from './api';
import type { TerminalDashboardGroup } from './components/terminal-dashboard';
import type { Project } from './interactions/types';
import type { DrawerAIChat } from './project-drive';

export interface HaltedSessionEpisode {
  key: string;
  kind: 'terminal' | 'chat';
  projectId: string;
  projectName: string;
  sessionId: string;
  sessionName: string;
  message: string;
}

/** Only currently halted terminals and completed failed turns are interruptive episodes. */
export function projectHaltedSessions(
  projects: readonly Project[],
  groups: readonly TerminalDashboardGroup[],
  chats: Readonly<Record<string, DrawerAIChat[]>>,
  connections: Readonly<Record<string, ToolConnection[]>>,
  conversations: Readonly<Record<string, ConversationState>>,
): HaltedSessionEpisode[] {
  return projects.flatMap((project) => [
    ...(groups.find((group) => group.projectId === project.id)?.sessions ?? []).flatMap((session) =>
      session.halt
        ? [
            {
              key: JSON.stringify(['terminal', project.id, session.id, session.halt.at]),
              kind: 'terminal' as const,
              projectId: project.id,
              projectName: project.name,
              sessionId: session.id,
              sessionName: session.title ?? session.id,
              message: session.halt.message,
            },
          ]
        : [],
    ),
    ...(chats[project.id] ?? []).flatMap((chat) => {
      const state = conversations[chat.connectionId] as ConversationState | undefined;
      if (!state) return [];
      const projectConnections = connections[project.id] as ToolConnection[] | undefined,
        assistant = [...state.messages].reverse().find((message) => message.role === 'assistant'),
        connection = projectConnections?.find((item) => item.id === chat.connectionId),
        message = conversationError(state, connection);
      if (assistant?.status !== 'failed' || !message) return [];
      return [
        {
          key: JSON.stringify(['chat', project.id, chat.id, assistant.id]),
          kind: 'chat' as const,
          projectId: project.id,
          projectName: project.name,
          sessionId: chat.id,
          sessionName: chat.name,
          message,
        },
      ];
    }),
  ]);
}

export const HALTED_SESSION_SEEN_KEY = 'hotsheet.halted-session-seen';
export const HISTORICAL_SEEN_LIMIT = 256;
type SeenStorage = Pick<Storage, 'getItem' | 'setItem'>;

export function parseHaltedSessionSeen(value: string | null): string[] {
  try {
    const parsed: unknown = JSON.parse(value ?? '[]');
    return Array.isArray(parsed) ? [...new Set(parsed.filter((item): item is string => typeof item === 'string'))] : [];
  } catch {
    return [];
  }
}

/** Active acknowledged episodes survive pruning; only inactive history is capped. */
export class HaltedSessionInbox {
  private episodes: HaltedSessionEpisode[] = [];
  private seen: Set<string>;
  private currentKey: string | undefined;
  private unresolvedProjects = new Set<string>();
  private restoring = false;

  constructor(seen: readonly string[] = []) {
    this.seen = new Set(seen);
  }

  reconcile(
    episodes: readonly HaltedSessionEpisode[],
    unresolvedProjects: readonly string[] = [],
    restoring = false,
  ): void {
    this.restoring = restoring;
    this.unresolvedProjects = new Set(unresolvedProjects);
    this.episodes = [...new Map(episodes.map((episode) => [episode.key, episode])).values()];
    if (!this.episodes.some((episode) => episode.key === this.currentKey)) this.currentKey = undefined;
    this.prune();
  }

  visible(paused: boolean, permissionVisible: boolean): HaltedSessionEpisode | undefined {
    if (paused) {
      this.currentKey = undefined;
      return;
    }
    if (permissionVisible) return;
    const current = this.episodes.find((episode) => episode.key === this.currentKey);
    if (current) return current;
    const next = this.episodes.find((episode) => !this.seen.has(episode.key));
    this.currentKey = next?.key;
    return next;
  }

  presented(key: string): boolean {
    if (this.currentKey !== key || !this.episodes.some((episode) => episode.key === key) || this.seen.has(key))
      return false;
    this.seen.add(key);
    this.prune();
    return true;
  }

  dismiss(key: string): void {
    if (this.currentKey === key) this.currentKey = undefined;
  }

  find(key: string): HaltedSessionEpisode | undefined {
    return this.episodes.find((episode) => episode.key === key);
  }

  waiting(): number {
    return this.episodes.filter((episode) => !this.seen.has(episode.key)).length;
  }

  mergeSeen(keys: readonly string[]): void {
    // Another window's acknowledgment removes only an as-yet-unpresented local selection.
    if (this.currentKey && !this.seen.has(this.currentKey) && keys.includes(this.currentKey))
      this.currentKey = undefined;
    for (const key of keys) this.seen.add(key);
    this.prune();
  }

  seenKeys(): string[] {
    return [...this.seen];
  }

  private prune(): void {
    // The open-project registry has not settled; saved active identities may belong to a project
    // that is not registered yet. Prune once restoration establishes which projects still exist.
    if (this.restoring) return;
    const active = new Set(this.episodes.map((episode) => episode.key)),
      retained = (key: string) => {
        if (active.has(key)) return true;
        try {
          const identity: unknown = JSON.parse(key);
          return Array.isArray(identity) && typeof identity[1] === 'string' && this.unresolvedProjects.has(identity[1]);
        } catch {
          return false;
        }
      },
      historical = new Set([...this.seen].filter((key) => !retained(key)).slice(-HISTORICAL_SEEN_LIMIT));
    this.seen = new Set([...this.seen].filter((key) => retained(key) || historical.has(key)));
  }
}

export function loadHaltedSessionSeen(storage: SeenStorage | undefined): string[] {
  try {
    return parseHaltedSessionSeen(storage?.getItem(HALTED_SESSION_SEEN_KEY) ?? null);
  } catch {
    return [];
  }
}

/** Merge before writing so a window doesn't overwrite acknowledgments it hasn't received yet. */
export function persistHaltedSessionSeen(inbox: HaltedSessionInbox, storage: SeenStorage | undefined): void {
  inbox.mergeSeen(loadHaltedSessionSeen(storage));
  try {
    storage?.setItem(HALTED_SESSION_SEEN_KEY, JSON.stringify(inbox.seenKeys()));
  } catch {
    // The live window retains acknowledgment even when this device cannot persist it.
  }
}

/** Converge overlapping window writes without echoing an already-complete storage event. */
export function synchronizeHaltedSessionSeen(
  inbox: HaltedSessionInbox,
  value: string | null,
  storage: SeenStorage | undefined,
): void {
  const incoming = parseHaltedSessionSeen(value);
  inbox.mergeSeen(incoming);
  if (value !== null && inbox.seenKeys().some((key) => !incoming.includes(key)))
    persistHaltedSessionSeen(inbox, storage);
}
