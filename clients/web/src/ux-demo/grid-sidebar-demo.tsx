import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Select } from '@kerfjs/ui/select';
import { wirePagedViews } from '@kerfjs/ui/wire-paged-views';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { LiveWord } from '../api';
import { GridSidebarGauges } from '../components/grid-sidebar-gauges';
import {
  GRID_SIDEBAR_PAGE_LABELS,
  GRID_SIDEBAR_PAGED_VIEW_ID,
  GRID_SIDEBAR_PAGES,
  type GridSidebarPage,
  parseGridSidebarPage,
  TerminalOperationsSidebar,
  type TerminalProjectSummary,
} from '../components/terminal-operations-sidebar';
import { type GridSidebarLive, LIVE_WORD_LABELS } from '../grid-sidebar-live';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';

const MINUTE = 60_000;

/** A synthetic live reading (HS2-E1AQE8); every name and number is made up. */
function live(state: LiveWord, overrides: Partial<GridSidebarLive> = {}): GridSidebarLive {
  return {
    state,
    tokensPerMinute: 0,
    tokensPeakPerMinute: 2600,
    ticketsUpdatedPerHour: 0,
    ticketsPeakPerHour: 14,
    waiting: 0,
    workersLive: 0,
    workersExpected: 0,
    ...overrides,
  };
}

/** The catalog's sidebar projects (synthetic): one active, one stopped, one that may be stuck. */
export const GRID_SIDEBAR_DEMO_PROJECTS: readonly TerminalProjectSummary[] = [
  {
    id: 'hotsheet2',
    name: 'HotSheet2',
    completedToday: 1,
    inProgress: 6,
    trend: [1, 1, 3, 2, 5, 4, 1],
    live: live('active', {
      tokensPerMinute: 1240,
      ticketsUpdatedPerHour: 9,
      workersLive: 2,
      workersExpected: 2,
      progressAgeMs: 2 * MINUTE,
      claim: { slug: 'HS2-R76MMW', phase: 'initial_testing' },
    }),
  },
  {
    id: 'best-in-manila',
    name: 'Best-in-Manila',
    completedToday: 0,
    inProgress: 0,
    trend: [0, 0, 0, 0, 0, 0, 0],
    live: live('stopped', { tokensPeakPerMinute: 3100, ticketsPeakPerHour: 18, stoppedAgeMs: 2 * 60 * MINUTE }),
  },
  {
    id: 'kerf',
    name: 'Kerf',
    completedToday: 1,
    inProgress: 1,
    trend: [1, 0, 2, 0, 0, 5, 1],
    live: live('may_be_stuck', {
      tokensPerMinute: 40,
      tokensPeakPerMinute: 1800,
      ticketsPeakPerHour: 8,
      waiting: 2,
      workersLive: 1,
      workersExpected: 2,
      progressAgeMs: 34 * MINUTE,
      claim: { slug: 'KF-4K2P', phase: 'working' },
    }),
  },
];

/** The All projects reading: the sums of the projects above. */
export const GRID_SIDEBAR_DEMO_ALL: GridSidebarLive = live('may_be_stuck', {
  tokensPerMinute: 1280,
  tokensPeakPerMinute: 7500,
  ticketsUpdatedPerHour: 9,
  ticketsPeakPerHour: 40,
  waiting: 2,
  workersLive: 3,
  workersExpected: 4,
  progressAgeMs: 2 * MINUTE,
});

type DemoLiveSource = 'received' | 'none';
const DEFAULTS = { page: 'completion' as GridSidebarPage, live: 'received' as DemoLiveSource };
export const gridSidebarDemoSettings = {
  page: signal<GridSidebarPage>(DEFAULTS.page),
  live: signal<DemoLiveSource>(DEFAULTS.live),
};

export function resetGridSidebarDemo(root?: ParentNode): void {
  gridSidebarDemoSettings.page.value = DEFAULTS.page;
  gridSidebarDemoSettings.live.value = DEFAULTS.live;
  if (root) syncSettingsControls(root, 'grid-sidebar', { values: { ...DEFAULTS } });
}

/** The production sidebar at sidebar width on the chosen page. */
export function TerminalOperationsSidebarDemo() {
  const received = gridSidebarDemoSettings.live.value === 'received';
  const projects = received
    ? GRID_SIDEBAR_DEMO_PROJECTS
    : GRID_SIDEBAR_DEMO_PROJECTS.map(({ id, name, completedToday, inProgress, trend }) => ({
        id,
        name,
        completedToday,
        inProgress,
        trend,
      }));
  return (
    <CatalogExample
      label="Interactive"
      note="Choose the page and whether live state has arrived in settings. Hover a gauge or state glyph for its value, duration, or cause."
      viewport={{ width: 'sidebar' }}
    >
      <TerminalOperationsSidebar
        projects={projects}
        {...(received ? { allLive: GRID_SIDEBAR_DEMO_ALL } : {})}
        page={gridSidebarDemoSettings.page.value}
      />
    </CatalogExample>
  );
}

