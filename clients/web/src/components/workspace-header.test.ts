import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { applyWorkspaceSortDirection, nextWorkspaceSort, WorkspaceHeader, type WorkspaceSort, type WorkspaceSortDirection, workspaceSortTrigger } from './workspace-header';

describe('WorkspaceHeader', () => {
  it('exposes an accessible selected view mode and optional search field', () => {
    const markup = String(WorkspaceHeader({ projectName: 'Hot Sheet 2', mode: 'settings', searchOpen: true, searchQuery: 'client', sort: 'priority', sortDirection: 'descending' }));
    expect(markup).not.toContain('All Tickets');
    expect(markup).toContain('data-component="toolbar-text" data-size="large">Hot Sheet 2');
    expect(markup).toContain('aria-label="View mode"');
    expect(markup).toContain('data-view-mode="settings" aria-label="Settings view" aria-pressed="true"');
    expect(markup).toContain('name="workspace-search"');
    expect(markup).toContain('value="client"');
    expect(markup).toContain('name="workspace-sort"');
    expect(markup).toContain('aria-label="Sort tickets: Priority, descending"');
    expect(markup).toContain('<wa-option value="priority"');
    expect(markup).toContain('data-lucide="arrow-down"');
    expect(markup).toContain('class="select__custom-selected"><svg data-lucide="arrow-down-wide-narrow"');
    expect(markup).not.toContain('type="checkbox"');
    expect(markup).toMatch(/workspace-header__search-group"[^>]*data-expanded="true"/);
    expect(markup).toContain('slot="start"');
    expect(markup).not.toContain('data-action="open-workspace-search"');
    expect(markup.indexOf('workspace-header__utility-group')).toBeLessThan(markup.indexOf('workspace-header__search'));
    expect(markup.match(/disabled/g)).toHaveLength(4);
  });

  it.each([
    ['updated', 'ascending', 'clock-arrow-up'], ['updated', 'descending', 'clock-arrow-down'],
    ['priority', 'ascending', 'arrow-up-narrow-wide'], ['priority', 'descending', 'arrow-down-wide-narrow'],
    ['title', 'ascending', 'arrow-down-a-z'], ['title', 'descending', 'arrow-up-a-z'],
    ['status', 'ascending', 'list-sort-ascending'], ['status', 'descending', 'list-sort-descending'],
  ] satisfies Array<[WorkspaceSort, WorkspaceSortDirection, string]>)('uses a semantic trigger for %s %s', (sort, direction, iconName) => {
    expect(workspaceSortTrigger(sort, direction).iconName).toBe(iconName);
    const label=sort === 'updated' ? 'Recently updated' : sort[0].toUpperCase()+sort.slice(1);
    expect(String(WorkspaceHeader({ projectName: 'Hot Sheet 2', mode: 'list', sort, sortDirection: direction }))).toContain(`aria-label="Sort tickets: ${label}, ${direction}"`);
  });

  it('retains the selected sort while toggling its direction', () => {
    expect(nextWorkspaceSort('updated', 'descending', 'priority')).toEqual({ sort: 'priority', direction: 'ascending' });
    expect(nextWorkspaceSort('priority', 'ascending', 'priority')).toEqual({ sort: 'priority', direction: 'descending' });
    expect(nextWorkspaceSort('priority', 'descending', 'priority')).toEqual({ sort: 'priority', direction: 'ascending' });
    expect(nextWorkspaceSort('priority', 'descending', 'updated')).toEqual({ sort: 'updated', direction: 'descending' });
    expect(applyWorkspaceSortDirection(-3, 'ascending')).toBe(-3);
    expect(applyWorkspaceSortDirection(-3, 'descending')).toBe(3);
  });

  it('renders the collapsed find state as a single magnifier button', () => {
    const markup = String(WorkspaceHeader({ projectName: 'Hot Sheet 2', mode: 'list', notificationCount: 7 }));
    expect(markup).toMatch(/workspace-header__search-group"[^>]*data-expanded="false"/);
    expect(markup).toContain('data-action="open-workspace-search" aria-label="Search tickets"');
    expect(markup).toContain('aria-label="Notifications view, 7 pending"');
    expect(markup).toContain('class="view-mode-switcher__badge" aria-hidden="true">7</span>');
    expect(markup).not.toContain('name="workspace-search"');
  });

  it('omits every project control for global shell modes', () => {
    const markup = String(WorkspaceHeader({ projectName: 'Terminals', mode: 'list', controlsVisible: false }));
    expect(markup).toContain('data-controls-visible="false"');
    expect(markup).not.toContain('workspace-header__actions');
    expect(markup).not.toContain('Search tickets');
  });

  it('progressively removes lower-priority actions when its owning toolbar narrows', () => {
    const toolbarCss = readFileSync(resolve(import.meta.dirname, 'toolbar.css'), 'utf8');
    const headerCss = readFileSync(resolve(import.meta.dirname, 'workspace-header.css'), 'utf8');
    expect(toolbarCss).toContain('container: toolbar / inline-size');
    expect(headerCss).toContain('.workspace-header__sort { width: 2.75rem; }');
    expect(headerCss).toContain('@container toolbar (max-width: 30rem) { .workspace-header__actions > .workspace-header__utility-group { display: none; } }');
    expect(headerCss).toContain('@container toolbar (max-width: 26rem) { .workspace-header__actions > .workspace-header__sort-group { display: none; } }');
    expect(headerCss).toContain('@container toolbar (max-width: 14rem) { .workspace-header__actions > .workspace-header__search-group { display: none; } }');
    expect(headerCss).toContain('@container toolbar (max-width: 11rem) { .workspace-header__actions > .view-mode-switcher { display: none; } }');
    expect(headerCss).not.toContain('overflow: hidden; } .workspace-header__actions');
  });
});
