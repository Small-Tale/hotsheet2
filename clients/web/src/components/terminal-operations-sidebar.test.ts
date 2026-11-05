import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import type { LiveWord } from '../api';
import { type GridSidebarLive, LIVE_WORD_LABELS } from '../grid-sidebar-live';
import { LIVE_STATE_ICONS } from '../live-state-icons';
import {
  aggregateTerminalProjectSummaries,
  GRID_SIDEBAR_PAGES,
  gridSidebarPageContent,
  TerminalOperationsSidebar,
} from './terminal-operations-sidebar';

const MINUTE = 60_000;
function reading(state: LiveWord, overrides: Partial<GridSidebarLive> = {}): GridSidebarLive {
  return {
    state,
    tokensPerMinute: 1200,
    tokensPeakPerMinute: 2400,
    ticketsUpdatedPerHour: 6,
    ticketsPeakPerHour: 12,
    waiting: 0,
    workersLive: 1,
    workersExpected: 1,
    ...overrides,
  };
}

describe('TerminalOperationsSidebar', () => {
  const projects = [
    { id: 'one', name: 'One', completedToday: 2, inProgress: 3, trend: [1, 0, 2] },
    { id: 'two', name: 'Two', completedToday: 4, inProgress: 1, trend: [0, 3, 4] },
  ];

  it('sums project counts and aligned trend days', () => {
    expect(aggregateTerminalProjectSummaries(projects)).toEqual({
      id: 'all',
      name: 'All projects',
      completedToday: 6,
      inProgress: 4,
      trend: [1, 3, 6],
    });
  });
  it('marks the aggregate incomplete when one project has a partial source', () => {
    expect(aggregateTerminalProjectSummaries([{ ...projects[0], partial: true }, projects[1]]).partial).toBe(true);
  });

  it('adds an aggregate group only when multiple projects are open', () => {
    const multiple = String(TerminalOperationsSidebar({ projects })),
      single = String(TerminalOperationsSidebar({ projects: [projects[0]] }));
    expect(multiple).toContain('class="kui-pane terminal-operations-sidebar"');
    expect(multiple).toContain('data-component="pane"');
    expect(multiple).toContain(
      '<section class="kui-pane__content kui-content" aria-label="Terminal operations sidebar">',
    );
    expect(multiple).toContain('data-control-insets="trbl"');
    expect(multiple).toContain('All projects');
    expect(multiple).toContain('data-chart-tone="success"');
    expect(multiple.match(/data-chart-tone="brand"/g)).toHaveLength(2);
    expect(multiple.match(/data-component="project-summary"/g)).toHaveLength(3);
    expect(multiple.match(/data-chart-maximum="6"/g)).toHaveLength(3);
    expect(multiple.match(/data-chart-background="true"/g)).toHaveLength(2);
    expect(multiple.match(/data-chart-background="false"/g)).toHaveLength(1);
    expect(multiple.match(/data-background-bar=/g)).toHaveLength(6);
    expect(multiple).toContain('All projects: 1, 3, 6');
    expect(multiple).toContain('data-project-id="one"');
    expect(multiple).toContain('--bar-height:33%');
    expect(single).not.toContain('All projects');
    expect(single.match(/data-component="project-summary"/g)).toHaveLength(1);
    expect(single).toContain('data-chart-maximum="2"');
    expect(single).toContain('data-chart-background="false"');
    expect(single).not.toContain('data-background-bar=');
  });

  it('separates only the multi-project aggregate through the List divider API', () => {
    const multiple = String(TerminalOperationsSidebar({ projects }));
    expect(multiple).toContain('data-project-id="all"');
    expect(multiple.match(/divider-sides="b"/g)).toHaveLength(1);
    expect(String(TerminalOperationsSidebar({ projects: [projects[0]] }))).not.toContain('divider-sides="b"');
  });

  it('aligns the group ListHeader label with the ProjectSummary content on one gutter (HS2-RSJ796)', () => {
    const markup = String(TerminalOperationsSidebar({ projects }));
    expect(markup).not.toContain('data-control-insets="rl"');
    expect(markup).toContain('data-inline="true"');
    // ListHeader owns its text inset; no wrapper doubles that gutter.
    expect(markup.match(/class="project-summary" data-component="project-summary" data-size="compact"/g)).toHaveLength(
      projects.length + 1,
    );
    const summaryCss = readFileSync(new URL('./project-summary.css', import.meta.url), 'utf8');
    expect(summaryCss).toMatch(/\.project-summary\[data-size='compact'\] \{[^}]*min-height: remify\(68px\);/);
    // The compact size sets the chart height through the summary's private property (HS2-H0THQW).
    expect(summaryCss).toMatch(
      /\.project-summary\[data-size='compact'\] \{[^}]*--_project-summary-chart-height: remify\(44px\);/,
    );
  });

  describe('pages share one group structure (HS2-E1AQE8)', () => {
    const live = [
      { id: 'one', name: 'One', completedToday: 2, inProgress: 3, trend: [1, 0, 2], live: reading('stopped') },
      {
        id: 'two',
        name: 'Two',
        completedToday: 4,
        inProgress: 1,
        trend: [0, 3, 4],
        live: reading('may_be_stuck', { progressAgeMs: 34 * MINUTE, claim: { slug: 'TWO-1', phase: 'working' } }),
      },
    ];
    const allLive = reading('may_be_stuck', { waiting: 2, workersLive: 3, workersExpected: 4 });
    const groupsOf = (markup: string) =>
      [...markup.matchAll(/data-project-id="([^"]+)" data-grid-sidebar-group=""/g)].map((match) => match[1]);

    it('renders the same header, divider, and group per project on every page', () => {
      for (const page of GRID_SIDEBAR_PAGES) {
        const markup = String(gridSidebarPageContent(page, { projects: live, allLive }));
        expect(markup, page).toContain(`data-grid-sidebar-page="${page}"`);
        expect(markup.match(/class="kui-list grid-sidebar-group"/g), page).toHaveLength(3);
        expect(markup.match(/divider-sides="b"/g), page).toHaveLength(1);
        expect(markup.match(/data-component="list-header"/g), page).toHaveLength(3);
        expect(groupsOf(markup)[0], page).toBe('all');
        expect(new Set(groupsOf(markup)), page).toEqual(new Set(['all', 'one', 'two']));
      }
      // Completion and gauges keep the open-project order; triage puts the most need first.
      expect(groupsOf(String(gridSidebarPageContent('completion', { projects: live })))).toEqual(['all', 'one', 'two']);
      expect(groupsOf(String(gridSidebarPageContent('gauges', { projects: live })))).toEqual(['all', 'one', 'two']);
      expect(groupsOf(String(gridSidebarPageContent('triage', { projects: live })))).toEqual(['all', 'two', 'one']);
    });

    it('keeps production on the Completion page by default', () => {
      const markup = String(TerminalOperationsSidebar({ projects: live, allLive }));
      expect(markup).toContain('data-grid-sidebar-page="completion"');
      expect(markup).not.toContain('data-component="grid-sidebar-gauges"');
    });

    it('gives each gauge group two bare gauges and its state glyph, the aggregate none', () => {
      const markup = String(gridSidebarPageContent('gauges', { projects: live, allLive }));
      expect(markup.match(/data-component="grid-sidebar-gauges"/g)).toHaveLength(3);
      expect(markup.match(/data-variant="bare" data-tone="brand"/g)).toHaveLength(3);
      expect(markup.match(/data-variant="bare" data-tone="success"/g)).toHaveLength(3);
      expect(markup.match(/data-live-word="/g)).toHaveLength(2);
      expect(markup).toContain('data-live-word="may_be_stuck"');
      expect(markup).toContain('Two: May be stuck, no progress for 34 minutes');
      expect(markup).toContain(
        'aria-label="Open project statistics, 1.2k tokens/min, 6 tickets updated in the last hour, May be stuck"',
      );
      expect(markup).toContain('data-action="open-project-stats"');
    });

    it('rests the gauges at zero with no glyph before live state arrives', () => {
      const markup = String(
        gridSidebarPageContent('gauges', {
          projects: live.map(({ id, name, completedToday, inProgress, trend }) => ({
            id,
            name,
            completedToday,
            inProgress,
            trend,
          })),
        }),
      );
      expect(markup).toContain('data-live-state="none"');
      expect(markup).not.toContain('data-live-word=');
      expect(markup.match(/data-empty="true"/g)).toHaveLength(6);
    });

    it.each(Object.keys(LIVE_STATE_ICONS) as LiveWord[])(
      'shows the %s glyph and title on gauges and triage',
      (word) => {
        const projects = [{ id: 'p', name: 'P', completedToday: 0, inProgress: 0, trend: [], live: reading(word) }];
        for (const page of ['gauges', 'triage'] as const) {
          const markup = String(gridSidebarPageContent(page, { projects }));
          expect(markup, page).toContain(`data-lucide="${LIVE_STATE_ICONS[word].name}"`);
          expect(markup, page).toContain(`data-live-word="${word}"`);
          expect(markup, page).toContain(`P: ${LIVE_WORD_LABELS[word]}`);
        }
      },
    );

    it('lists triage rows with the state word and work, durations only in the title', () => {
      const markup = String(gridSidebarPageContent('triage', { projects: live, allLive }));
      expect(markup).toContain('May be stuck');
      expect(markup).toContain('TWO-1 · working');
      expect(markup).toContain('title="Two: May be stuck, no progress for 34 minutes');
      expect(markup).not.toMatch(/>\s*34 ?m(inutes)?\s*</);
      expect(markup).toContain('data-triage-summary="waiting"');
      expect(markup).toContain('3 of 4');
      const calm = String(gridSidebarPageContent('triage', { projects: live, allLive: reading('active') }));
      expect(calm).not.toContain('data-triage-summary="waiting"');
      const unknown = String(gridSidebarPageContent('triage', { projects: [{ ...live[0], live: undefined }] }));
      expect(unknown).toContain('No live state');
      expect(unknown).toContain('data-live-word="none"');
    });
  });
});
