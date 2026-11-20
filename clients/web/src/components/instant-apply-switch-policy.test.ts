import { readdirSync, readFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { hostHealthDisksFixture } from '../ux-demo/host-health-fixtures';
import { FeatureSettings } from './feature-settings';
import { HostDiskSettings } from './host-disk-settings';
import { STANDBY_SETTING_FIELDS, StandbySettings } from './standby-settings';

/**
 * HS2-1MSTXP policy (maintainer decision: "Switch everywhere"; Kerf `checkbox.md` "Not when": a
 * setting that applies the moment it changes uses Switch). A settings toggle that saves on change
 * renders Kerf `Switch`; `Checkbox` stays for an option a form submits later (Save, Apply Limits,
 * Apply Selected Guidance, a dialog's submit button).
 */
const SRC = resolve(__dirname, '..');

/** Every shipped `.tsx` module; demos and tests mirror production and are not policed here. */
function sources(dir: string = SRC): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return entry.name === 'ux-demo' ? [] : sources(path);
    return entry.name.endsWith('.tsx') && !entry.name.includes('.test.') ? [path] : [];
  });
}

/** The JSX tag name of the element whose attributes contain `index`. */
function owningTag(source: string, index: number): string | undefined {
  const open = source.lastIndexOf('<', index);
  return /^<([A-Za-z][\w.]*)/u.exec(source.slice(open))?.[1];
}

/**
 * Checkbox `action`s that only stage a choice; a separate control applies it. Each entry names
 * where the staged choice is applied.
 */
const STAGED_CHECKBOX_ACTIONS: Record<string, string> = {
  // Selection is saved, but rules install only through Apply Selected Guidance.
  'COMMANDS_AND_AI_ACTIONS.toggleGuidanceRule': 'Apply Selected Guidance',
  'COMMANDS_AND_AI_ACTIONS.toggleGuidanceSkill': 'Apply Selected Guidance',
};

/** The instant-apply settings toggles, by source file and the expression that wires them. */
const INSTANT_APPLY_TOGGLES: readonly (readonly [file: string, anchor: string])[] = [
  ['components/settings-workspace.tsx', 'COMMANDS_AND_AI_ACTIONS.toggleLoadingActivity'],
  ['components/settings-workspace.tsx', 'COMMANDS_AND_AI_ACTIONS.toggleGlobalShellHistory'],
  ['components/settings-workspace.tsx', 'NOTIFICATIONS_AND_LINKS_ACTIONS.toggleAutoGroupNewTickets'],
  ['components/settings-workspace.tsx', 'NOTIFICATIONS_AND_LINKS_ACTIONS.toggleVerifiedColumn'],
  ['components/standby-settings.tsx', 'STANDBY_SETTING_FIELDS.enabled'],
  ['components/host-disk-settings.tsx', 'SHELL_AND_GLOBAL_ACTIONS.toggleHostDisk'],
  ['components/feature-settings.tsx', 'SHELL_AND_GLOBAL_ACTIONS.setFeature'],
];

describe('HS2-1MSTXP instant-apply settings toggles use Kerf Switch', () => {
  it('renders each listed instant-apply settings toggle as a Switch', () => {
    for (const [file, anchor] of INSTANT_APPLY_TOGGLES) {
      const source = readFileSync(join(SRC, file), 'utf8'),
        index = source.indexOf(anchor);
      expect(index, `${anchor} is wired in ${file}`).toBeGreaterThan(-1);
      expect(owningTag(source, index), `${anchor} in ${file}`).toBe('Switch');
    }
  });

  it('gives a Checkbox a change action only when that action stages a choice applied elsewhere', () => {
    const offenders: string[] = [];
    for (const path of sources()) {
      const source = readFileSync(path, 'utf8');
      for (const match of source.matchAll(/<Checkbox\b[^>]*?\baction=\{([\w.]+?)(?:\.value)?\}/gsu)) {
        const [, action] = match;
        if (!(action in STAGED_CHECKBOX_ACTIONS)) {
          const line = source.slice(0, match.index).split('\n').length;
          offenders.push(`${relative(SRC, path)}:${line} ${action}`);
        }
      }
    }
    // A Checkbox whose change saves at once is an instant-apply setting: render Switch instead,
    // or list the action above with the control that applies it.
    expect(offenders).toEqual([]);
  });

  it('keeps the staged-checkbox allowance current', () => {
    const all = sources()
      .map((path) => readFileSync(path, 'utf8'))
      .join('\n');
    for (const action of Object.keys(STAGED_CHECKBOX_ACTIONS))
      expect(all, `${action} is still a Checkbox action`).toMatch(
        new RegExp(`<Checkbox\\b[^>]*?\\baction=\\{${action.replace('.', '\\.')}\\.value\\}`, 'su'),
      );
  });

  it('exposes the General and Features toggles to assistive technology as switches', () => {
    const standby = String(StandbySettings({})),
      disks = String(HostDiskSettings({ disks: hostHealthDisksFixture(), hostName: 'studio' })),
      features = String(FeatureSettings({}));
    expect(standby).toContain(`type="checkbox" role="switch" name="${STANDBY_SETTING_FIELDS.enabled}"`);
    expect(standby).not.toContain('data-component="checkbox"');
    expect(disks.match(/role="switch"/gu)?.length).toBe(hostHealthDisksFixture().length);
    expect(disks).not.toContain('data-component="checkbox"');
    expect(features).toContain('role="switch"');
    expect(features).not.toContain('data-component="checkbox"');
  });
});
