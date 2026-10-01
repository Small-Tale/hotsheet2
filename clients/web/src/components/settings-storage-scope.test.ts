import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

import { projectScopedServerPath } from '../project-bridge';
import {
  isAppSettingsCategory,
  SETTINGS_STORAGE,
  type SettingsCategory,
  type SettingsStorage,
} from './settings-navigation';

const PROJECT = 'checkout-under-test';
const source = (() => {
  const root = join(__dirname, '..'),
    files: string[] = [];
  const walk = (directory: string) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (/\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name)) files.push(readFileSync(path, 'utf8'));
    }
  };
  walk(root);
  return files.join('\n');
})();

/** Whether a declared storage location holds a per-project value once the client requests it. */
function storedPerProject(storage: SettingsStorage): boolean {
  if (storage.server !== undefined)
    return projectScopedServerPath(PROJECT, storage.server.replace('{project}', PROJECT)).startsWith(
      `/checkouts/${PROJECT}/`,
    );
  return storage.browser.includes('.{project}.');
}

/** The literal part of a declared location the client source must contain. */
function sourceMarker(storage: SettingsStorage): string {
  const location = storage.server ?? storage.browser;
  return location.includes('{project}') ? location.slice(location.indexOf('{project}') + '{project}'.length) : location;
}

describe('settings category grouping matches storage scope (HS2-S1184P)', () => {
  const entries = Object.entries(SETTINGS_STORAGE) as [SettingsCategory, readonly SettingsStorage[]][];

  it.each(entries)('files %s under the group that matches where it stores values', (category, storages) => {
    expect(storages.length).toBeGreaterThan(0);
    for (const storage of storages)
      expect({ control: storage.control, perProject: storedPerProject(storage) }).toEqual({
        control: storage.control,
        perProject: !isAppSettingsCategory(category),
      });
  });

  it('declares only storage locations the client actually uses', () => {
    for (const [, storages] of entries)
      for (const storage of storages) expect(source, storage.control).toContain(sourceMarker(storage));
  });

  it('detects a misfiled setting', () => {
    // A machine-wide connection catalog would be a mismatch under Project Settings.
    expect(storedPerProject({ control: 'catalog', server: '/provider-connections' })).toBe(false);
    expect(isAppSettingsCategory('sources')).toBe(false);
    // A project-keyed browser value would be a mismatch under App Settings.
    expect(storedPerProject({ control: 'columns', browser: 'hotsheet.project.{project}.hide-verified-column' })).toBe(
      true,
    );
    expect(isAppSettingsCategory('general')).toBe(true);
  });
});
