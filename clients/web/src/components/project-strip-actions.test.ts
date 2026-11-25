import { describe, expect, it } from 'vitest';

import { AddProjectAction, type MinimizedProjectChoice, minimizedProjectMenuItem } from './project-strip-actions';
import { projectLocationIcon, projectStatusIndicators, ProjectTab } from './project-tab';

const active: MinimizedProjectChoice = {
  id: 'overview',
  name: 'Overview',
  location: 'local',
  upNextCount: 2,
  activeTicketCount: 3,
};
const remote: MinimizedProjectChoice = {
  id: 'remote:01ARZ3NDEKTSV4RRFFQ69G5FAV:domotion',
  name: 'Build host · Domotion',
  location: 'remote',
};

describe('Restore Project rows share the project tab presentation (HS2-R64JXA)', () => {
  it('uses the tab name, the cloud for a remote project, and a folder for a local one', () => {
    const local = minimizedProjectMenuItem(active),
      cloud = minimizedProjectMenuItem(remote);
    expect(local.label).toBe('Overview');
    expect(String(local.icon)).toContain('data-lucide="folder"');
    expect(cloud.label).toBe('Build host · Domotion');
    expect(String(cloud.icon)).toBe(String(projectLocationIcon('remote')));
    expect(String(cloud.icon)).toContain('data-lucide="cloud"');
    expect(projectLocationIcon('local')).toBeUndefined();
    expect(cloud.attributes).toEqual({ 'data-project-id': remote.id, 'data-location': 'remote' });
    expect(local.action).toBe('restore-minimized-project');
  });

  it('draws the same status indicators as the project tab', () => {
    const row = String(minimizedProjectMenuItem(active).details),
      tab = String(ProjectTab(active));
    expect(row).toBe(String(projectStatusIndicators(active)));
    expect(row).toContain('aria-label="2 Up Next tickets, 3 active tickets"');
    expect(row).toContain('project-tab__activity-ring');
    expect(tab).toContain(row);
    expect(minimizedProjectMenuItem(remote).details).toBeUndefined();
  });

  it('carries attention, disconnection, notifications, partial counts, and operations', () => {
    const status = {
      location: 'remote' as const,
      disconnected: true,
      attention: true,
      attentionLabel: 'An AI session needs your attention',
      notificationCount: 1,
      upNextCount: 1,
      countsPartial: true,
      operation: { label: 'Import needs attention', state: 'failed' as const },
    };
    const row = String(minimizedProjectMenuItem({ id: 'p', name: 'P', ...status }).details);
    expect(row).toContain('aria-label="An AI session needs your attention"');
    expect(row).toContain('aria-label="Server disconnected"');
    expect(row).toContain('aria-label="1 pending notification"');
    expect(row).toContain('aria-label="At least 1 Up Next ticket"');
    expect(row).toContain('data-state="failed"');
    expect(String(ProjectTab({ id: 'p', name: 'P', ...status }))).toContain(row);
    expect(String(projectStatusIndicators({ location: 'local', disconnected: true }))).toContain(
      'aria-label="Server disconnected"',
    );
    expect(String(projectStatusIndicators({ location: 'local', attention: true }))).toContain('circle-alert');
    expect(String(projectStatusIndicators({ location: 'local', busy: true }))).toContain('Project busy');
  });

  it('renders the shared rows in the shipped Restore Project menu', () => {
    const markup = String(AddProjectAction({ minimizedProjects: [active, remote] }));
    expect(markup).toContain('Restore Project');
    expect(markup).toContain('data-location="remote"');
    expect(markup).toContain('data-lucide="cloud"');
    expect(markup).toContain('aria-label="2 Up Next tickets, 3 active tickets"');
  });
});
