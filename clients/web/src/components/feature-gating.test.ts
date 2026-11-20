import { afterEach, describe, expect, it } from 'vitest';

import {
  allFeaturesOn,
  FEATURE_AREAS,
  featureSettings,
  featureSettingsView,
  resetFeatureSettings,
} from '../feature-areas';
import { FeatureSettings } from './feature-settings';
import { ProjectSidebar } from './project-sidebar';
import { ProjectDashboardModes } from './project-strip-actions';
import { settingsCategoryShown, SettingsNavigation } from './settings-navigation';
import { shownInspectorTab, TicketInspector } from './ticket-inspector';

afterEach(() => {
  resetFeatureSettings();
});

const sidebar = () =>
  String(
    ProjectSidebar({
      completedToday: 1,
      inProgress: 2,
      completionTrend: [0, 1],
      branch: 'main',
      unpushed: 0,
      uncommitted: 1,
      views: [{ id: 'all', label: 'All Tickets', icon: 'all' }],
      selectedViewId: 'all',
      commandGroupLabel: 'Commands',
      commands: [{ id: 'test', label: 'Test', color: '#3b82f6', icon: 'test' }],
      commandGroupExpanded: true,
      driveRunning: false,
      driveTool: 'codex',
      openCount: 7,
      upNextCount: 3,
      activeCount: 2,
    }),
  );
const inspector = () =>
  String(
    TicketInspector({
      slug: 'HS2-TEST',
      title: 'Inspect this ticket',
      status: 'completed',
      priority: 'high',
      category: 'feature',
      tags: [],
      details: 'Readable details.',
      latestConfidence: 80,
      activeTab: 'timeline',
    }),
  );
const quiet = () =>
  featureSettingsView(
    Object.fromEntries(FEATURE_AREAS.filter((area) => area.defaultOn).map((area) => [area.id, false])),
  );

describe('feature-area gating through the shipped components (HS2-3GJZGG)', () => {
  it('hides the experimental entry points by default and shows them once turned on', () => {
    let markup = sidebar();
    expect(markup).not.toContain('data-component="drive-control"');
    expect(markup).not.toContain('data-feature="ai-chat"');
    expect(markup).not.toContain('project-sidebar__drive-row');
    // Project statistics off: the chart stays as a plain, non-clickable summary.
    expect(markup).toContain('data-interactive="false"');
    expect(markup).not.toContain('data-action="open-project-stats"');
    let strip = String(ProjectDashboardModes({ mode: 'project' }));
    expect(strip).toContain('data-feature="workspace-grid"');
    expect(strip).not.toContain('Cross-Project Stats');

    featureSettings.value = featureSettingsView({ drive: true, 'ai-chat': true, 'project-stats': true });
    markup = sidebar();
    expect(markup).toContain('data-component="drive-control"');
    expect(markup).toContain('data-feature="ai-chat"');
    expect(markup).toContain('data-feature="project-stats"');
    expect(markup).not.toContain('data-interactive="false"');
    featureSettings.value = featureSettingsView({ 'cross-project-stats': true });
    strip = String(ProjectDashboardModes({ mode: 'stats' }));
    expect(strip).toContain('aria-label="Cross-Project Stats" aria-pressed="true"');
  });

  it('turns the optional areas off for a quiet workspace and back on', () => {
    featureSettings.value = quiet();
    const markup = sidebar();
    expect(markup).not.toContain('data-component="repository-summary"');
    expect(markup).not.toContain('data-component="command-navigation"');
    expect(markup).not.toContain('aria-label="Add view"');
    expect(markup).toContain('data-component="project-summary"');
    expect(markup).toContain('data-component="view-navigation"');
    expect(String(ProjectDashboardModes({ mode: 'project' }))).not.toContain('data-shell-mode');
    expect(settingsCategoryShown('commands', false)).toBe(false);
    expect(settingsCategoryShown('groups', false)).toBe(false);
    const navigation = String(SettingsNavigation({ selected: 'features' }));
    expect(navigation).not.toContain('data-item-id="commands"');
    expect(navigation).not.toContain('data-item-id="groups"');
    // Timeline off: a restored Timeline tab shows Info, and confidence is hidden.
    expect(shownInspectorTab('timeline')).toBe('info');
    const panel = inspector();
    expect(panel).not.toContain('data-item-id="timeline"');
    expect(panel).not.toContain('data-confidence=');

    featureSettings.value = allFeaturesOn();
    expect(sidebar()).toContain('data-component="command-navigation"');
    expect(sidebar()).toContain('aria-label="Add view"');
    expect(shownInspectorTab('timeline')).toBe('timeline');
    expect(inspector()).toContain('data-confidence="80"');
    expect(String(SettingsNavigation({ selected: 'features' }))).toContain('data-item-id="commands"');
  });

  it('lists Features last in App Settings', () => {
    const navigation = String(SettingsNavigation({ selected: 'features' }));
    const items = [...navigation.matchAll(/data-item-id="([\w-]+)"/g)].map((match) => match[1]);
    expect(items.at(-1)).toBe('features');
  });

  it('renders one switch per area, on from the settings, in the Kerf control inset', () => {
    let page = String(FeatureSettings({}));
    for (const area of FEATURE_AREAS) expect(page).toContain(`data-feature-row="${area.id}"`);
    const checked = (markup: string, id: string) =>
      new RegExp(`value="${id}"[^>]*checked|checked[^>]*value="${id}"`).test(markup);
    expect(checked(page, 'drive')).toBe(false);
    expect(checked(page, 'standby')).toBe(true);
    // Each switch list carries the 8px control inset that lines its track up with the headings' text.
    expect(page).toContain('role="switch"');
    expect(page).not.toContain('kui-checkbox');
    expect(page.match(/data-control-insets="l"/g)?.length ?? 0).toBeGreaterThanOrEqual(5);
    page = String(FeatureSettings({ settings: featureSettingsView({ drive: true, standby: false }) }));
    expect(checked(page, 'drive')).toBe(true);
    expect(checked(page, 'standby')).toBe(false);
    expect(page).toContain('data-action="restore-feature-defaults"');
    expect(String(FeatureSettings({ message: 'settings.json is read-only' }))).toContain('role="alert"');
  });
});
