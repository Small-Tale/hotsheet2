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
