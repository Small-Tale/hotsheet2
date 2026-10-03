import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { ProjectTabBar } from './project-tab-bar';

describe('ProjectTabBar actions (HS2-402AXQ, HS2-PNCDAE)', () => {
  it('renders Add project as a native button in a borderless Kerf control group the strip never styles', () => {
    const markup = String(ProjectTabBar({ tabs: [{ id: 'demo', name: 'demo', location: 'local', selected: true }] }));
    expect(markup).toMatch(
      /<div class="kui-toolbar-control-group" data-component="toolbar-control-group" data-appearance="borderless"[^>]*data-single="true"[^>]*data-size="compact"[^>]*data-content="icon"[^>]*><button type="button" data-action="choose-project" aria-label="Add project">/,
    );
    expect(markup).not.toContain('<wa-button');
    const css = readFileSync(new URL('./project-tab-bar.css', import.meta.url), 'utf8');
    expect(css).not.toMatch(/wa-button|wa-dropdown/);
    // The strip owns no control chrome: the groups' buttons are Kerf-styled (HS2-PNCDAE).
    expect(css).not.toMatch(/project-tab-bar__(?:action|actions|modes)\b/);
    expect(css).not.toMatch(/(?:^|[\s,])button\b/m);
  });

  it('renders the phone strip as a Kerf Toolbar of control groups with the same actions', () => {
    const markup = String(
      ProjectTabBar({
        mobile: true,
        mode: 'stats',
        tabs: [
          { id: 'demo', name: 'demo', location: 'local', selected: true },
          { id: 'opening', name: 'opening', location: 'local', pending: true },
        ],
      }),
    );
    expect(markup).toContain('data-component="toolbar"');
    expect(markup).toMatch(
      /kui-toolbar__leading"><div class="kui-toolbar-control-group"[^>]*aria-label="Global dashboards"[^]*data-shell-mode="stats" aria-label="Cross-project stats" aria-pressed="true"[^]*name="mobile-project"[^]*data-action="choose-project" aria-label="Add project"/,
    );
    // Still-opening projects are not choices (HS2-2BEJXD).
    expect(markup).not.toContain('value="opening"');
    // With no choosable project, the Select group is omitted rather than left empty.
    const empty = String(
      ProjectTabBar({ mobile: true, tabs: [{ id: 'opening', name: 'opening', location: 'local', pending: true }] }),
    );
    expect(empty).not.toContain('mobile-project');
    expect(empty).toContain('data-action="choose-project"');
  });
});
