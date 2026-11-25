import type { AiCommandRun, CommandDefinition, ToolConnection } from './api';

/** The toast after an AI command ran through its configured action (HS2-17Y2EC). */
export function aiCommandRunMessage(result: AiCommandRun, toolLabel: (tool: string) => string): string {
  const slug = result.ticket.slug;
  if (result.delivery === 'queued') return `Queued ${slug}.`;
  if (result.delivery === 'started') {
    const tool = result.run.launch?.tool;
    return `Started ${slug} in ${tool ? `a ${toolLabel(tool)}` : 'an AI'} session.`;
  }
  const reason = result.reason ? (RUN_FAILURE_REASONS[result.reason] ?? result.reason) : undefined;
  return `Queued ${slug}; no AI session could start it${reason ? ` (${reason})` : ''}.`;
}

/** Plain words for the host's hand-off failure codes; an unknown code is shown as sent. */
const RUN_FAILURE_REASONS: Record<string, string> = {
  ai_provider_unavailable: 'no AI tool is available',
  ai_handoff_failed: 'the AI session did not start',
  ticket_claimed: 'another worker holds the ticket',
  ticket_claim_failed: 'the ticket could not be claimed',
  ticket_source_unavailable: 'the ticket source is unavailable',
  ticket_unavailable: 'the ticket is unavailable',
};

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
