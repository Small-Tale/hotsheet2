import { describe, expect, it } from 'vitest';

import type { CommandDefinition } from './api';
import { activelyScheduledCommandIds, type ScheduleView, sidebarCommandDefinitions } from './command-schedules';

const view = (commandId: string, paused = false): ScheduleView => ({
  schedule: {
    command_id: commandId,
    time_zone: 'UTC',
    local_time: '09:00',
    paused,
    recurrence: { kind: 'daily', every: 1, start_date: '2027-02-15' },
  },
  next_runs: [],
  history: [],
});

const commands: CommandDefinition[] = [
  { id: 'lint', title: 'Lint', kind: 'shell', command: 'npm run lint' },
  { id: 'nightly', title: 'Nightly', kind: 'ai', prompt: 'Review the day' },
  { id: 'review', title: 'Review', kind: 'ai', prompt: 'Review' },
  { id: 'build', title: 'Build', program: 'npm', args: ['run', 'build'] },
];
const ids = (items: readonly CommandDefinition[]) => items.map((item) => item.id);

describe('HS2-MA31ZN scheduled commands in the sidebar', () => {
  it('counts only saved, unpaused schedules as active', () => {
    expect([...activelyScheduledCommandIds([view('nightly'), view('review', true)])]).toEqual(['nightly']);
    expect(activelyScheduledCommandIds([]).size).toBe(0);
  });

  it('hides an actively scheduled AI command by default and keeps the order of the rest', () => {
    expect(ids(sidebarCommandDefinitions(commands, [view('nightly')]))).toEqual(['lint', 'review', 'build']);
    // No schedules, or a schedule whose command is gone, hides nothing.
    expect(ids(sidebarCommandDefinitions(commands, []))).toEqual(ids(commands));
    expect(ids(sidebarCommandDefinitions(commands, [view('deleted-command')]))).toEqual(ids(commands));
  });

  it('shows a paused schedule, and an opted-in scheduled command, like any other', () => {
    expect(ids(sidebarCommandDefinitions(commands, [view('nightly', true)]))).toEqual(ids(commands));
    const optedIn = commands.map((command) =>
      command.id === 'nightly' ? { ...command, show_when_scheduled: true } : command,
    );
    expect(ids(sidebarCommandDefinitions(optedIn, [view('nightly')]))).toEqual(ids(commands));
  });

  it('follows schedule transitions: create, pause, resume, delete, then schedule again', () => {
    const visible = (views: ScheduleView[]) => ids(sidebarCommandDefinitions(commands, views)).includes('nightly');
    const sequence = [[view('nightly')], [view('nightly', true)], [view('nightly')], [], [view('nightly')]];
    expect(sequence.map(visible)).toEqual([false, true, false, true, false]);
  });

  it('never hides a shell or program command, even if a stale schedule names it', () => {
    expect(ids(sidebarCommandDefinitions(commands, [view('lint'), view('build')]))).toEqual(ids(commands));
  });
});
