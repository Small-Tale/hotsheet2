import type { CommandDefinition, ToolConnection } from './api';

export const HOTSHEET_SKILL_SIGNAL = '$hotsheet';

/** Manual and scheduled tickets share this provenance for the same-command guard (HS2-G5J2E7). */
export const customAiCommandTag = (commandId: string) => `hs2-command:${commandId}`;

export function customAiCommandTicket(command: CommandDefinition) {
  return {
    title: command.title.trim(),
    details: command.prompt?.trim() ?? '',
    category: 'task',
    priority: 'urgent',
    up_next: true,
    tags: [customAiCommandTag(command.id)],
  };
}

export function customAiCommandSignalConnection(
  connections: readonly ToolConnection[],
  commandTool: string | undefined,
  defaultTool: string,
): ToolConnection | undefined {
  const tool = commandTool?.trim() || defaultTool;
  const available = connections.filter(
    (connection) =>
      connection.tool.toLowerCase() === tool.toLowerCase() &&
      !connection.busy &&
      connection.actions?.includes('send_turn'),
  );
  return available.find((connection) => connection.role === 'main') ?? available[0];
}
