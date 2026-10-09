import { expect, it } from 'vitest';

import { workspaceSearchFit, workspaceSlotWidthChanged, workspaceToolbarHidden } from './workspace-toolbar-visibility';

it('uses strict lower and inclusive upper width boundaries', () => {
  expect(workspaceToolbarHidden(223, 224)).toBe(true);
  expect(workspaceToolbarHidden(224, 224)).toBe(false);
  expect(workspaceToolbarHidden(479, undefined, 480)).toBe(false);
  expect(workspaceToolbarHidden(480, undefined, 480)).toBe(true);
  expect(workspaceToolbarHidden(480)).toBe(false);
});

it('uses the rendered title width and retains controls until the search needs their space', () => {
  const groups = [112, 96, 48];
  expect(workspaceSearchFit(750, 120, groups, 44, 8)).toEqual([true, true, true, true]);
  expect(workspaceSearchFit(750, 320, groups, 44, 8)).toEqual([true, true, false, false]);
  expect(workspaceSearchFit(380, 120, groups, 44, 8)).toEqual([false, false, false, false]);
});

it('accepts CSSOM-rounded widths and still repairs a cleared search slot', () => {
  expect(workspaceSlotWidthChanged('329.078px', 329.078125)).toBe(false);
  expect(workspaceSlotWidthChanged('329.078px', 329.25)).toBe(true);
  expect(workspaceSlotWidthChanged('', 329.078125)).toBe(true);
});
