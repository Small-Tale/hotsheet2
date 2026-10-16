import { expect, it } from 'vitest';

import {
  applyWorkspaceSearchSizing,
  retargetIdentityTextObserver,
  workspaceSearchFit,
  workspaceToolbarHidden,
} from './workspace-toolbar-visibility';

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

it('writes search sizing to its morph-stable rule without redundant style writes', () => {
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

it('follows the title through replacement, repeat, removal, and return to remeasure on text changes (HS2-2RD2M9)', () => {
  const calls: string[] = [];
  const observer = {
    disconnect() {
      calls.push('disconnect');
    },
    observe(target: string, options: MutationObserverInit) {
      calls.push(`observe ${target} ${JSON.stringify(options)}`);
    },
  };
  const options = JSON.stringify({ characterData: true, childList: true, subtree: true });
  let current = retargetIdentityTextObserver(observer, null, 'queue-title');
  expect(current).toBe('queue-title');
  expect(calls).toEqual(['disconnect', `observe queue-title ${options}`]);
  // The same title element (only its text changed) keeps the existing observation.
  calls.length = 0;
  current = retargetIdentityTextObserver(observer, current, 'queue-title');
  expect(calls).toEqual([]);
  // A replaced title moves the observation; a header without a title observes nothing.
  current = retargetIdentityTextObserver(observer, current, 'all-tickets-title');
  current = retargetIdentityTextObserver(observer, current, null);
  expect(current).toBeNull();
  current = retargetIdentityTextObserver(observer, current, null);
  expect(retargetIdentityTextObserver(observer, current, 'queue-title')).toBe('queue-title');
  expect(calls).toEqual([
    'disconnect',
    `observe all-tickets-title ${options}`,
    'disconnect',
    'disconnect',
    `observe queue-title ${options}`,
  ]);
});
