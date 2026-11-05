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
    expect(markup).toContain('aria-label="Token rate: 1.6k tokens/min (30-day peak 6.2k)"');
    expect(markup).toContain('aria-label="Tickets updated: 14 tickets/h (30-day peak 22)"');
    for (const kind of ['waiting', 'overrun', 'runway']) expect(markup).toContain(`data-live-chip="${kind}"`);
    expect(markup).not.toContain('data-live-chip="state"');
    // The explanations are titles and accessible names only.
    expect(markup).toContain('title="Needs you · 2 tickets waiting on you"');
    expect(markup).toContain('title="HS2-R76MMW past ETA by 18m"');
    expect(markup).not.toContain('>Needs you<');
    expect(markup).not.toContain('data-live-row');
  });

  it('labels the header action for the mode it switches to', () => {
    const compact = String(ProjectLiveSection({ summary: quiet }));
    expect(compact).toContain('data-action="toggle-live-section"');
    expect(compact).toContain('Show live details');
    expect(compact).toContain('chevrons-up-down');
    const expanded = String(ProjectLiveSection({ summary: quiet, expanded: true }));
    expect(expanded).toContain('Show compact live summary');
    expect(expanded).toContain('chevrons-down-up');
  });

  it('shows only the rows that have something to say when expanded', () => {
    const full = String(ProjectLiveSection({ summary: attention, expanded: true }));
    expect(full).toContain('data-mode="expanded"');
    expect(count(full, /data-variant="full"/)).toBe(2);
    for (const row of ['state', 'waiting', 'overrun', 'runway']) expect(full).toContain(`data-live-row="${row}"`);
    expect(full).toContain('HS2-R76MMW past ETA');
    expect(full).toContain('Queue runway ~3h 40m');
    expect(full).toContain('4 ready');
    expect(full).toContain('2m ago');
    const calm = String(
      ProjectLiveSection({ summary: projectLiveSummary([demoLiveState('stopped')], DEMO_LIVE_NOW_MS), expanded: true }),
    );
    expect(calm).toContain('data-live-row="state"');
    for (const row of ['waiting', 'overrun', 'runway']) expect(calm).not.toContain(`data-live-row="${row}"`);
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
