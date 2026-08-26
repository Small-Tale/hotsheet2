import { describe, expect, it } from 'vitest';
import { syncSettingsControls, type LiveControl } from './settings-controls';

describe('syncSettingsControls', () => {
  it('writes live value and checked properties and tolerates absent controls', () => {
    const controls = new Map<string, LiveControl>([
      ['title', { value: 'stale' } as LiveControl],
      ['selected', { checked: false } as LiveControl],
    ]);
    const root = { querySelector: (selector: string) => controls.get(selector.match(/name="([^"]+)"/)?.[1] ?? '') ?? null } as unknown as ParentNode;
    syncSettingsControls(root, 'row', {
      values: { title: 'reset', missing: 'ignored' },
      checked: { selected: true, absent: false },
    });
    expect(controls.get('title')?.value).toBe('reset');
    expect(controls.get('selected')?.checked).toBe(true);
  });
});
