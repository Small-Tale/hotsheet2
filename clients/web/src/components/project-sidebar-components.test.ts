import { readFileSync } from 'node:fs';

import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Archive, Plus } from 'lucide';
import { describe, expect, it } from 'vitest';

import { CommandNavigation, isCommandNavigationIcon } from './command-navigation';
import { DriveControl } from './drive-control';
import { ProjectSidebar } from './project-sidebar';
import { aggregateAlignedChartValues, chartDomainMaximum, ProjectSummary } from './project-summary';
import { RepositorySummary } from './repository-summary';
import { SavedViewContextMenu, ViewNavigation } from './view-navigation';

describe('ProjectSidebar component slice', () => {
  it('uses one stable menu-item grid for icon, label, trailing content, and selection', () => {
    const markup = String(
      ListItem({
        action: 'select',
        itemId: 'archive',
        selected: true,
        icon: LucideIcon({ icon: Archive, name: 'archive' }),
        label: 'Archive',
        trailing: '241' as never,
      }),
    );
    expect(markup).toContain('data-component="list-item"');
    expect(markup).toContain('data-item-id="archive"');
    expect(markup).toContain('aria-current="page"');
    expect(markup).toContain('list-item__icon');
    expect(markup).toContain('list-item__label');
    expect(markup).toContain('list-item__trailing');
  });

  it('shares one menu-header contract for action and toggle groups', () => {
    const icon = LucideIcon({ icon: Plus, name: 'plus' });
    const action = String(ListHeader({ label: 'Views', action: 'add', actionLabel: 'Add view', actionIcon: icon }));
    const toggle = String(
      ListHeader({ label: 'Commands', action: 'toggle', actionIcon: icon, expanded: false, toggle: true }),
    );
    expect(action).toContain('data-component="list-header"');
    expect(action).toContain('aria-label="Add view"');
    const popoverAction = String(
      ListHeader({
        label: 'Tags',
        action: 'add',
        actionLabel: 'Add tag',
        actionIcon: icon,
        triggerAttributes: { popoverTarget: 'tag-popover', 'aria-haspopup': 'dialog', 'aria-controls': 'tag-popover' },
      }),
    );
    expect(popoverAction).toContain('popoverTarget="tag-popover"');
    expect(popoverAction).toContain('aria-haspopup="dialog"');
    expect(popoverAction).toContain('aria-controls="tag-popover"');
    expect(toggle).toContain('aria-expanded="false"');
    const disabled = String(
      ListHeader({
        label: 'Views',
        action: 'add',
        actionLabel: 'Add view',
        actionIcon: icon,
        actionDisabled: true,
        disabledReason: 'Not available yet.',
      }),
    );
    expect(disabled).toContain('disabled');
    expect(disabled).toContain('title="Not available yet."');
  });
  it('derives project progress bars and accessible summary text from props', () => {
    const markup = String(ProjectSummary({ completedToday: 8, inProgress: 2, trend: [1, 4], projectId: 'demo' }));
    expect(markup).not.toContain('coverage');
    expect(markup).not.toContain('project-summary__coverage');
    expect(markup).toContain('Tickets completed over the last 2 days: 1, 4');
    expect(markup).toContain('8 completed today');
    expect(markup).toContain('2 in progress');
    expect(markup).toContain('data-action="open-project-stats"');
    expect(markup).toContain('data-project-id="demo"');
    expect(markup).toContain('aria-label="Open project statistics: 8 completed today, 2 in progress"');
    expect(markup.match(/data-bar=/g)).toHaveLength(2);
  });

  it('uses an explicit shared chart domain without allowing bars to overflow it', () => {
    const shared = String(ProjectSummary({ completedToday: 4, inProgress: 1, trend: [1, 4], chartMaximum: 8 }));
    expect(shared).toContain('data-chart-maximum="8"');
    expect(shared).toContain('--bar-height:13%');
    expect(shared).toContain('--bar-height:50%');
    const undersized = String(ProjectSummary({ completedToday: 9, inProgress: 1, trend: [9], chartMaximum: 4 }));
    expect(undersized).toContain('data-chart-maximum="9"');
    expect(undersized).toContain('--bar-height:100%');
  });

  it('generalizes aligned aggregate domains across unequal chart series', () => {
    const aggregate = aggregateAlignedChartValues([[2, 1, 4], [3, 5], []]);
    expect(aggregate).toEqual([2, 4, 9]);
    expect(chartDomainMaximum(aggregate)).toBe(9);
    expect(chartDomainMaximum([], 0)).toBe(1);
  });

  it('renders zero-completion days as explicit baseline marks', () => {
    const markup = String(ProjectSummary({ completedToday: 1, inProgress: 0, trend: [0, 1] }));
    expect(markup).toContain('data-zero="true"');
    expect(markup).toContain('data-zero="false"');
  });

  it('aligns an aggregate background behind the project bars and describes both series', () => {
    const markup = String(
      ProjectSummary({ completedToday: 4, inProgress: 1, trend: [2, 4], backgroundTrend: [1, 3, 5], chartMaximum: 5 }),
    );
    expect(markup).toContain('data-chart-background="true"');
    expect(markup).toContain('Tickets completed over the last 3 days: 0, 2, 4. All projects: 1, 3, 5');
    expect(markup.match(/data-background-bar=/g)).toHaveLength(3);
    expect(markup.match(/data-bar=/g)).toHaveLength(3);
    expect(markup).toContain('data-background-bar="2" data-background-zero="false"');
    expect(markup).toContain('--bar-height:100%');
    const css = readFileSync(new URL('./project-summary.css', import.meta.url), 'utf8');
    expect(css).toMatch(
      /project-summary__bar-slot \{[^}]*min-width: remify\(4\.8px\);[^}]*max-width: remify\(14\.4px\);[^}]*flex: 1 1 remify\(14\.4px\)/,
    );
  });

  it('scales each project fill as a proportion of its aggregate background without clamping a small fill up toward the whole bar (HS2-C9JM65)', () => {
    // Day 1: project 4 of aggregate 9; day 2: project 1 of aggregate 9; the tallest aggregate day is 9.
    const markup = String(
      ProjectSummary({ completedToday: 1, inProgress: 0, trend: [4, 1], backgroundTrend: [9, 9], chartMaximum: 9 }),
    );
    // The gray aggregate bars fill the domain (100%) on both days.
    expect(markup.match(/data-background-bar="\d" data-background-zero="false"/g)).toHaveLength(2);
    expect(markup).toContain('--bar-height:100%');
    // Fills are value/maximum: 44% and 11%. The 11% fill must stay proportional (an eleventh of the bar),
    // not be floored up to ~12% where a fraction would read as the whole aggregate bar.
    expect(markup).toContain('--bar-height:44%');
    expect(markup).toContain('--bar-height:11%');
    expect(markup).not.toContain('--bar-height:12%');
    // A small non-zero fill relies on a low min-height for visibility (not a large clamp).
    const css = readFileSync(new URL('./project-summary.css', import.meta.url), 'utf8');
    expect(css).toMatch(
      /project-summary__bar-background,\s*\.project-summary__bar-foreground \{[^}]*min-height: remify\(2px\)/,
    );
  });

  it('renders repository status as one discoverable action', () => {
    const markup = String(RepositorySummary({ branch: 'main', unpushed: 3, behind: 2, uncommitted: 1 }));
    expect(markup).toContain('Repository status for main: 3 ahead, 2 behind, 1 uncommitted, 0 conflicted');
    expect(markup).toContain('data-lucide="git-branch"');
    expect(markup).toContain('repository-summary__branch-name');
    expect(markup).toContain('3 unpushed commits');
    expect(markup).toContain('2 commits behind');
    expect(markup).not.toContain('data-lucide="file-pen-line"');
  });

  it('makes repository failures and conflicts visible from the collapsed summary', () => {
    expect(String(RepositorySummary({ branch: 'main', unpushed: 0, uncommitted: 0, error: true }))).toContain(
      'data-state="error"',
    );
    expect(String(RepositorySummary({ branch: 'main', unpushed: 0, uncommitted: 0, conflicted: 2 }))).toContain(
      'data-state="conflicted"',
    );
  });

  it('projects current view, counts, and attention', () => {
    const markup = String(
      ViewNavigation({
        selectedId: 'all',
        items: [
          { id: 'all', label: 'All Tickets', count: 4, icon: 'all' },
          { id: 'review', label: 'Review', count: 2, attention: true, icon: 'needs-review' },
        ],
      }),
    );
    expect(markup.match(/class="kui-list-item__count"/g)).toHaveLength(2);
    expect(markup).toContain('aria-current="page"');
    expect(markup).toContain('data-attention="true"');
    expect(markup).toContain('aria-label="Add view"');
    expect(markup).toMatch(/aria-current="page"[\s\S]*class="kui-list-item__count"/);
    expect(markup).not.toContain('disabled');
    const searching = String(
      ViewNavigation({
        selectedId: 'all',
        items: [
          { id: 'all', label: 'Queue', countLoading: true, icon: 'all' },
          { id: 'archive', label: 'Archive', count: 3, searchCount: true, icon: 'archive' },
        ],
      }),
    );
    expect(searching).toContain('aria-label="Searching this view"');
    expect(searching).toContain('data-lucide="loader-circle"');
    expect(searching).toContain('aria-label="3 search results"');
    expect(searching).toContain('data-lucide="search"');
    const custom = String(
      ViewNavigation({
        selectedId: 'custom:docs',
        items: [{ id: 'custom:docs', label: 'Needs docs', icon: 'custom', manageable: true }],
      }),
    );
    expect(custom).toContain('data-lucide="search"');
    expect(custom).toContain('aria-label="More actions for Needs docs"');
    expect(custom).not.toContain('aria-label="Rename Needs docs"');
    expect(custom).not.toContain('aria-label="Delete Needs docs"');
    const menu = String(SavedViewContextMenu({ id: 'custom:docs', label: 'Needs docs', x: 20, y: 30 }));
    expect(menu).toContain('aria-label="Needs docs view actions"');
    expect(menu).toContain('data-action="edit-saved-view"');
    expect(menu).toContain('data-action="delete-saved-view"');
  });

  it('gives parsing errors a distinct shared navigation item', () => {
    const markup = String(
      ViewNavigation({
        selectedId: 'errors',
        items: [{ id: 'errors', label: 'Ticket errors', count: 2, attention: true, icon: 'errors' }],
      }),
    );
    expect(markup).toContain('menu-item--errors');
    expect(markup).toContain('data-lucide="file-warning"');
    expect(markup).toContain('aria-current="page"');
  });

  it('renders expanded command state and running presentation', () => {
    const markup = String(
      CommandNavigation({
        label: 'Commands',
        expanded: true,
        commands: [
          {
            id: 'test',
            label: 'Test',
            color: '#3b82f6',
            icon: 'test',
            group: 'Checks',
            running: true,
            lastRun: 'completed (exit 0)',
          },
        ],
      }),
    );
    expect(markup).toContain('aria-expanded="true"');
    expect(markup).toContain('Running Test');
    expect(markup).toContain('data-lucide="test-tube-2"');
    expect(markup).toContain('data-command-color="#3b82f6"');
    expect(markup).toContain('data-command-group="Checks"');
    expect(markup).toContain('data-action="toggle-command-section"');
    expect(markup).toContain('Last run: completed (exit 0). Press and hold for output.');
  });

  it('shows half-opacity type icons for idle shell and AI commands', () => {
    const markup = String(
      CommandNavigation({
        label: 'Commands',
        expanded: true,
        commands: [
          { id: 'shell', label: 'Shell', color: 'blue', icon: 'build', kind: 'shell' },
          { id: 'ai', label: 'AI', color: 'purple', icon: 'send', kind: 'ai' },
        ],
      }),
    );
    expect(markup).toContain('aria-label="Shell command"');
    expect(markup).toContain('data-lucide="square-terminal"');
    expect(markup).toContain('aria-label="AI command"');
    expect(markup).toContain('data-lucide="bot"');
    const css = readFileSync(new URL('./command-navigation.css', import.meta.url), 'utf8');
    expect(css).toMatchSource(/\.command-navigation__type \{[^}]*opacity: \.5/);
  });

  it('renders every configured HS1 command icon instead of replacing it by command kind', () => {
    const configured = [
      'send',
      'file-text',
      'arrow-left-right',
      'soap-dispenser-droplet',
      'circle-check-big',
      'balloon',
      'git-compare',
      'git-compare-arrows',
      'wand',
      'globe',
    ] as const;
    expect(configured.every(isCommandNavigationIcon)).toBe(true);
    const markup = String(
      CommandNavigation({
        label: 'Commands',
        expanded: true,
        commands: configured.map((icon, index) => ({ id: String(index), label: icon, color: 'blue', icon })),
      }),
    );
    for (const icon of configured) expect(markup).toContain(`data-lucide="${icon}"`);
  });

  it('collapses named command groups independently without hiding ungrouped commands', () => {
    const markup = String(
      CommandNavigation({
        label: 'Commands',
        expanded: true,
        collapsedGroups: ['Quality'],
        commands: [
          { id: 'test', label: 'Test', color: 'blue', icon: 'test', group: 'Quality' },
          { id: 'ship', label: 'Ship', color: 'purple', icon: 'send', group: 'Release' },
          { id: 'status', label: 'Status', color: 'green', icon: 'build' },
        ],
      }),
    );
    expect(markup).toMatch(/data-command-group="Quality"[^]*aria-expanded="false"/);
    expect(markup).not.toContain('data-item-id="test"');
    expect(markup).toMatch(/data-command-group="Release"[^]*aria-expanded="true"/);
    expect(markup).toContain('data-item-id="ship"');
    expect(markup).toContain('data-item-id="status"');
  });

  it('renders a neutral or unrecognized command color as a transparent, unstyled button (HS2-ZQWQCM)', () => {
    const markup = String(
      CommandNavigation({
        label: 'Commands',
        expanded: true,
        commands: [{ id: 'custom', label: 'Custom', color: '#123456', icon: 'build' }],
      }),
    );
    expect(markup).toContain('data-command-color="transparent"');
    expect(markup).not.toContain('--command-color:'); // no color overrides are applied, so the default styling shows
  });

  it('changes drive action semantics with running state', () => {
    const idle = String(DriveControl({ running: false, tool: 'Codex', optionsOpen: true }));
    expect(idle).toContain('Drive with Codex');
    expect(idle).toContain('data-action="toggle-drive-options"');
    expect(idle).toContain('data-lucide="triangle"');
    expect(idle).toContain('aria-expanded="true"');
    const running = String(DriveControl({ running: true, tool: 'Codex' }));
    expect(running).toContain('Codex workflow is running');
    const disabled = String(
      DriveControl({ running: true, tool: 'Codex', disabled: true, disabledReason: 'Cannot stop here.' }),
    );
    expect(disabled).toContain('disabled');
    expect(disabled).toContain('title="Cannot stop here."');
  });

  it('composes the five sidebar boundaries without duplicating their markup', () => {
    const markup = String(
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
    for (const component of [
      'project-summary',
      'repository-summary',
      'view-navigation',
      'command-navigation',
      'drive-control',
    ])
      expect(markup).toContain(`data-component="${component}"`);
    expect(markup).toContain('data-component="project-work-summary">7 open, 3 up next, 2 active');
    expect(markup.indexOf('project-work-summary')).toBeLessThan(markup.indexOf('data-component="drive-control"'));
  });

  it('builds the sidebar on the unpadded kui-pane so children own their inset', () => {
    const markup = String(
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
        commands: [],
        commandGroupExpanded: true,
        driveRunning: false,
        driveTool: 'codex',
        openCount: 7,
        upNextCount: 3,
        activeCount: 2,
        collapseControl: true,
      }),
    );
    expect(markup).toContain('class="kui-pane project-sidebar"');
    expect(markup).toContain('data-component="pane"');
    expect(markup).toContain('class="kui-pane__content kui-content project-sidebar__content"');
    expect(markup).toContain('class="kui-pane__footer project-sidebar__footer"');
    const css = readFileSync(new URL('./project-sidebar.css', import.meta.url), 'utf8');
    // The pane shell owns only card chrome — no padding and no zeroed inline margin.
    const shell = css.match(/\.project-sidebar \{([^}]*)\}/)?.[1] ?? '';
    expect(shell).not.toMatch(/(^|;|\{)\s*padding:/);
    expect(shell).not.toContain('--kui-layout-inline-margin');
    expect(shell).not.toContain('--kui-layout-item-padding');
    // Non-menu content children self-inset instead of leaning on the shell.
    expect(css).toMatch(/\.project-sidebar__content > \.project-summary \{[^}]*margin-inline: var\(--kui-space-xs\)/);
    expect(css).toMatch(/\.project-sidebar__footer \{[^}]*padding: var\(--kui-space-xs\)/);
    // The collapse toolbar aligns its lone control to the gutter with flex, not the old negative-margin hack.
    expect(css).toMatch(/\.project-sidebar > \.kui-pane__header > \.kui-toolbar \{[^}]*justify-content: flex-end/);
    expect(css).not.toMatch(/\.project-sidebar > \.kui-pane__header > \.kui-toolbar \{[^}]*margin:/);
  });

  it('omits the command section when the project has no commands', () => {
    const markup = String(
      ProjectSidebar({
        completedToday: 0,
        inProgress: 0,
        completionTrend: [],
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
        openCount: 0,
        upNextCount: 0,
        activeCount: 0,
      }),
    );
    expect(markup).not.toContain('data-component="command-navigation"');
    expect(markup).not.toContain('Project commands');
  });

  it('offers split-button overrides and a separate chat before the workflow starts', () => {
    const base = {
      completedToday: 0,
      inProgress: 0,
      completionTrend: [],
      branch: 'main',
      unpushed: 0,
      uncommitted: 0,
      views: [],
      selectedViewId: 'all',
      commandGroupLabel: 'Commands',
      commands: [],
      commandGroupExpanded: true,
      driveRunning: false,
      driveTool: 'codex' as const,
      openCount: 0,
      upNextCount: 0,
      activeCount: 0,
    };
    const markup = String(ProjectSidebar(base));
    expect(markup).not.toContain('name="drive-tool"');
    expect(markup).toContain('Drive with Codex');
    expect(markup).toContain('Choose Drive provider, model, and effort');
    expect(markup).toContain(
      'data-action="open-conversation" aria-label="Open Codex conversation" title="Open chat without starting the Hot Sheet workflow" aria-pressed="false"',
    );
    expect(markup).not.toContain('aria-pressed="false" disabled');
    expect(String(ProjectSidebar({ ...base, driveTool: 'claude', conversationOpen: true }))).toContain(
      'aria-label="Open Claude conversation"',
    );
    const options = String(
      ProjectSidebar({
        ...base,
        driveOptionsOpen: true,
        driveTools: [{ id: 'codex', display_name: 'Codex', models: [] }],
        driveSelection: { tool: 'codex' },
        driveDefaultSelection: { tool: 'codex' },
      }),
    );
    expect(options).toContain('aria-label="Drive provider, model, and effort options"');
  });
});
