import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import {
  applyWorkspaceSortDirection,
  nextWorkspaceSort,
  WorkspaceControls,
  WorkspaceHeader,
  type WorkspaceSort,
  type WorkspaceSortDirection,
  workspaceSortTrigger,
  type WorkspaceUpNextState,
  workspaceUpNextState,
  type WorkspaceViewMode,
} from './workspace-header';

describe('WorkspaceHeader', () => {
  it('projects all modes, resets, and edits through canonical native segments', () => {
    const icons = { list: 'list', board: 'columns-3', notifications: 'bell', settings: 'settings' };
    for (const mode of ['list', 'board', 'notifications', 'settings', 'list', 'board'] satisfies WorkspaceViewMode[]) {
      const markup = String(WorkspaceControls({ mode }));
      expect(markup.match(/aria-label="View mode"/g)).toHaveLength(1);
      expect(markup).toContain('data-component="segmented-control"');
      expect(markup).toContain('data-appearance="toolbar" data-shape="pill"');
      expect(markup).toContain('data-layout="content"');
      const buttons = [...markup.matchAll(/<button[^>]*data-segment-value="([^"]+)"[^>]*>[\s\S]*?<\/button>/g)];
      expect(buttons).toHaveLength(4);
      for (const [button, value] of buttons) {
        expect(button).toContain('data-action="set-view-mode"');
        expect(button).toContain('tabindex="0"');
        expect(button).toContain(`aria-pressed="${String(value === mode)}"`);
        expect(button).toContain(`data-selected="${String(value === mode)}"`);
        expect(button).toContain(`data-lucide="${icons[value as WorkspaceViewMode]}"`);
      }
    }
  });

  it.each([0, 7, 120])('preserves the full accessible notification count %s and clamped badge', (count) => {
    const markup = String(WorkspaceControls({ mode: 'notifications', notificationCount: count }));
    expect(markup).toContain(`aria-label="Notifications view${count ? `, ${count} pending` : ''}"`);
    if (count)
      expect(markup).toContain(
        `class="view-mode-switcher__badge" aria-hidden="true">${count > 99 ? '99+' : count}</span>`,
      );
    else expect(markup).not.toContain('view-mode-switcher__badge');
  });

  it('offers only equal-width List and Notifications in the explicit rounded rail presentation', () => {
    const markup = String(WorkspaceControls({ mode: 'notifications', presentation: 'rail', notificationCount: 7 }));
    expect(markup).toContain('data-shape="rounded"');
    expect(markup).toContain('data-layout="equal"');
    expect(markup.match(/data-action="set-view-mode"/g)).toHaveLength(2);
    for (const mode of ['board', 'settings']) {
      expect(markup).not.toContain(`data-segment-value="${mode}"`);
      expect(markup).not.toContain(`data-view-mode="${mode}"`);
    }
  });

  it('exposes an accessible selected view mode and optional search field', () => {
    const markup = String(
      WorkspaceHeader({
        projectName: 'Hot Sheet 2',
        mode: 'settings',
        searchOpen: true,
        searchQuery: 'NOT  AND tag:cl',
        searchTokens: [{ kind: 'tag', value: 'server', raw: 'tag:server', label: 'tag:server', offset: 4 }],
        searchTagSuggestions: ['client'],
        searchDatePrefix: 'created-after',
        searchHelpOpen: true,
        sort: 'priority',
        sortDirection: 'descending',
      }),
    );
    expect(markup).not.toContain('All Tickets');
    expect(markup).toContain(
      'data-component="toolbar-text" data-size="large"><span class="kui-toolbar-text__text">Hot Sheet 2',
    );
    expect(markup).toContain('aria-label="View mode"');
    expect(markup).toContain(
      'data-segment-value="settings" data-selected="true" aria-label="Settings view" aria-pressed="true"',
    );
    expect(markup).toContain('data-token-search-editor="workspace-search"');
    expect(markup).toContain('role="searchbox" aria-label="Search tickets"');
    expect(markup).toContain('contenteditable="false"');
    expect(markup).toMatch(
      /data-token-search-text data-empty="false">NOT <\/span><span class="kui-token-search__token"[^>]*data-token-value="tag:server">.*tag:server.*data-token-search-text data-empty="false"> AND tag:cl<\/span>/s,
    );
    expect(markup).toContain('aria-label="Edit tag server"');
    expect(markup).toContain('>tag:server</button>');
    expect(markup).toContain('aria-label="Remove tag server"');
    expect(markup).toContain('data-action="select-workspace-search-tag" data-tag="client"');
    expect(markup).toContain('name="workspace-sort"');
    expect(markup).toContain('aria-label="Sort tickets: Priority, descending"');
    expect(markup).toContain('<wa-option value="priority"');
    expect(markup).toContain('class="kui-select__custom-selected"><svg data-lucide="arrow-down-wide-narrow"');
    expect(markup).not.toContain('<input type="checkbox"');
    expect(markup).toMatch(/workspace-header__search-group"[^>]*data-expanded="true"/);
    expect(markup).toContain('data-collapsible="true" data-expanded="true"');
    expect(markup).not.toContain('workspace-header__search-tokens');
    expect(markup).toContain('aria-label="Search syntax help"');
    expect(markup).toContain('aria-label="Date and time helper"');
    expect(markup).toContain('aria-label="Search syntax"');
    expect(markup).toContain('<dt>Tags</dt>');
    expect(markup).toContain('<dt>Content</dt>');
    expect(markup).toContain('<dt>Workflow</dt>');
    expect(markup).toContain('<code>is:closed</code>');
    expect(markup).toContain('<code>is:duplicate</code>');
    expect(markup).toContain('<dt>Dates</dt>');
    expect(markup).toContain('<strong>Combine filters</strong>');
    expect(markup).toContain('local, relative, and ISO 8601 dates work');
    expect(markup).toContain('updated-after:2026-09-01T11:05');
    expect(markup).not.toContain('class="workspace-header__search-button"');
    expect(markup).not.toContain('data-action="open-global-search"');
    expect(markup.indexOf('workspace-header__utility-group')).toBeLessThan(markup.indexOf('workspace-header__search'));
    const beforeOverflow = markup.slice(0, markup.indexOf('<wa-dropdown class="workspace-header__overflow"'));
    expect(beforeOverflow).toMatch(/name="workspace-sort"[^>]*disabled/);
    expect(
      (
        beforeOverflow.match(/disabled[^>]*data-action="(?:toggle-selected-up-next|open-selected-ticket-actions)"/g) ??
        []
      ).length,
    ).toBe(2);
    expect(beforeOverflow).toContain(
      'data-component="token-search-field" data-token-search-id="workspace-search" data-disabled="true"',
    );
    const headerCss = readFileSync(resolve(import.meta.dirname, 'workspace-header.css'), 'utf8'),
      shellCss = readFileSync(resolve(import.meta.dirname, 'app-shell.css'), 'utf8');
    expect(headerCss).toContainSource(
      '.workspace-header__search-group[data-expanded="true"] { max-width:100%; height:auto; overflow:visible; align-self:flex-start; }',
    );
    expect(headerCss).toContainSource(
      '.workspace-header__actions > .workspace-header__search-group.kui-toolbar-control-group[data-expanded="true"] { --kui-token-search-expanded-width: 48rem; min-width: 19rem; flex: 1 1 19rem; }',
    );
    expect(headerCss).toContainSource(
      '.workspace-header__search-group .kui-token-search { --kui-token-search-background: var(--wa-color-surface-default); --kui-token-search-border: var(--wa-color-neutral-border-normal); --kui-token-search-token-background: var(--wa-color-brand-fill-quiet); --kui-token-search-token-foreground: var(--wa-color-brand-on-quiet); }',
    );
    expect(headerCss).toContainSource(
      '.workspace-header__search-suggestions{display:flex;box-sizing:border-box;width:min(remify(416px),100%);align-items:stretch;flex-direction:column;text-align:left}',
    );
    expect(headerCss).toContainSource(
      '.workspace-header__search-suggestions button{display:block;box-sizing:border-box;width:100%;',
    );
    expect(headerCss).toContainSource(
      '.workspace-header__actions[data-search-open="true"] > .workspace-header__overflow { display: none; }',
    );
    expect(headerCss).toContainSource('wa-button.workspace-header__text-action::part(base) { width: auto;');
    expect(headerCss).toContainSource(
      '.workspace-header__text-action-label { display: inline-flex; align-items: center;',
    );
    expect(shellCss).toContainSource(
      '.app-shell__main > .kui-toolbar:has(.workspace-header__search-group[data-expanded="true"]) { height:auto; align-items:start; }',
    );
  });

  it.each([
    ['updated', 'ascending', 'clock-arrow-up'],
    ['updated', 'descending', 'clock-arrow-down'],
    ['priority', 'ascending', 'arrow-up-narrow-wide'],
    ['priority', 'descending', 'arrow-down-wide-narrow'],
    ['title', 'ascending', 'arrow-down-a-z'],
    ['title', 'descending', 'arrow-up-a-z'],
    ['status', 'ascending', 'list-sort-ascending'],
    ['status', 'descending', 'list-sort-descending'],
  ] satisfies Array<[WorkspaceSort, WorkspaceSortDirection, string]>)(
    'uses a semantic trigger for %s %s',
    (sort, direction, iconName) => {
      expect(workspaceSortTrigger(sort, direction).iconName).toBe(iconName);
      const label = sort === 'updated' ? 'Recently updated' : sort[0].toUpperCase() + sort.slice(1);
      expect(
        String(WorkspaceHeader({ projectName: 'Hot Sheet 2', mode: 'list', sort, sortDirection: direction })),
      ).toContain(`aria-label="Sort tickets: ${label}, ${direction}"`);
    },
  );

  it('retains the selected sort while toggling its direction', () => {
    expect(nextWorkspaceSort('updated', 'descending', 'priority')).toEqual({
      sort: 'priority',
      direction: 'ascending',
    });
    expect(nextWorkspaceSort('priority', 'ascending', 'priority')).toEqual({
      sort: 'priority',
      direction: 'descending',
    });
    expect(nextWorkspaceSort('priority', 'descending', 'priority')).toEqual({
      sort: 'priority',
      direction: 'ascending',
    });
    expect(nextWorkspaceSort('priority', 'descending', 'updated')).toEqual({
      sort: 'updated',
      direction: 'descending',
    });
    expect(applyWorkspaceSortDirection(-3, 'ascending')).toBe(-3);
    expect(applyWorkspaceSortDirection(-3, 'descending')).toBe(3);
  });

  it('keeps an editable caret boundary after a trailing filter chip', () => {
    const markup = String(
      WorkspaceHeader({
        projectName: 'Hot Sheet 2',
        mode: 'list',
        searchOpen: true,
        searchTokens: [{ kind: 'tag', value: 'client', raw: 'tag:client', label: 'tag:client', offset: 0 }],
      }),
    );
    expect(markup).toMatch(
      /data-token-value="tag:client"[\s\S]*<\/span><span data-token-search-text data-empty="true">\u200b<\/span><\/div>/,
    );
  });

  it('omits status sorting from column view while retaining it for list view', () => {
    expect(String(WorkspaceHeader({ projectName: 'Hot Sheet 2', mode: 'board' }))).not.toContain(
      '<wa-option value="status"',
    );
    expect(String(WorkspaceHeader({ projectName: 'Hot Sheet 2', mode: 'list' }))).toContain(
      '<wa-option value="status"',
    );
  });

  it('hides the columns/board view toggle and its overflow entry when listOnly (mobile) (HS2-1XCHZT)', () => {
    const desktop = String(WorkspaceHeader({ projectName: 'Hot Sheet 2', mode: 'list' }));
    // Desktop keeps the columns toggle button and its overflow entry.
    expect(desktop).toContain('data-segment-value="board" data-selected="false" aria-label="Columns view"');
    expect(desktop).toContain('data-workspace-overflow-action="set-view-mode" data-view-mode="board"');
    expect(desktop.match(/data-action="set-view-mode"/g)).toHaveLength(4);

    const mobile = String(WorkspaceHeader({ projectName: 'Hot Sheet 2', mode: 'list', listOnly: true }));
    // Mobile drops the board toggle and its overflow entry, keeping list/notifications/settings.
    expect(mobile).not.toContain('data-view-mode="board"');
    expect(mobile).not.toContain('Show Columns View');
    expect(mobile.match(/data-action="set-view-mode"/g)).toHaveLength(3);
    expect(mobile).toContain(
      'data-segment-value="list" data-selected="true" aria-label="List view" aria-pressed="true"',
    );
  });

  it('delegates the collapsed find state to the canonical TokenSearchField trigger', () => {
    const markup = String(
      WorkspaceHeader({
        projectName: 'Hot Sheet 2',
        mode: 'list',
        notificationCount: 7,
        searchTagSuggestions: ['client'],
        searchDatePrefix: 'created-after',
        searchHelpOpen: true,
      }),
    );
    expect(markup).toMatch(/workspace-header__search-group"[^>]*data-expanded="false"/);
    expect(markup).toContain('data-component="token-search-field" data-token-search-id="workspace-search"');
    expect(markup).toContain('data-collapsible="true" data-expanded="false"');
    expect(markup).toContain(
      'class="kui-token-search__expand" data-action="expand-token-search" aria-label="Search tickets"',
    );
    expect(markup).not.toContain('class="workspace-header__search-button"');
    expect(markup).not.toContain('aria-label="Matching tags"');
    expect(markup).not.toContain('aria-label="Date and time helper"');
    expect(markup).not.toContain('aria-label="Search syntax"');
    expect(markup).toContain('aria-label="Notifications view, 7 pending"');
    expect(markup).toContain('class="view-mode-switcher__badge" aria-hidden="true">7</span>');
    expect(markup.match(/data-action="set-view-mode"/g)).toHaveLength(4);
    expect(markup).not.toContain('data-workspace-search="true"');
    const css = readFileSync(resolve(import.meta.dirname, 'workspace-header.css'), 'utf8');
    expect(css).toMatch(
      /\.view-mode-switcher__badge \{[^}]*min-width: remify\(14\.4px\);[^}]*padding: remify\(1px\) remify\(5px\);/,
    );
    expect(css).not.toMatch(/\.view-mode-switcher__badge \{[^}]*(?:^|[;{]\s*)height:/);
  });

  it('enables selected-ticket actions and reflects the shared Up Next state', () => {
    const empty = String(WorkspaceHeader({ projectName: 'Hot Sheet 2', mode: 'list' }));
    expect(empty).toMatch(/<button[^>]*disabled[^>]*data-action="toggle-selected-up-next"/);
    expect(empty).toMatch(/<button[^>]*disabled[^>]*data-action="open-selected-ticket-actions"/);
    const selected = String(
      WorkspaceHeader({
        projectName: 'Hot Sheet 2',
        mode: 'list',
        selectedTicketCount: 2,
        selectedTicketsUpNext: 'all',
        selectedTicketsUpNextEligible: true,
      }),
    );
    expect(selected).toMatch(/data-action="toggle-selected-up-next"[^>]*aria-pressed="true"/);
    expect(selected).not.toMatch(/data-action="toggle-selected-up-next"[^>]*disabled/);
    expect(selected).not.toMatch(/data-action="open-selected-ticket-actions"[^>]*disabled/);
  });

  it.each([
    [[], 'none'],
    [[false], 'none'],
    [[true], 'all'],
    [[false, true], 'mixed'],
    [[true, true], 'all'],
    [[false, false], 'none'],
  ] satisfies Array<[boolean[], WorkspaceUpNextState]>)('projects Up Next selection %j as %s', (values, state) => {
    expect(workspaceUpNextState(values)).toBe(state);
    const markup = String(
      WorkspaceHeader({
        projectName: 'Demo',
        mode: 'list',
        selectedTicketCount: values.length,
        selectedTicketsUpNext: state,
        selectedTicketsUpNextEligible: values.length > 0,
      }),
    );
    expect(markup).toContain(`aria-pressed="${state === 'mixed' ? 'mixed' : String(state === 'all')}"`);
    expect(markup.match(new RegExp(`data-up-next-state="${state}"`, 'g'))).toHaveLength(2);
    expect(markup.match(/workspace-header__up-next-fill/g) ?? []).toHaveLength(state === 'mixed' ? 2 : 0);
    const utility = markup.slice(
      markup.indexOf('workspace-header__utility-group'),
      markup.indexOf('workspace-header__search-group'),
    );
    expect(utility).not.toContain('wa-button');
    expect(utility.match(/<button /g)).toHaveLength(2);
  });

  it.each([
    { selectedTicketsUpNextEligible: false, selectedTicketsMutable: true },
    { selectedTicketsUpNextEligible: true, selectedTicketsMutable: false },
  ])('preserves eligibility and provider capability guards: %j', (guards) => {
    const markup = String(WorkspaceHeader({ projectName: 'Demo', mode: 'list', selectedTicketCount: 1, ...guards }));
    expect(markup).toMatch(/<button[^>]*disabled[^>]*data-action="toggle-selected-up-next"/);
    expect(markup).toMatch(
      /<wa-dropdown-item[^>]*disabled[^>]*data-workspace-overflow-action="toggle-selected-up-next"/,
    );
  });

  it('omits every project control for global shell modes', () => {
    const markup = String(WorkspaceHeader({ projectName: 'Terminals', mode: 'list', controlsVisible: false }));
    expect(markup).toContain('data-controls-visible="false"');
    expect(markup).not.toContain('workspace-header__actions');
    expect(markup).not.toContain('Search tickets');
  });

  it('relocates every progressively hidden command into the responsive overflow', () => {
    const markup = String(
      WorkspaceHeader({
        projectName: 'Hot Sheet 2',
        mode: 'board',
        sort: 'priority',
        sortDirection: 'descending',
        notificationCount: 7,
        selectedTicketCount: 2,
        selectedTicketsUpNext: 'all',
        selectedTicketsUpNextEligible: true,
      }),
    );
    expect(markup).toContain('aria-label="More workspace controls"');
    expect(markup).toMatch(
      /workspace-header__overflow-utility" aria-label="Toggle Up Next: all selected tickets are Up Next" data-workspace-overflow-action="toggle-selected-up-next"/,
    );
    expect(markup).toContain(
      'workspace-header__overflow-utility" data-workspace-overflow-action="open-selected-ticket-actions"',
    );
    expect(markup).toContain('data-workspace-overflow-action="set-workspace-sort" data-workspace-sort="priority"');
    expect(markup).not.toContain('data-workspace-sort="status"');
    expect(markup).toContain(
      'workspace-header__overflow-search" data-workspace-overflow-action="open-workspace-search"',
    );
    expect(markup).toContain('data-view-mode="notifications"');
    expect(markup).toContain('Show Notifications (7 pending)');
    const headerCss = readFileSync(resolve(import.meta.dirname, 'workspace-header.css'), 'utf8');
    expect(headerCss).toContainSource('.workspace-header__sort { width: remify(44px); }');
    // The single sort group owns the pill; the Select combobox stays transparent so it does not
    // draw a second, shorter pill that pokes out of the group's rounding (HS2-W3VD53).
    expect(headerCss).toMatch(/\.workspace-header__sort::part\(combobox\) \{[^}]*background: transparent/);
    // The focus ring is a single pill on the group (following its radius), not a mismatched ring on
    // the smaller inner combobox (HS2-M1DF1D).
    expect(headerCss).toContainSource('.workspace-header__sort-group { border-radius: var(--wa-border-radius-pill); }');
    expect(headerCss).toContainSource(
      '.workspace-header__sort-group:focus-within, .workspace-header__sort-group:has(.workspace-header__sort[open]) { outline: var(--wa-focus-ring)',
    );
    expect(headerCss).toMatch(/\.workspace-header__sort::part\(combobox\) \{[^}]*outline: none/);
    expect(headerCss).toContainSource(
      '.workspace-header__sort .kui-select__custom-selected { width: remify(16px); height: remify(16px); color: var(--kui-toolbar-control-color);',
    );
    expect(headerCss).toContainSource('@container kui-toolbar (max-width: remify(480px))');
    expect(headerCss).toContainSource(
      '.workspace-header__actions > .workspace-header__utility-group { display: none; }',
    );
    expect(headerCss).toContainSource(
      '.workspace-header__actions > .workspace-header__overflow { display: inline-flex; }',
    );
    expect(headerCss).toContainSource('@container kui-toolbar (max-width: remify(416px))');
    expect(headerCss).toContainSource('.workspace-header__actions > .workspace-header__sort-group { display: none; }');
    expect(headerCss).toContainSource('@container kui-toolbar (max-width: remify(224px))');
    expect(headerCss).toContainSource(
      '.workspace-header__actions > .workspace-header__search-group:not([data-expanded="true"]) { display: none; }',
    );
    expect(headerCss).toContainSource(
      '@container kui-toolbar (max-width: remify(176px)) { .workspace-header__actions > .view-mode-switcher { display: none; } }',
    );
    expect(headerCss).not.toContain('overflow: hidden; } .workspace-header__actions');
  });
});
