import type { Api, ToolConnection } from './api';

export const SIDEBAR_DRIVE_CONNECTION_ID = 'hotsheet-sidebar';
export const SIDEBAR_DRIVE_PROMPT = '$hotsheet';
export type ProjectDriveTool = 'codex' | 'claude';

type ProjectDriveClient = Pick<Api, 'createToolConnection' | 'sendToolTurn' | 'interruptToolTurn'>;

export interface ProjectDriveControlState {
  connection?: ToolConnection;
  running: boolean;
  disabled: boolean;
  disabledReason?: string;
}

export function sidebarDriveConnectionId(checkout: string, tool: ProjectDriveTool): string {
  return `${SIDEBAR_DRIVE_CONNECTION_ID}-${tool}-${checkout}`;
}

export function projectDriveConnection(connections: readonly ToolConnection[], checkout: string, tool: ProjectDriveTool = 'codex'): ToolConnection | undefined {
  return connections.find(connection => connection.id === sidebarDriveConnectionId(checkout, tool));
}

export function projectDriveControlState(connections: readonly ToolConnection[], checkout: string, pending = false, tool: ProjectDriveTool = 'codex'): ProjectDriveControlState {
  const connection = projectDriveConnection(connections, checkout, tool);
  const label = tool === 'codex' ? 'Codex' : 'Claude';
  if (pending) return { connection, running: Boolean(connection?.busy), disabled: true, disabledReason: `Updating the ${label} connection…` };
  if (connection?.busy && !connection.actions?.includes('interrupt')) {
    return { connection, running: true, disabled: true, disabledReason: `This ${label} connection cannot be stopped from Hot Sheet.` };
  }
  return { connection, running: Boolean(connection?.busy), disabled: false };
}

export async function prepareProjectConversation(client: ProjectDriveClient, connections: readonly ToolConnection[], checkout: string, tool: ProjectDriveTool = 'codex'): Promise<ToolConnection> {
  const id = sidebarDriveConnectionId(checkout, tool);
  const connection = projectDriveConnection(connections, checkout, tool);
  if (connection?.actions?.includes('send_turn')) return connection;
  const prepared = await client.createToolConnection({ tool, checkout, connection_id: id });
  if (!prepared.actions?.includes('send_turn')) throw new Error(`This ${tool === 'codex' ? 'Codex' : 'Claude'} connection cannot accept a turn.`);
  return prepared;
}

export async function toggleProjectDrive(client: ProjectDriveClient, connections: readonly ToolConnection[], checkout: string, tool: ProjectDriveTool = 'codex'): Promise<ToolConnection> {
  const connection = projectDriveConnection(connections, checkout, tool);
  const label = tool === 'codex' ? 'Codex' : 'Claude';
  if (connection?.busy) {
    if (!connection.actions?.includes('interrupt')) throw new Error(`This ${label} connection cannot be stopped from Hot Sheet.`);
    return client.interruptToolTurn(connection.id);
  }
  const prepared = await prepareProjectConversation(client, connections, checkout, tool);
  return client.sendToolTurn(prepared.id, SIDEBAR_DRIVE_PROMPT);
}
