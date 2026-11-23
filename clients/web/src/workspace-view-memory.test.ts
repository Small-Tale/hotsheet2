import { describe, expect, it } from 'vitest';

import type { WorkspaceViewMode } from './components/workspace-header';
import {
  DEFAULT_ORCHESTRATION_PROJECT_VIEW,
  isOrchestrationProjectViewMode,
  projectArrivalView,
  rememberOrchestrationProjectView,
} from './workspace-view-memory';

describe('the view orchestration projects remember (HS2-BRRSYM)', () => {
  it('shows the Orchestration view on a first visit', () => {
    expect(DEFAULT_ORCHESTRATION_PROJECT_VIEW).toBe('orchestration');
    expect(projectArrivalView('list', DEFAULT_ORCHESTRATION_PROJECT_VIEW)).toBe('orchestration');
    expect(projectArrivalView('board', DEFAULT_ORCHESTRATION_PROJECT_VIEW)).toBe('orchestration');
  });

  it('swaps only a project view, never Notifications or Settings', () => {
    for (const mode of ['notifications', 'settings'] as const) {
      expect(isOrchestrationProjectViewMode(mode)).toBe(false);
      expect(projectArrivalView(mode, 'orchestration')).toBeUndefined();
      expect(rememberOrchestrationProjectView(mode, 'board')).toBe('board');
    }
    expect(projectArrivalView('orchestration', 'orchestration')).toBeUndefined();
  });

  it('walks orchestration → software → orchestration back to the Orchestration view', () => {
    let remembered = DEFAULT_ORCHESTRATION_PROJECT_VIEW,
      view: WorkspaceViewMode = 'orchestration';
    remembered = rememberOrchestrationProjectView(view, remembered);
    // The software project shows its own ticket view (the existing fallback), which is not remembered here.
    view = 'board';
    view = projectArrivalView(view, remembered) ?? view;
    expect(view).toBe('orchestration');
  });

  it('restores a software project’s own ticket view on arrival from an orchestration project', () => {
    expect(projectArrivalView('orchestration', 'board')).toBe('board');
    expect(projectArrivalView('list', 'board')).toBe('board');
    expect(projectArrivalView('board', 'board')).toBeUndefined();
  });

  it('keeps a ticket view chosen in an orchestration project apart from the software projects’ one', () => {
    let remembered = rememberOrchestrationProjectView('list', DEFAULT_ORCHESTRATION_PROJECT_VIEW);
    expect(remembered).toBe('list');
    // Back from a software project in Columns: orchestration projects show their List again.
    expect(projectArrivalView('board', remembered)).toBe('list');
    expect(projectArrivalView('list', remembered)).toBeUndefined();
    remembered = rememberOrchestrationProjectView('workers', remembered);
    expect(projectArrivalView('orchestration', remembered)).toBe('workers');
    remembered = rememberOrchestrationProjectView('orchestration', remembered);
    expect(projectArrivalView('workers', remembered)).toBe('orchestration');
  });
});
