import { describe, expect, it } from 'vitest';

import { loadLucideCatalog } from '../lucide-catalog';
import { commandIconName, commandIconNeedsCatalog, resolveCommandIcon } from './command-icon';

describe('commandIconName', () => {
  it('defaults to send and maps legacy aliases to their Lucide ids', () => {
    expect(commandIconName()).toBe('send');
    expect(commandIconName('')).toBe('send');
    expect(commandIconName('test')).toBe('test-tube-2');
    expect(commandIconName('build')).toBe('hammer');
    expect(commandIconName('git-branch')).toBe('git-branch');
  });
});

describe('commandIconNeedsCatalog', () => {
  it('is false for empty, default, and bundled popular icons, true for other names', () => {
    expect(commandIconNeedsCatalog()).toBe(false);
    expect(commandIconNeedsCatalog('send')).toBe(false);
    expect(commandIconNeedsCatalog('test')).toBe(false); // resolves to bundled test-tube-2
    expect(commandIconNeedsCatalog('git-branch')).toBe(false); // bundled popular
    expect(commandIconNeedsCatalog('activity')).toBe(true); // not bundled
  });
});

describe('resolveCommandIcon', () => {
  it('resolves bundled popular icons synchronously', () => {
    const send = resolveCommandIcon('send');
    expect(send.name).toBe('send');
    expect(Array.isArray(send.icon)).toBe(true);
    // Legacy alias resolves to its bundled Lucide id and node.
    expect(resolveCommandIcon('test').name).toBe('test-tube-2');
  });

  it('falls back to the default node for an unloaded custom icon, then resolves it once loaded', async () => {
    const before = resolveCommandIcon('activity');
    expect(before.name).toBe('activity');
    // Until the catalog loads, it renders the fallback node.
    expect(before.icon).toEqual(resolveCommandIcon('send').icon);
    await loadLucideCatalog();
    const after = resolveCommandIcon('activity');
    expect(after.name).toBe('activity');
    expect(Array.isArray(after.icon)).toBe(true);
    expect(after.icon).not.toEqual(resolveCommandIcon('send').icon);
  });
});
