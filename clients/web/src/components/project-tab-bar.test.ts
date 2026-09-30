import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { ProjectTabBar } from './project-tab-bar';

describe('ProjectTabBar actions (HS2-402AXQ)', () => {
  it('renders the Add project action as an app-styled native button beside the Kerf TabBar', () => {
    const markup = String(ProjectTabBar({ tabs: [{ id: 'demo', name: 'demo', location: 'local', selected: true }] }));
    expect(markup).toMatch(
      /<button type="button" class="project-tab-bar__action" data-action="choose-project" aria-label="Add project" title="Add project">/,
    );
    expect(markup).not.toContain('<wa-button');
    const css = readFileSync(new URL('./project-tab-bar.css', import.meta.url), 'utf8');
    expect(css).not.toMatch(/wa-button|wa-dropdown/);
    expect(css).toMatch(
      /\.project-tab-bar__action \{[^}]*width: remify\(32px\)[^}]*border-radius: var\(--wa-border-radius-pill\)/,
    );
  });
});
