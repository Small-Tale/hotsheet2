const ULID = /^[0-9A-HJKMNP-TV-Z]{26}$/i;

export function defaultTerminalName(id: string, index: number): string {
  if (ULID.test(id)) return `Terminal ${index + 1}`;
  const words = id
    .trim()
    .split(/[-_\s]+/)
    .filter(Boolean);
  if (words.length === 0) return `Terminal ${index + 1}`;
  return words
    .map((word) =>
      word.length <= 3 && word === word.toUpperCase() ? word : word.charAt(0).toUpperCase() + word.slice(1),
    )
    .join(' ');
}

/**
 * Default tab names for one project's terminals, in list order (HS2-HZK0NK). A generated-id AI
 * terminal is named after its provider and numbered among that provider's terminals ("Claude 1",
 * "Claude 2", "Codex 1"); a generated-id shell is numbered among the project's other unnamed
 * shells ("Terminal 1"). A readable id keeps its title-cased words. A saved rename always wins.
 */
export function defaultTerminalNames(
  sessions: readonly { id: string; tool?: string }[],
  toolLabel: (tool: string) => string,
): string[] {
  const counts = new Map<string, number>();
  return sessions.map((session) => {
    const readable = defaultTerminalName(session.id, 0);
    if (!ULID.test(session.id) && readable !== 'Terminal 1') return readable;
    const group = session.tool ?? '',
      ordinal = (counts.get(group) ?? 0) + 1;
    counts.set(group, ordinal);
    return session.tool ? `${toolLabel(session.tool)} ${ordinal}` : `Terminal ${ordinal}`;
  });
}

export function terminalNameKey(projectId: string, terminalId: string): string {
  return `${projectId}:${terminalId}`;
}

export function parseTerminalNames(raw: string | null): Record<string, string> {
  if (!raw) return {};
  try {
    const value = JSON.parse(raw) as unknown;
    if (!value || typeof value !== 'object' || Array.isArray(value)) return {};
    return Object.fromEntries(
      Object.entries(value)
        .filter((entry): entry is [string, string] => typeof entry[1] === 'string' && Boolean(entry[1].trim()))
        .map(([key, name]) => [key, name.trim()]),
    );
  } catch {
    return {};
  }
}

/**
 * A terminal tab's title (HS2-89FPV1). A browser-local name is only ever a rename that has not
 * reached the server yet (in flight, or saved before names moved to the server), so it wins; then
 * the server-saved name every client shares; then the derived default.
 */
export function terminalTitle(localName: string | undefined, serverName: string | undefined, fallback: string): string {
  return localName?.trim() || serverName?.trim() || fallback;
}

/** Drop one terminal's browser-local name, returning the same object when nothing changed. */
export function withoutTerminalName(names: Record<string, string>, key: string): Record<string, string> {
  if (!Object.hasOwn(names, key)) return names;
  return Object.fromEntries(Object.entries(names).filter(([candidate]) => candidate !== key));
}

/**
 * Reconcile settled browser-local names with what the server reports for one project's
 * terminals. A local name for a terminal the server has not named is uploaded once; a local name
 * for a terminal the server already names is stale (the server is authoritative) and is dropped.
 * Renames still in flight (`pending` keys) are left alone.
 */
export function reconcileLocalTerminalNames(
  projectId: string,
  sessions: readonly { id: string; name?: string }[],
  local: Readonly<Record<string, string>>,
  pending: ReadonlySet<string>,
): { upload: { terminalId: string; name: string }[]; drop: string[] } {
  const upload: { terminalId: string; name: string }[] = [],
    drop: string[] = [];
  for (const session of sessions) {
    const key = terminalNameKey(projectId, session.id),
      name = local[key];
    if (!name || pending.has(key)) continue;
    if (session.name) drop.push(key);
    else upload.push({ terminalId: session.id, name });
  }
  return { upload, drop };
}

/** Retitle one project's terminal in place (an optimistic rename or a `terminal_renamed` event). */
export function retitleTerminal<
  G extends { projectId: string; sessions: readonly { id: string; title?: string; named?: boolean }[] },
>(groups: readonly G[], projectId: string, terminalId: string, title: string): G[] {
  return updateTerminalSession(groups, projectId, terminalId, { title, named: true });
}

/**
 * Return one project's terminal to its derived default tab name (HS2-2Q7KTX), as the optimistic
 * half of clearing its saved name. A session without a recorded default is left unchanged.
 */
export function restoreDefaultTerminalTitle<
  G extends {
    projectId: string;
    sessions: readonly { id: string; title?: string; defaultTitle?: string; named?: boolean }[];
  },
>(groups: readonly G[], projectId: string, terminalId: string): G[] {
  const session = groups
    .find((group) => group.projectId === projectId)
    ?.sessions.find((item) => item.id === terminalId);
  if (!session?.defaultTitle) return groups as G[];
  return updateTerminalSession(groups, projectId, terminalId, { title: session.defaultTitle, named: false });
}

function updateTerminalSession<
  G extends { projectId: string; sessions: readonly { id: string; title?: string; named?: boolean }[] },
>(groups: readonly G[], projectId: string, terminalId: string, patch: { title: string; named: boolean }): G[] {
  const stale = (group: G) =>
    group.projectId === projectId &&
    group.sessions.some(
      (session) => session.id === terminalId && (session.title !== patch.title || session.named !== patch.named),
    );
  if (!groups.some(stale)) return groups as G[];
  return groups.map((group) =>
    stale(group)
      ? {
          ...group,
          sessions: group.sessions.map((session) => (session.id === terminalId ? { ...session, ...patch } : session)),
        }
      : group,
  );
}
