import { expect, it } from 'vitest';

import { applyWorkspaceSearchSizing, workspaceSearchFit, workspaceToolbarHidden } from './workspace-toolbar-visibility';

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
  // The rail passes only second-row groups; it can yield utility, then sort, without
  // creating a phantom fourth visibility state or hiding its full-width view tabs.
  expect(workspaceSearchFit(500, 0, [88, 96], 44, 8)).toEqual([true, true, true]);
  expect(workspaceSearchFit(410, 0, [88, 96], 44, 8)).toEqual([true, true, false]);
  expect(workspaceSearchFit(370, 0, [88, 96], 44, 8)).toEqual([true, false, false]);
});

it('keeps search sizing on the stable root without redundant style writes', () => {
  const properties = new Map<string, string>();
  let writes = 0;
  const style = {
    getPropertyValue(name: string) {
      return properties.get(name) ?? '';
    },
    setProperty(name: string, value: string) {
      writes += 1;
      properties.set(name, value);
    },
  };
  const sizing = { width: 329.078125, expandedWidth: '329.078125px' };
  expect(applyWorkspaceSearchSizing(style, sizing)).toBe(true);
  expect(properties.get('--hs-workspace-search-expanded-width')).toBe('329.078125px');
  expect(properties.get('--hs-workspace-search-slot-width')).toBe('329.078125px');
  expect(writes).toBe(2);
  expect(applyWorkspaceSearchSizing(style, sizing)).toBe(false);
  expect(writes).toBe(2);
  expect(applyWorkspaceSearchSizing(style, { ...sizing, width: 320 })).toBe(true);
  expect(properties.get('--hs-workspace-search-slot-width')).toBe('320px');
  expect(writes).toBe(3);
});
