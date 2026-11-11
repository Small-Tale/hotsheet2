import { describe, expect, it } from 'vitest';

import { hostCallouts, projectLiveSummary } from '../project-live-summary';
import { DEMO_LIVE_NOW_MS, demoHostHealth, demoLiveState } from '../ux-demo/live-section-fixtures';
import { ProjectLiveSection } from './project-live-section';
import { projectSidebarPanel } from './project-sidebar';

const attention = projectLiveSummary([demoLiveState('attention')], DEMO_LIVE_NOW_MS);
const quiet = projectLiveSummary([demoLiveState('quiet')], DEMO_LIVE_NOW_MS);

function count(markup: string, pattern: RegExp): number {
  return markup.match(new RegExp(pattern.source, 'g'))?.length ?? 0;
}

describe('ProjectLiveSection (HS2-F788GW)', () => {
  it('renders compact by default: two bare chip gauges and glyph chips, no sentences on the face', () => {
    const markup = String(ProjectLiveSection({ summary: attention }));
    expect(markup).toContain('data-component="project-live-section"');
    expect(markup).toContain('data-mode="compact"');
    expect(count(markup, /data-component="glance-gauge"/)).toBe(2);
    expect(count(markup, /data-variant="bare"/)).toBe(2);
    expect(count(markup, /data-size="chip"/)).toBe(2);
    expect(markup).toContain('data-tone="brand"');
    expect(markup).toContain('data-tone="success"');
    expect(markup).toContain('aria-label="Token rate: 1.6k tokens/min (24-hour peak 6.2k)"');
    expect(markup).toContain('aria-label="Tickets updated: 14 tickets/h (24-hour peak 22)"');
    for (const kind of ['waiting', 'overrun']) expect(markup).toContain(`data-live-chip="${kind}"`);
    expect(markup).not.toContain('data-live-chip="state"');
    // Queue runway is gone (HS2-5A83MR).
    expect(markup).not.toContain('data-live-chip="runway"');
    // The explanations are titles and accessible names only.
    expect(markup).toContain('title="Needs you · 2 tickets waiting on you"');
    expect(markup).toContain('title="HS2-R76MMW past ETA by 18m"');
    expect(markup).not.toContain('>Needs you<');
    expect(markup).not.toContain('data-live-row');
  });

  it('has no header bar: the content itself is the toggle, named for the mode it switches to (HS2-9J6PPR)', () => {
    const compact = String(ProjectLiveSection({ summary: quiet }));
    expect(compact).not.toContain('data-component="list-header"');
    expect(compact).not.toContain('chevrons-up-down');
    // One native button wraps the gauges and chips and carries the delegated toggle.
    expect(count(compact, /<button/)).toBe(1);
    expect(compact).toMatch(
      /<button type="button" class="project-live-section__toggle" data-action="toggle-live-section" aria-expanded="false" aria-label="Show live details\. Active · 2 workers live, last progress 2m ago">/,
    );
    const gaugesAt = compact.indexOf('data-component="glance-gauge"');
    expect(gaugesAt).toBeGreaterThan(compact.indexOf('<button'));
    expect(gaugesAt).toBeLessThan(compact.indexOf('</button>'));
    const expanded = String(ProjectLiveSection({ summary: attention, expanded: true }));
    expect(expanded).toContain('aria-expanded="true"');
    expect(expanded).toContain('aria-label="Show compact live summary. Active · 2 workers live, last progress 2m ago"');
    expect(expanded).not.toContain('chevrons-down-up');
    expect(expanded.indexOf('data-live-row="overrun"')).toBeLessThan(expanded.indexOf('</button>'));
    // Host callouts are not toggles: they sit outside the button.
    const callouts = hostCallouts(demoHostHealth('disk-critical'));
    const withCallout = String(ProjectLiveSection({ summary: quiet, callouts }));
    expect(withCallout.indexOf('data-host-callout')).toBeGreaterThan(withCallout.indexOf('</button>'));
    // Before any store reports there is nothing to toggle.
    expect(String(ProjectLiveSection({}))).not.toContain('<button');
  });

  it('centers the gauges horizontally in expanded mode only, through the Row prop (HS2-80SWPT)', () => {
    const expanded = String(ProjectLiveSection({ summary: quiet, expanded: true }));
    expect(expanded).toMatch(/<div data-live-gauges=""[^>]*data-h-align="center"/);
    const compact = String(ProjectLiveSection({ summary: quiet }));
    expect(compact).not.toContain('data-live-gauges');
    expect(compact).not.toContain('data-h-align="center"');
  });

  it('shows only the rows that have something to say when expanded', () => {
    const full = String(ProjectLiveSection({ summary: attention, expanded: true }));
    expect(full).toContain('data-mode="expanded"');
    expect(count(full, /data-variant="full"/)).toBe(2);
    for (const row of ['state', 'waiting', 'overrun']) expect(full).toContain(`data-live-row="${row}"`);
    expect(full).toContain('HS2-R76MMW past ETA');
    expect(full).not.toContain('data-live-row="runway"');
    expect(full).not.toContain('Queue runway');
    expect(full).toContain('2m ago');
    const calm = String(
      ProjectLiveSection({ summary: projectLiveSummary([demoLiveState('stopped')], DEMO_LIVE_NOW_MS), expanded: true }),
    );
    expect(calm).toContain('data-live-row="state"');
    for (const row of ['waiting', 'overrun']) expect(calm).not.toContain(`data-live-row="${row}"`);
  });

  it('keeps a full-width host callout in both modes, toned by level', () => {
    const callouts = hostCallouts(demoHostHealth('disk-critical'));
    for (const expanded of [false, true]) {
      const markup = String(ProjectLiveSection({ summary: quiet, callouts, expanded }));
      expect(markup).toContain('data-host-callout="disk"');
      expect(markup).toContain('Disk 4 GB free · full in ~2h');
      expect(markup).toContain('data-tone="danger"');
    }
    const warning = String(ProjectLiveSection({ callouts: hostCallouts(demoHostHealth('disk-warning')) }));
    expect(warning).toContain('data-tone="warning"');
    expect(warning).not.toContain('data-component="glance-gauge"');
  });

  it('sits under the completion chart in the real project sidebar, and is absent without data', () => {
    const base = {
      completedToday: 6,
      inProgress: 3,
      completionTrend: [1, 2, 3],
      branch: 'main',
      unpushed: 0,
      uncommitted: 0,
      views: [],
      selectedViewId: 'all',
      commandGroupLabel: 'Project commands',
      commands: [],
      commandGroupExpanded: true,
      driveRunning: false,
      driveTool: 'codex',
      openCount: 1,
      upNextCount: 0,
      activeCount: 0,
    };
    const withLive = String(projectSidebarPanel({ ...base, live: { summary: quiet } }).content);
    const summaryAt = withLive.indexOf('data-component="project-summary"'),
      liveAt = withLive.indexOf('data-component="project-live-section"'),
      repositoryAt = withLive.indexOf('data-component="repository-summary"');
    expect(summaryAt).toBeGreaterThan(-1);
    expect(liveAt).toBeGreaterThan(summaryAt);
    expect(repositoryAt).toBeGreaterThan(liveAt);
    expect(String(projectSidebarPanel(base).content)).not.toContain('project-live-section');
  });
});
