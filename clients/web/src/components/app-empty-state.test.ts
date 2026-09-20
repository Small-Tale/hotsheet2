import { describe, expect, it } from 'vitest';

import { AppEmptyState, ProjectRestoreState } from './app-empty-state';

describe('app empty states', () => {
  it('offers project opening from the empty application', () => {
    const markup = String(AppEmptyState());
    expect(markup).toContain('Open a Hot Sheet project');
    expect(markup).toContain('data-action="add-project"');
  });

  it('announces remembered-project restoration as busy', () => {
    const markup = String(ProjectRestoreState());
    expect(markup).toContain('data-component="project-restore-state"');
    expect(markup).toContain('role="status"');
    expect(markup).toContain('aria-busy="true"');
  });
});
