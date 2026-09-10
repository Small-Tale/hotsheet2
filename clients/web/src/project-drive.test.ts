import { describe, expect, it, vi } from 'vitest';

import type { ToolConnection } from './api';
import { prepareProjectConversation,projectChatConnectionId,projectDriveConnection, projectDriveControlState,runProjectDrive, SIDEBAR_DRIVE_PROMPT, sidebarDriveConnectionId } from './project-drive';

const checkout='checkout-1';
const connection = (value: Partial<ToolConnection> = {}): ToolConnection => ({
  id: sidebarDriveConnectionId(checkout,'codex'), tool: 'codex', project: '/project', role: 'main', busy: false, actions: ['send_turn', 'interrupt'], ...value,
});

describe('production project drive control', () => {
  it('keeps project chat separate from the dedicated driven session',()=>{
    expect(projectChatConnectionId(checkout,'codex')).not.toBe(sidebarDriveConnectionId(checkout,'codex'));
  });
  it('projects a busy shared connection and disables unsupported interruption', () => {
    const shared = connection({ busy: true, actions: undefined });
    const autonomous = connection({ id: 'autonomous', busy: true });
    expect(projectDriveConnection([autonomous,shared],checkout)).toBe(shared);
    expect(projectDriveControlState([shared],checkout)).toEqual({ connection: shared, running: true, disabled: true, disabledReason: 'The Codex workflow is already running in its chat tab.' });
    expect(projectDriveControlState([],checkout,true,'claude')).toMatchObject({ running: false, disabled: true, disabledReason:'Updating the Claude connection…' });
  });

  it('prepares chat without sending the workflow and keeps tools independent',async()=>{
    const id=projectChatConnectionId(checkout,'claude'),claude=connection({id,tool:'claude'}),client={createToolConnection:vi.fn().mockResolvedValue(claude),sendToolTurn:vi.fn(),interruptToolTurn:vi.fn()};
    await expect(prepareProjectConversation(client,[],checkout,'claude',{connectionId:id,model:'opus'})).resolves.toBe(claude);
    expect(client.createToolConnection).toHaveBeenCalledWith({tool:'claude',checkout,connection_id:id,model:'opus'});
    expect(client.sendToolTurn).not.toHaveBeenCalled();
  });

  it('reuses an idle driven session and applies model and effort to the next workflow turn',async()=>{
    const idle=connection({session_id:'thread-1'}),running=connection({busy:true,session_id:'thread-1',model:'gpt-5.6',effort:'high'});
    const client={createToolConnection:vi.fn(),sendToolTurn:vi.fn().mockResolvedValue(running),interruptToolTurn:vi.fn()};
    await expect(runProjectDrive(client,[idle],checkout,'codex',{model:'gpt-5.6',effort:'high'})).resolves.toBe(running);
    expect(client.createToolConnection).not.toHaveBeenCalled();
    expect(client.sendToolTurn).toHaveBeenCalledWith(idle.id,SIDEBAR_DRIVE_PROMPT,idle.session_id,{model:'gpt-5.6',effort:'high'});
  });
});