export function TerminalOperationsSidebarSettings() {
  return (
    <DemoSettingsForm settings="grid-sidebar">
      <Select
        name="page"
        label="Page"
        value={gridSidebarDemoSettings.page.value}
        choices={GRID_SIDEBAR_PAGES.map((page) => ({ value: page, label: GRID_SIDEBAR_PAGE_LABELS[page] }))}
      />
      <Select
        name="live"
        label="Live State"
        value={gridSidebarDemoSettings.live.value}
        choices={[
          { value: 'received', label: 'Received' },
          { value: 'none', label: 'Not Yet Received' },
        ]}
      />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

/** One gauge body per state, plus the aggregate without a glyph and the no-live-state rest. */
export function GridSidebarGaugesDemo() {
  return (
    <>
      {GRID_SIDEBAR_DEMO_PROJECTS.map((project) => (
        <CatalogExample
          label={project.live ? LIVE_WORD_LABELS[project.live.state] : project.name}
          viewport={{ width: 'sidebar' }}
        >
          <GridSidebarGauges projectId={project.id} name={project.name} live={project.live} />
        </CatalogExample>
      ))}
      <CatalogExample label="Waiting on You" viewport={{ width: 'sidebar' }}>
        <GridSidebarGauges
          projectId="review"
          name="UX Review"
          live={live('waiting_on_you', { ticketsUpdatedPerHour: 1, waiting: 2, waitingSlug: 'UXR-7T3Q' })}
        />
      </CatalogExample>
      <CatalogExample label="Stalled" viewport={{ width: 'sidebar' }}>
        <GridSidebarGauges
          projectId="api"
          name="Internal API"
          live={live('stalled', { tokensPeakPerMinute: 1200, stalledSlug: 'API-2M8C', progressAgeMs: 50 * MINUTE })}
        />
      </CatalogExample>
      <CatalogExample label="Halted" viewport={{ width: 'sidebar' }}>
        <GridSidebarGauges
          projectId="api"
          name="Internal API"
          live={live('halted', { tokensPeakPerMinute: 1200, haltedMessage: 'Usage limit reached' })}
        />
      </CatalogExample>
      <CatalogExample label="All Projects (No Glyph)" viewport={{ width: 'sidebar' }}>
        <GridSidebarGauges projectId="all" name="All projects" live={GRID_SIDEBAR_DEMO_ALL} showState={false} />
      </CatalogExample>
      <CatalogExample label="No Live State" viewport={{ width: 'sidebar' }}>
        <GridSidebarGauges projectId="legacy" name="Legacy Archive" />
      </CatalogExample>
    </>
  );
}

/** Install this file's demo interactions on the catalog root. */
function wireGridSidebarDemo(root: HTMLElement): () => void {
  const scope = createScope();
  scope.add(
    delegate(root, 'change', '[data-settings="grid-sidebar"] [name]', (_event, target) => {
      const control = target as FormControl;
      const name = control.getAttribute('name');
      if (name === 'page') gridSidebarDemoSettings.page.value = control.value as GridSidebarPage;
      if (name === 'live') gridSidebarDemoSettings.live.value = control.value as DemoLiveSource;
    }),
  );
  // The production pager wiring: swipes, trackpad scrolls, dots, and arrow keys change the page, which
  // the Page setting mirrors (HS2-EZSQDF). AppShell's Workspace Grid mode shares this page.
  scope.add(
    wirePagedViews(root, {
      onPageChange: (page, view) => {
        if (view !== GRID_SIDEBAR_PAGED_VIEW_ID) return;
        gridSidebarDemoSettings.page.value = parseGridSidebarPage(page);
        syncSettingsControls(root, 'grid-sidebar', { values: { page: gridSidebarDemoSettings.page.value } });
      },
    }),
  );
  return () => {
    scope.dispose();
  };
}

/** The catalog entries this file renders. */
export const demoModule: DemoModule = {
  entries: {
    'terminal-operations-sidebar': {
      catalogExamples: true,
      Demo: TerminalOperationsSidebarDemo,
      Settings: TerminalOperationsSidebarSettings,
      reset: resetGridSidebarDemo,
    },
    'grid-sidebar-gauges': { catalogExamples: true, Demo: GridSidebarGaugesDemo },
  },
  wire: wireGridSidebarDemo,
};
