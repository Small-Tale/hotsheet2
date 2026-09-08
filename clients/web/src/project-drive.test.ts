import { describe, expect, it, vi } from 'vitest';

import type { ToolConnection } from './api';
import { projectDriveConnection, projectDriveControlState, SIDEBAR_DRIVE_CONNECTION_ID, SIDEBAR_DRIVE_PROMPT, toggleProjectDrive } from './project-drive';

const connection = (value: Partial<ToolConnection> = {}): ToolConnection => ({
  id: SIDEBAR_DRIVE_CONNECTION_ID, tool: 'codex', project: '/project', role: 'main', busy: false, actions: ['send_turn', 'interrupt'], ...value,
});

describe('production project drive control', () => {
  it('projects a busy shared connection and disables unsupported interruption', () => {
    const autonomous = connection({ id: 'autonomous', busy: true, actions: undefined });
    expect(projectDriveConnection([connection(), autonomous])).toBe(autonomous);
    expect(projectDriveControlState([autonomous])).toEqual({ connection: autonomous, running: true, disabled: true, disabledReason: 'This Codex connection cannot be stopped from Hot Sheet.' });
    expect(projectDriveControlState([], true)).toMatchObject({ running: false, disabled: true });
  });

  it('prepares the stable sidebar connection and sends the Hot Sheet workflow turn', async () => {
    const prepared = connection(), running = connection({ busy: true });
    const client = { createToolConnection: vi.fn().mockResolvedValue(prepared), sendToolTurn: vi.fn().mockResolvedValue(running), interruptToolTurn: vi.fn() };
    await expect(toggleProjectDrive(client, [])).resolves.toBe(running);
    expect(client.createToolConnection).toHaveBeenCalledWith({ tool: 'codex', connection_id: SIDEBAR_DRIVE_CONNECTION_ID });
    expect(client.sendToolTurn).toHaveBeenCalledWith(SIDEBAR_DRIVE_CONNECTION_ID, SIDEBAR_DRIVE_PROMPT);
  });

  it('resumes the prepared sidebar connection and interrupts its next busy state', async () => {
    const idle = connection({ session_id: 'thread-1' }), running = connection({ busy: true, session_id: 'thread-1' }), interrupted = connection({ busy: true, session_id: 'thread-1' });
    const client = { createToolConnection: vi.fn(), sendToolTurn: vi.fn().mockResolvedValue(running), interruptToolTurn: vi.fn().mockResolvedValue(interrupted) };
    await toggleProjectDrive(client, [idle]);
    expect(client.createToolConnection).not.toHaveBeenCalled();
    expect(client.sendToolTurn).toHaveBeenCalledWith(idle.id, SIDEBAR_DRIVE_PROMPT);
    await toggleProjectDrive(client, [running]);
    expect(client.interruptToolTurn).toHaveBeenCalledWith(running.id);
  });
});
