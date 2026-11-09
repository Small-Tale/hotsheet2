import { signal } from 'kerfjs';
import { describe, expect, it, vi } from 'vitest';

import type { ProjectCloseDialogState } from '../components/project-close-dialog';
import type { Project } from '../interactions/types';
import { createProjectCloseController } from './project-close';

describe('HS2-7V2EEV failed remembered-project close', () => {
  function controller() {
    const projects = signal([{ id: 'healthy', name: 'Healthy', apiPath: '/api/healthy' } as Project]),
      projectRestoreFailures = signal([{ root: '/work/missing', name: 'Missing', error: 'Synthetic missing folder' }]),
      projectCloseDialog = signal<ProjectCloseDialogState | undefined>(undefined),
      closeProjectIds = vi.fn();
    const owner = createProjectCloseController({
      projects,
      projectRestoreFailures,
      projectCloseDialog,
      terminalGroups: signal([]),
      terminalDrawerChatsByProject: signal({}),
      driveConnectionsByProject: signal({}),
      conversationStates: signal({}),
      aiToolLabel: (tool) => tool,
      closeProjectIds,
    });
    return { owner, projectCloseDialog, closeProjectIds, projectRestoreFailures };
  }

  it('offers a no-resource confirmation for the failed synthetic tab id and closes only after confirmation', () => {
    const { owner, projectCloseDialog, closeProjectIds } = controller();
    owner.requestProjectClose(['project-restore-error:/work/missing', 'unknown']);
    expect(projectCloseDialog.value).toMatchObject({
      projectId: 'project-restore-error:/work/missing',
      projectName: 'Missing',
      resources: [],
    });
    expect(closeProjectIds).not.toHaveBeenCalled();
    owner.confirmProjectClose();
    expect(closeProjectIds).toHaveBeenCalledTimes(1);
    expect(closeProjectIds).toHaveBeenCalledWith(['project-restore-error:/work/missing']);
    expect(projectCloseDialog.value).toBeUndefined();
  });

  it('cancel leaves a failed root remembered and ordinary project closing still works', () => {
    const { owner, projectCloseDialog, closeProjectIds, projectRestoreFailures } = controller();
    owner.requestProjectClose(['project-restore-error:/work/missing']);
    owner.cancelProjectClose();
    expect(projectCloseDialog.value).toBeUndefined();
    expect(closeProjectIds).not.toHaveBeenCalled();
    expect(projectRestoreFailures.value).toHaveLength(1);
    owner.requestProjectClose(['healthy']);
    expect(projectCloseDialog.value).toMatchObject({ projectId: 'healthy', projectName: 'Healthy' });
    owner.confirmProjectClose();
    expect(closeProjectIds).toHaveBeenCalledTimes(1);
    expect(closeProjectIds).toHaveBeenCalledWith(['healthy']);
  });
});
