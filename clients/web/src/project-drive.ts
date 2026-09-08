import type { Api, ToolConnection } from './api';

export const SIDEBAR_DRIVE_CONNECTION_ID = 'hotsheet-sidebar';
export const SIDEBAR_DRIVE_PROMPT = '$hotsheet';

type ProjectDriveClient = Pick<Api, 'createToolConnection' | 'sendToolTurn' | 'interruptToolTurn'>;

export interface ProjectDriveControlState {
  connection?: ToolConnection;
  running: boolean;
  disabled: boolean;
  disabledReason?: string;
}

export function projectDriveConnection(connections: readonly ToolConnection[], tool = 'codex'): ToolConnection | undefined {
  const matching = connections.filter(connection => connection.tool.toLowerCase() === tool.toLowerCase());
  return matching.find(connection => connection.busy)
    ?? matching.find(connection => connection.id === SIDEBAR_DRIVE_CONNECTION_ID);
}

export function projectDriveControlState(connections: readonly ToolConnection[], pending = false, tool = 'codex'): ProjectDriveControlState {
  const connection = projectDriveConnection(connections, tool);
  if (pending) return { connection, running: Boolean(connection?.busy), disabled: true, disabledReason: 'Updating the Codex connection…' };
  if (connection?.busy && !connection.actions?.includes('interrupt')) {
    return { connection, running: true, disabled: true, disabledReason: 'This Codex connection cannot be stopped from Hot Sheet.' };
  }
  return { connection, running: Boolean(connection?.busy), disabled: false };
}

export async function toggleProjectDrive(client: ProjectDriveClient, connections: readonly ToolConnection[], tool = 'codex'): Promise<ToolConnection> {
  const connection = projectDriveConnection(connections, tool);
  if (connection?.busy) {
    if (!connection.actions?.includes('interrupt')) throw new Error('This Codex connection cannot be stopped from Hot Sheet.');
    return client.interruptToolTurn(connection.id);
  }
  const prepared = connection?.id === SIDEBAR_DRIVE_CONNECTION_ID && connection.actions?.includes('send_turn')
    ? connection
    : await client.createToolConnection({ tool, connection_id: SIDEBAR_DRIVE_CONNECTION_ID });
  if (!prepared.actions?.includes('send_turn')) throw new Error('This Codex connection cannot accept a turn.');
  return client.sendToolTurn(prepared.id, SIDEBAR_DRIVE_PROMPT);
}
