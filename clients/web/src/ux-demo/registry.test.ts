import { describe, expect, it, vi } from 'vitest';

import { flattenCatalog } from './catalog';
import { demoEntry, demoModules, registeredDemoIds, wireDemoModules } from './registry';

// This registry test isolates demo module lifetimes; the Combobox's DOM wiring has its own unit tests.
vi.mock('../components/app-combobox-wiring', () => ({ wireAppComboboxes: () => () => undefined }));

describe('UX demo registry (HS2-HMVBZW)', () => {
  it('registers a demo for every catalog entry and nothing outside the catalog', () => {
    const catalog = flattenCatalog();
    const ids = new Set(catalog.map((item) => item.id));
    expect(registeredDemoIds().filter((id) => !ids.has(id))).toEqual([]);
    expect(catalog.filter((item) => !demoEntry(item.id)).map((item) => item.id)).toEqual([]);
  });

  it('offers Settings with a Reset for every entry that has adjustable settings', () => {
    for (const id of registeredDemoIds()) {
      const entry = demoEntry(id)!;
      if (!entry.Settings) continue;
      expect(entry.reset, id).toBeTypeOf('function');
      expect(String(entry.Settings()), id).toContain('data-action="reset-settings"');
    }
    expect(registeredDemoIds().filter((id) => demoEntry(id)?.Settings)).toContain('tag-chip');
  });

  it('wires every module once and disposes all of their listeners together', () => {
    const disposed: string[] = [];
    const root = {} as HTMLElement;
    vi.stubGlobal('document', { body: root });
    const wired = demoModules.filter((module) => module.wire);
    const original = wired.map((module) => module.wire!);
    try {
      wired.forEach((module, index) => {
        Object.assign(module, {
          wire: () => () => {
            disposed.push(String(index));
          },
        });
      });
      const dispose = wireDemoModules(root);
      expect(disposed).toEqual([]);
      dispose();
      expect(disposed).toHaveLength(wired.length);
    } finally {
      wired.forEach((module, index) => {
        Object.assign(module, { wire: original[index] });
      });
      vi.unstubAllGlobals();
    }
  });
});
