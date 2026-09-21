import { describe, expect, it, vi } from 'vitest';

import type { ToolConnection } from './api';
import {compatibleAiEffort, prepareProjectConversation,projectChatConnectionId,projectDriveConnection, projectDriveControlState,recoverProjectConnections,restoreDrawerAIChats,runProjectDrive, SIDEBAR_DRIVE_PROMPT, sidebarDriveConnectionId } from './project-drive';

const checkout='checkout-1';
const connection = (value: Partial<ToolConnection> = {}): ToolConnection => ({
  id: sidebarDriveConnectionId(checkout,'codex'), tool: 'codex', project: '/project', role: 'main', busy: false, actions: ['send_turn', 'interrupt'], ...value,
});

describe('production project drive control', () => {
  it('recreates the newest durable connection sessions after a server restart (HS2-YHQCS2)',async()=>{
    const createToolConnection=vi.fn().mockImplementation(async value=>connection({id:value.connection_id,tool:value.tool,session_id:value.session_id}));
    const active=connection({id:'already-live'}),sessions=[
      {connection_id:'hotsheet-drawer-chat-restored',tool:'codex',project:'/repo',session_id:'newest',updated_at_ms:3},
      {connection_id:'hotsheet-drawer-chat-restored',tool:'codex',project:'/repo',session_id:'older',updated_at_ms:2},
      {connection_id:'other-project',tool:'claude',project:'/elsewhere',session_id:'elsewhere',updated_at_ms:1},
      {connection_id:'background-command',tool:'codex',project:'/repo',session_id:'worker',updated_at_ms:4},
    ];
    const recovered=await recoverProjectConnections({createToolConnection},[active],sessions,'checkout','/repo');
    expect(recovered.map(item=>item.id)).toEqual(['already-live','hotsheet-drawer-chat-restored']);
    expect(createToolConnection).toHaveBeenCalledOnce();
    expect(createToolConnection).toHaveBeenCalledWith({tool:'codex',checkout:'checkout',connection_id:'hotsheet-drawer-chat-restored',session_id:'newest'});
  });
  it('keeps project chat separate from the dedicated driven session',()=>{
    expect(projectChatConnectionId(checkout,'codex')).not.toBe(sidebarDriveConnectionId(checkout,'codex'));
  });
  it('drops effort for unsupported models and chooses only compatible fallbacks',()=>{
    expect(compatibleAiEffort([], 'medium')).toBeUndefined();
    expect(compatibleAiEffort(['low','high'],'medium','high')).toBe('high');
    expect(compatibleAiEffort(['low','high'],'medium')).toBe('low');
  });
  it('restores eligible server-side chats with connection metadata and stable tab identities',()=>{
    const plain=connection({id:'hotsheet-drawer-chat-01',tool:'claude',session_id:'claude-thread',model:'opus',effort:'high'}),drive=connection({session_id:'codex-thread',model:'gpt-6-astra',effort:'medium'}),saved=connection({id:'hotsheet-saved-chat-01',session_id:'saved-thread'}),existing={id:'ai-chat:hotsheet-drawer-chat-01',connectionId:plain.id,tool:'claude',name:'Claude chat',model:'stale',savedSource:'/export'};
    expect(restoreDrawerAIChats([plain,drive,saved],checkout,[existing],tool=>tool.toUpperCase())).toEqual([
      {...existing,model:'opus',effort:'high',readOnly:false},
      {id:`ai-chat:${drive.id}`,connectionId:drive.id,tool:'codex',name:'CODEX Drive',model:'gpt-6-astra',effort:'medium',drive:true,readOnly:false},
      {id:`ai-chat:${saved.id}`,connectionId:saved.id,tool:'codex',name:'CODEX saved chat',readOnly:false},
    ]);
  });
  it('does not project modal project chats, terminal connections, or unrelated driven work',()=>{
    const local={id:'ai-chat:local',connectionId:'local',tool:'codex',name:'Local saved chat',localOnly:true};
    const ineligible=[connection({id:projectChatConnectionId(checkout,'codex')}),connection({id:'terminal-1'}),connection({id:'hotsheet-drawer-chat-worker',role:'worker'})];
    expect(restoreDrawerAIChats(ineligible,checkout,[local])).toEqual([local]);
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
