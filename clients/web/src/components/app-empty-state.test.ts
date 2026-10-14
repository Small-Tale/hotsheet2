import { describe, expect, it } from 'vitest';

import { AppEmptyState, ProjectOpeningState, ProjectRestoreState } from './app-empty-state';

describe('app empty states', () => {
  it('offers project opening from the empty application', () => {
    const markup = String(AppEmptyState());
    expect(markup).toContain('Open a Hot Sheet Project');
    expect(markup).toContain('data-action="add-project"');
  });

  it('announces remembered-project restoration as busy', () => {
    const markup = String(ProjectRestoreState());
    expect(markup).toContain('data-project-restore-state="true"');
    expect(markup).toContain('role="status"');
    expect(markup).toContain('aria-busy="true"');
  });

  it('names a selected still-opening project in a busy opening panel (HS2-XGSSK1)', () => {
    const markup = String(ProjectOpeningState({ name: 'alpha' }));
    expect(markup).toContain('data-project-opening-state="true"');
    expect(markup).not.toContain('data-project-restore-state');
    expect(markup).toContain('Opening alpha…');
    expect(markup).toContain('aria-busy="true"');
  });
});
