import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { createTicketSearchModel } from '../ticket-search-model';
import {
  applyWorkspaceSortDirection,
  EmptyTrashAction,
  nextWorkspaceSort,
  WorkspaceControls,
  WorkspaceHeader,
  WorkspaceIdentity,
  type WorkspaceSort,
  type WorkspaceSortDirection,
  workspaceSortTrigger,
  type WorkspaceUpNextState,
  workspaceUpNextState,
  type WorkspaceViewMode,
} from './workspace-header';

/** A Kerf-managed model edited (not committed) to `text`, as the live field would be mid-typing. */
function searchModelWith(text: string, tags: readonly string[]) {
  const model = createTicketSearchModel({ tags: () => tags });
  model.edit({ query: text, tokens: [] });
  return model;
}

describe('WorkspaceHeader', () => {
  it('collapses disabled search in Notifications and Settings even if an open state is supplied (HS2-6ZK9KF)', () => {
    for (const mode of ['notifications', 'settings'] as const) {
      const markup = String(WorkspaceControls({ mode, searchOpen: true }));
      expect(markup).toMatch(/class="kui-toolbar-control-group ticket-search-field"[^>]*data-expanded="false"/);
      expect(markup).toContain('data-search-open="false"');
      expect(markup).toContain('data-token-search-keep-open="true"');
    }
    expect(String(WorkspaceControls({ mode: 'list', searchOpen: true }))).toMatch(
      /class="kui-toolbar-control-group ticket-search-field"[^>]*data-expanded="true"/,
    );
  });
  it('projects a compact primary heading when the project view title moves into the main toolbar', () => {
    const markup = String(WorkspaceIdentity({ projectName: 'Queue', id: 'workspace-page-title', headingLevel: 1 }));
    // The identity is a real ToolbarText, so a Toolbar leading zone accepts it (HS2-EZ1N7Z).
    expect(markup).toContain('class="kui-toolbar-text workspace-header__identity" data-component="toolbar-text"');
    expect(markup).toContain('id="workspace-page-title"');
    expect(markup).toContain('data-size="large"');
    expect(markup).toContain('role="heading" aria-level="1"');
    expect(markup).toContain('>Queue</span>');
  });
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

  it('offers equal-width List, Columns, and Notifications in the explicit rounded rail presentation', () => {
    const markup = String(WorkspaceControls({ mode: 'notifications', presentation: 'rail', notificationCount: 7 }));
    expect(markup).toContain('data-shape="rounded"');
    expect(markup).toContain('data-layout="equal"');
    // The rail pages a Columns view like the phone board; Settings stays in the main workspace (HS2-656Q43).
    expect(markup.match(/data-action="set-view-mode"/g)).toHaveLength(3);
    expect(markup).toContain('data-segment-value="board"');
    expect(markup).not.toContain('data-workspace-overflow');
    expect(markup).not.toContain('data-segment-value="settings"');
    expect(markup).not.toContain('data-view-mode="settings"');
  });

  it('exposes an accessible selected view mode and optional search field', () => {
    const markup = String(
      WorkspaceHeader({
        projectName: 'Hot Sheet 2',
        mode: 'settings',
        searchOpen: true,
        searchModel: searchModelWith('NOT tag:server AND tag:cl', ['client', 'server']),
        searchHelpOpen: true,
        sort: 'priority',
        sortDirection: 'descending',
      }),
    );
    const enabled = String(
      WorkspaceControls({
        mode: 'list',
        searchOpen: true,
        searchModel: searchModelWith('NOT tag:server AND tag:cl', ['client', 'server']),
      }),
    );
    expect(markup).not.toContain('All Tickets');
    expect(markup).toContain('data-component="toolbar-text"');
    expect(markup).toContain('data-hide-below="224px"');
    expect(markup).toContain('<span class="kui-toolbar-text__text">Hot Sheet 2');
    expect(markup).toContain('aria-label="View mode"');
    expect(markup).toContain(
      'data-segment-value="settings" data-selected="true" aria-label="Settings view" aria-pressed="true"',
    );
    expect(markup).not.toContain('data-token-search-editor="workspace-search"');
    expect(enabled).toContain('data-token-search-editor="workspace-search"');
    expect(enabled).toContain('role="searchbox" aria-label="Search tickets"');
    expect(enabled).toMatch(
      /data-token-search-text data-empty="false">NOT <\/span><span class="kui-token-search__token"[^>]*data-token-value="tag:server">.*tag:server.*data-token-search-text data-empty="false"> AND tag:cl<\/span>/s,
    );
    expect(enabled).toContain('aria-label="Edit tag:server"');
    expect(enabled).toContain('>tag:server</button>');
    expect(enabled).toContain('aria-label="Remove tag:server"');
    // The settings view disables the field, and Kerf renders no suggestions for a disabled field; an
    // enabled field shows Kerf's in-flow rows without the committed tag (HS2-5JXBQY).
    expect(markup).not.toContain('kui-token-search__suggestion');
    expect(enabled).toContain('class="kui-token-search__suggestion" data-token-search-suggestion="tag:client"');
    expect(enabled).not.toContain('data-token-search-suggestion="tag:server"');
    expect(enabled).toContain('data-action="edit-ticket-search-token"');
    expect(enabled).toContain('data-action="clear-ticket-search"');
    expect(markup).toContain('name="workspace-sort"');
    expect(markup).toContain('aria-label="Sort tickets: Priority, descending"');
    expect(markup).toContain('<wa-option value="priority"');
    expect(markup).toContain(
      'class="kui-select__custom-selected"><span class="kui-select__custom-selected-content"><svg data-lucide="arrow-down-wide-narrow"',
    );
    expect(markup).not.toContain('<input type="checkbox"');
    expect(markup).toMatch(/class="kui-toolbar-control-group ticket-search-field"(?=[^>]*data-expanded="false")/);
    expect(enabled).toMatch(/class="kui-toolbar-control-group ticket-search-field"(?=[^>]*data-expanded="true")/);
    expect(markup).not.toContain('workspace-header__search-tokens');
    expect(enabled).toContain('aria-label="Search syntax help"');
    expect(markup).not.toContain('aria-label="Date and time helper"');
    expect(
      String(
        WorkspaceControls({
          mode: 'list',
          searchOpen: true,
          searchModel: searchModelWith('created-after:', ['client']),
        }),
      ),
    ).toContain('aria-label="Date and time helper"');
    expect(markup).not.toContain('aria-label="Search syntax"');
    const enabledWithHelp = String(WorkspaceControls({ mode: 'list', searchOpen: true, searchHelpOpen: true }));
    expect(enabledWithHelp).toContain('aria-label="Search syntax"');
    expect(enabledWithHelp).toContain('<dt>Tags</dt>');
    expect(enabledWithHelp).toContain('<dt>Content</dt>');
    expect(enabledWithHelp).toContain('<dt>Workflow</dt>');
    expect(enabledWithHelp).toContain('<code>is:closed</code>');
    expect(enabledWithHelp).toContain('<code>is:duplicate</code>');
    expect(enabledWithHelp).toContain('<dt>Dates</dt>');
    expect(enabledWithHelp).toContain('<strong>Combine filters</strong>');
    expect(enabledWithHelp).toContain('local, relative, and ISO 8601 dates work');
    expect(enabledWithHelp).toContain('updated-after:2026-09-01T11:05');
    expect(markup).not.toContain('class="workspace-header__search-button"');
    expect(markup).not.toContain('data-action="open-global-search"');
    expect(markup.indexOf('workspace-header__utility-group')).toBeLessThan(markup.indexOf('ticket-search-field'));
    expect(markup).toMatch(
      /workspace-header__utility-group[^>]*data-selected-chrome="outline"[^>]*data-selected-tone="pop"/,
    );
    const beforeOverflow = markup.slice(0, markup.indexOf('data-workspace-overflow="true"'));
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
    // No wrapper element: the search field sizes itself inside whichever Toolbar zone holds it (HS2-EZ1N7Z).
    expect(headerCss).not.toContain('workspace-header__actions');
    expect(headerCss).not.toContain('.kui-toolbar-control-group');
    // The header uses Kerf's inline search sizing without restyling its group (HS2-NZK4KA).
    expect(headerCss).not.toContain('ticket-search-field');
    expect(markup).toMatch(/class="kui-toolbar-control-group ticket-search-field"[^>]*data-content="search"/);
    // Token colors and the helper popovers belong to TicketSearchField, not the header (HS2-N5G6JS).
    expect(headerCss).not.toContain('.kui-token-search {');
    expect(headerCss).not.toContain('search-suggestions');
    expect(headerCss).not.toContain('search-help');
    expect(headerCss).not.toContain('.workspace-header__overflow-group {');
    // More stays beside expanded search and retains the Up Next action (HS2-NZK4KA).
    expect(String(WorkspaceControls({ mode: 'list', searchOpen: true }))).toMatch(
      /class="kui-toolbar-control-group workspace-header__overflow-group"[^>]*data-show-below="1024px"/,
    );
    // The header module owns the Empty Trash text action that the page header places (HS2-T35VN7).
    const emptyTrash = String(EmptyTrashAction());
    expect(emptyTrash).toMatch(/<wa-button[^>]*class="workspace-header__text-action"/);
    expect(emptyTrash).toContain('variant="danger"');
    expect(emptyTrash).toContain('data-action="open-empty-trash"');
    expect(emptyTrash).toContain('class="workspace-header__text-action-label"');
    expect(emptyTrash).toContain('data-lucide="trash-2"');
    expect(emptyTrash).toContain('<span>Empty Trash</span>');
    expect(headerCss).toContainSource('wa-button.workspace-header__text-action::part(base) { width: auto;');
    expect(headerCss).toContainSource(
      '.workspace-header__text-action-label { display: inline-flex; align-items: center;',
    );
    expect(shellCss).not.toContain('.kui-toolbar:has(.ticket-search-field');
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
        searchModel: searchModelWith('tag:client ', ['client']),
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

  it('offers the columns/board view toggle and its overflow entry (HS2-ZYJMDP)', () => {
    const markup = String(WorkspaceHeader({ projectName: 'Hot Sheet 2', mode: 'board' }));
    expect(markup).toContain('data-segment-value="board" data-selected="true" aria-label="Columns view"');
    expect(markup).toContain('data-workspace-overflow-action="set-view-mode" data-view-mode="board"');
    expect(markup.match(/data-action="set-view-mode"/g)).toHaveLength(4);
  });

  it('delegates the collapsed find state to the canonical TokenSearchField trigger', () => {
    const markup = String(
      WorkspaceHeader({
        projectName: 'Hot Sheet 2',
        mode: 'list',
        notificationCount: 7,
        searchHelpOpen: true,
      }),
    );
    expect(markup).toMatch(/class="kui-toolbar-control-group ticket-search-field"(?=[^>]*data-expanded="false")/);
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

  it("leaves the disabled utility buttons to Kerf's ToolbarControlGroup (KF-FTADQT, HS2-0MH5V1)", () => {
    const css = readFileSync(resolve(import.meta.dirname, 'workspace-header.css'), 'utf8');
    expect(css).not.toContain(':disabled');
    expect(css).not.toContain('--kui-toolbar-control-hover-background');
    const empty = String(WorkspaceHeader({ projectName: 'Hot Sheet 2', mode: 'list' }));
    expect(empty).not.toContain('workspace-header__utility-action');
    expect(empty).toMatch(/<button[^>]*disabled[^>]*data-action="open-selected-ticket-actions"/);
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
    expect(markup.match(/data-lucide="star"[^>]*fill="currentColor"/g) ?? []).toHaveLength(state === 'none' ? 0 : 2);
    const utility = markup.slice(
      markup.indexOf('workspace-header__utility-group'),
      markup.indexOf('ticket-search-field'),
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
      /<wa-dropdown-item[^>]*data-workspace-overflow-action="toggle-selected-up-next"[^>]*disabled/,
    );
  });

  it('omits every project control for global shell modes', () => {
    const markup = String(WorkspaceHeader({ projectName: 'Terminals', mode: 'list', controlsVisible: false }));
    expect(markup).toMatch(/^<header class="kui-toolbar workspace-header" data-component="toolbar"/);
    expect(markup).toContain('<div class="kui-toolbar__trailing"></div>');
    expect(markup).not.toContain('toolbar-control-group');
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
    expect(markup).toContain('kui-popup-menu__label">More workspace controls</span>');
    // PopupMenu items carry the app's routing and state as data attributes (HS2-CSRJ9Y).
    expect(markup).toContain(
      'data-workspace-overflow-kind="utility" data-workspace-overflow-action="toggle-selected-up-next" data-workspace-overflow-state="all"',
    );
    expect(markup).toContain(
      'data-workspace-overflow-kind="utility" data-workspace-overflow-action="open-selected-ticket-actions"',
    );
    expect(markup).toContain('data-workspace-overflow-action="set-workspace-sort" data-workspace-sort="priority"');
    expect(markup).not.toContain('data-workspace-sort="status"');
    expect(markup).toContain(
      'data-workspace-overflow-kind="search" data-workspace-overflow-action="open-workspace-search"',
    );
    expect(markup).toContain('data-view-mode="notifications"');
    expect(markup).toContain('Show Notifications (7 pending)');
    const headerCss = readFileSync(resolve(import.meta.dirname, 'workspace-header.css'), 'utf8');
    // Kerf's icon-only Select owns the trigger width and centers it in the single group (HS2-06GDW3,
    // KF-Y3YZBE fixed in 5.0.0-beta.51), so the app sets neither a width nor a centering workaround.
    expect(headerCss).not.toContain('.workspace-header__sort {');
    expect(headerCss).not.toContain('KF-Y3YZBE');
    // The single sort group owns the pill; the Select combobox stays transparent so it does not
    // draw a second, shorter pill that pokes out of the group's rounding (HS2-W3VD53).
    // Kerf's borderless toolbar presentation keeps the combobox transparent inside the pill group.
    expect(headerCss).not.toContain('.workspace-header__sort::part(');
    // The focus ring is a single pill on the group (following its radius), not a mismatched ring on
    // the smaller inner combobox (HS2-M1DF1D).
    // The group's own pill shape (`shape="pill"`) carries the ring radius; no app radius rule.
    expect(markup).toMatch(/workspace-header__sort-group[^>]*data-shape="pill"/);
    expect(headerCss).not.toMatch(/\.workspace-header__sort-group \{[^}]*border-radius/);
    // Kerf paints the pill ring (`focusRing="outline"`); the app draws no ring of its own.
    expect(markup).toMatch(/workspace-header__sort-group[^>]*data-focus-ring="outline"/);
    expect(headerCss).not.toContain('.workspace-header__sort-group:focus-within');
    // The Select hands its focus ring to the group (`focusRingOwner="group"`).
    expect(markup).toMatch(
      /<wa-select[^>]*data-presentation="toolbar-borderless"[^>]*data-caret="false"[^>]*data-selected-presentation="icon-only"[^>]*data-focus-ring-owner="group"[^>]*name="workspace-sort"/,
    );
    expect(headerCss).toContainSource(
      '.workspace-header__sort-group { --kui-select-selected-color: var(--kui-toolbar-control-color); }',
    );
    expect(headerCss).not.toContain('.kui-select__custom-selected');
    // Kerf applies strict Toolbar content-width thresholds; the rail keeps its wrapped groups and
    // does not render the narrow header's overflow menu (HS2-BBG8ZC).
    expect(headerCss).not.toContain('@container kui-toolbar');
    expect(markup).toMatch(/view-mode-switcher"[^>]*data-hide-below="176px"/);
    expect(markup).toMatch(/workspace-header__sort-group"[^>]*data-hide-below="416px"/);
    expect(markup).toMatch(/workspace-header__utility-group"[^>]*data-hide-below="480px"/);
    expect(markup).toMatch(/workspace-header__overflow-group"[^>]*data-show-below="480px"/);
    const railMarkup = String(
      WorkspaceControls({ mode: 'list', presentation: 'rail', sort: 'updated', sortDirection: 'descending' }),
    );
    for (const group of ['view-mode-switcher', 'workspace-header__sort-group', 'workspace-header__utility-group'])
      expect(railMarkup).toContain(`${group} ${group}--rail`);
    expect(railMarkup).not.toContain('workspace-header__overflow-group');
    expect(railMarkup).not.toContain('data-hide-below');
    expect(markup).not.toContain('--rail');
    // The rail fills its own row; the header keeps Kerf's shrinkable inline search (HS2-NZK4KA).
    expect(railMarkup).toMatch(
      /class="kui-toolbar-control-group ticket-search-field"[^>]*data-sizing="fill"[^>]*data-placement="end"/,
    );
    expect(markup).toMatch(/class="kui-toolbar-control-group ticket-search-field"[^>]*data-content="search"/);
    // The rail's view switcher fills its own row through Kerf's `sizing="fill"`.
    expect(railMarkup).toMatch(/view-mode-switcher view-mode-switcher--rail"[^>]*data-sizing="fill"/);
    // Transition matrix for the yield state: closed -> open -> closed again, toolbar and rail. The
    // header's groups always opt into Kerf's yield, which acts only while a sibling is expanded, so
    // the search's own expanded state is the single source of truth.
    const groups = ['view-mode-switcher', 'workspace-header__sort-group', 'workspace-header__utility-group'];
    const yieldsIn = (html: string, group: string) =>
      new RegExp(`class="kui-toolbar-control-group ${group}"[^>]*data-visibility="yield-to-expanded-sibling"`).test(
        html,
      );
    const closedMarkup = String(WorkspaceControls({ mode: 'list', searchOpen: false }));
    const openMarkup = String(WorkspaceControls({ mode: 'list', searchOpen: true }));
    for (const group of groups) {
      expect(yieldsIn(closedMarkup, group)).toBe(true);
      expect(yieldsIn(openMarkup, group)).toBe(true);
    }
    expect(yieldsIn(closedMarkup, 'workspace-header__overflow-group')).toBe(false);
    expect(yieldsIn(openMarkup, 'workspace-header__overflow-group')).toBe(false);
    expect(closedMarkup).toMatch(/class="kui-toolbar-control-group ticket-search-field"(?=[^>]*data-expanded="false")/);
    expect(openMarkup).toMatch(/class="kui-toolbar-control-group ticket-search-field"(?=[^>]*data-expanded="true")/);
    expect(String(WorkspaceControls({ mode: 'list', searchOpen: false }))).toBe(closedMarkup);
    // The rail never yields; it wraps its groups onto rows instead (HS2-K9KWJJ).
    const railOpen = String(WorkspaceControls({ mode: 'list', presentation: 'rail', searchOpen: true }));
    expect(railOpen).not.toContain('yield-to-expanded-sibling');
    expect(railOpen).not.toContain('--yield');
    expect(railOpen).toMatch(/class="kui-toolbar-control-group ticket-search-field"[^>]*data-expanded="true"/);
    expect(headerCss).not.toContainSource('.workspace-header__sort-group { display: none; }');
    // An open search hides its sibling groups through Kerf's yield visibility, never through app
    // modifiers, by reading the search field's rendered state, or by styling it from this stylesheet
    // (HS2-0SARDD, HS2-8FS5BJ, HS2-DAMHD1).
    expect(headerCss).not.toContain('--yield');
    expect(headerCss).not.toContain('view-mode-switcher--rail');
    expect(headerCss).not.toContain('ticket-search-field');
    expect(headerCss).not.toContain('.workspace-header__identity {');
  });
});
