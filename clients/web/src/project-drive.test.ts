import { describe, expect, it, vi } from 'vitest';

import type { ToolConnection } from './api';
import { prepareProjectConversation,projectDriveConnection, projectDriveControlState, SIDEBAR_DRIVE_PROMPT, sidebarDriveConnectionId, toggleProjectDrive } from './project-drive';

const checkout='checkout-1';
const connection = (value: Partial<ToolConnection> = {}): ToolConnection => ({
  id: sidebarDriveConnectionId(checkout,'codex'), tool: 'codex', project: '/project', role: 'main', busy: false, actions: ['send_turn', 'interrupt'], ...value,
});

describe('production project drive control', () => {
  it('projects a busy shared connection and disables unsupported interruption', () => {
    const shared = connection({ busy: true, actions: undefined });
    const autonomous = connection({ id: 'autonomous', busy: true });
    expect(projectDriveConnection([autonomous,shared],checkout)).toBe(shared);
    expect(projectDriveControlState([shared],checkout)).toEqual({ connection: shared, running: true, disabled: true, disabledReason: 'This Codex connection cannot be stopped from Hot Sheet.' });
    expect(projectDriveControlState([],checkout,true,'claude')).toMatchObject({ running: false, disabled: true, disabledReason:'Updating the Claude connection…' });
  });

  it('prepares the stable sidebar connection and sends the Hot Sheet workflow turn', async () => {
    const prepared = connection(), running = connection({ busy: true });
    const client = { createToolConnection: vi.fn().mockResolvedValue(prepared), sendToolTurn: vi.fn().mockResolvedValue(running), interruptToolTurn: vi.fn() };
    await expect(toggleProjectDrive(client, [],checkout)).resolves.toBe(running);
    expect(client.createToolConnection).toHaveBeenCalledWith({ tool: 'codex', checkout, connection_id: sidebarDriveConnectionId(checkout,'codex') });
    expect(client.sendToolTurn).toHaveBeenCalledWith(sidebarDriveConnectionId(checkout,'codex'), SIDEBAR_DRIVE_PROMPT);
  });

  it('resumes the prepared sidebar connection and interrupts its next busy state', async () => {
    const idle = connection({ session_id: 'thread-1' }), running = connection({ busy: true, session_id: 'thread-1' }), interrupted = connection({ busy: true, session_id: 'thread-1' });
    const client = { createToolConnection: vi.fn(), sendToolTurn: vi.fn().mockResolvedValue(running), interruptToolTurn: vi.fn().mockResolvedValue(interrupted) };
    await toggleProjectDrive(client, [idle],checkout);
    expect(client.createToolConnection).not.toHaveBeenCalled();
    expect(client.sendToolTurn).toHaveBeenCalledWith(idle.id, SIDEBAR_DRIVE_PROMPT);
    await toggleProjectDrive(client, [running],checkout);
    expect(client.interruptToolTurn).toHaveBeenCalledWith(running.id);
  });

  it('prepares chat without sending the workflow and keeps tools independent',async()=>{
    const claude=connection({id:sidebarDriveConnectionId(checkout,'claude'),tool:'claude'}),client={createToolConnection:vi.fn().mockResolvedValue(claude),sendToolTurn:vi.fn(),interruptToolTurn:vi.fn()};
    await expect(prepareProjectConversation(client,[],checkout,'claude')).resolves.toBe(claude);
    expect(client.createToolConnection).toHaveBeenCalledWith({tool:'claude',checkout,connection_id:sidebarDriveConnectionId(checkout,'claude')});
    expect(client.sendToolTurn).not.toHaveBeenCalled();
  });
});
