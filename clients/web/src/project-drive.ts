import type { Api, ToolConnection } from './api';

export const SIDEBAR_DRIVE_CONNECTION_ID = 'hotsheet-sidebar';
export const PROJECT_CHAT_CONNECTION_ID = 'hotsheet-project-chat';
export const SIDEBAR_DRIVE_PROMPT = '$hotsheet';
export type ProjectDriveTool = string;

type ProjectDriveClient = Pick<Api, 'createToolConnection' | 'sendToolTurn'>;

export interface ProjectDriveControlState {
  connection?: ToolConnection;
  running: boolean;
  disabled: boolean;
  disabledReason?: string;
}

export function sidebarDriveConnectionId(checkout: string, tool: ProjectDriveTool): string {
  return `${SIDEBAR_DRIVE_CONNECTION_ID}-${tool}-${checkout}`;
}

export function projectChatConnectionId(checkout:string,tool:ProjectDriveTool):string{
  return `${PROJECT_CHAT_CONNECTION_ID}-${tool}-${checkout}`;
}

export function projectDriveConnection(connections: readonly ToolConnection[], checkout: string, tool: ProjectDriveTool = 'codex'): ToolConnection | undefined {
  return connections.find(connection => connection.id === sidebarDriveConnectionId(checkout, tool));
}

export function projectDriveControlState(connections: readonly ToolConnection[], checkout: string, pending = false, tool: ProjectDriveTool = 'codex'): ProjectDriveControlState {
  const connection = projectDriveConnection(connections, checkout, tool);
  const label = `${tool.slice(0,1).toUpperCase()}${tool.slice(1)}`;
  if (pending) return { connection, running: Boolean(connection?.busy), disabled: true, disabledReason: `Updating the ${label} connection…` };
  if (connection?.busy)return {connection,running:true,disabled:true,disabledReason:`The ${label} workflow is already running in its chat tab.`};
  return { connection, running: Boolean(connection?.busy), disabled: false };
}

export async function prepareProjectConversation(client: ProjectDriveClient, connections: readonly ToolConnection[], checkout: string, tool: ProjectDriveTool = 'codex',options:{connectionId?:string;model?:string;effort?:string}={}): Promise<ToolConnection> {
  const id = options.connectionId??sidebarDriveConnectionId(checkout, tool);
  const connection = connections.find(item=>item.id===id);
  if (connection?.actions?.includes('send_turn')) return connection;
  const prepared = await client.createToolConnection({ tool, checkout, connection_id: id,...(options.model?{model:options.model}:{}),...(options.effort?{effort:options.effort}:{}) });
  if (!prepared.actions?.includes('send_turn')) throw new Error(`This ${tool === 'codex' ? 'Codex' : 'Claude'} connection cannot accept a turn.`);
  return prepared;
}

export async function runProjectDrive(client:ProjectDriveClient,connections:readonly ToolConnection[],checkout:string,tool:ProjectDriveTool='codex',selection:{model?:string;effort?:string}={}):Promise<ToolConnection>{
  const existing=projectDriveConnection(connections,checkout,tool);
  if(existing?.busy)throw new Error(`This ${tool} drive session is already running.`);
  const prepared=existing?.actions?.includes('send_turn')?existing:await client.createToolConnection({tool,checkout,connection_id:sidebarDriveConnectionId(checkout,tool),...selection});
  if(!prepared.actions?.includes('send_turn'))throw new Error(`This ${tool} connection cannot accept a turn.`);
  return client.sendToolTurn(prepared.id,SIDEBAR_DRIVE_PROMPT,prepared.session_id,selection);
}
