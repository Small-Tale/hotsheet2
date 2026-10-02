import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

describe('Drive options lifecycle (HS2-S010QF)', () => {
  const source = ['./app/runtime.tsx', './interactions/commands-and-ai.ts', './interactions/shell-and-global.ts']
    .map((file) => readFileSync(new URL(file, import.meta.url), 'utf8'))
    .join('\n');

  it('keeps the owning menu open across provider, model, effort, default, and manual-model selections', () => {
    for (const action of ['selectDriveDefault', 'selectDriveTool', 'selectDriveModel', 'selectDriveEffort']) {
      const start = source.indexOf(`COMMANDS_AND_AI_ACTIONS.${action}.selector`),
        end = source.indexOf('\n', start);
      expect(start, action).toBeGreaterThan(0);
      expect(source.slice(start, end)).not.toContain('driveOptionsOpen.value=false');
    }
    const manualStart = source.indexOf('function openManualModel(target:'),
      manualEnd = source.indexOf('function restoreCommandEditorAfterManualModel', manualStart);
    expect(source.slice(manualStart, manualEnd)).not.toContain('driveOptionsOpen.value=false');
  });

  it('still closes from its toggle and a true outside pointer interaction', () => {
    expect(source).toContainSource('driveOptionsOpen.value=opening');
    expect(source).toContainSource(
      "event.composedPath().some(item=>item instanceof Element&&item.matches('.project-sidebar__drive-row'))",
    );
  });
});
