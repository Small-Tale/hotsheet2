import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { createDevApp } from '../dev-server';
import { demoCatalog, demoKind,demosUsing, findDemo, flattenCatalog,kerfCatalogSections,usesCatalogGeometryOverlay } from './catalog';
import { connectionDetailsAssessment,ConnectionDetailsDialogSettings,connectionDetailsScenario,resetConnectionDetailsDemo } from './connection-details-demo';
import { repositoryDemoScenario, repositoryStatusForScenario, RepositoryStatusPopoverDemo, RepositoryStatusPopoverSettings, resetRepositoryStatusDemo } from './repository-status-demo';
import { resetStatusBadgeDemo, statusBadgeSettings } from './status-badge-demo';
import { resetTagChipDemo, tagChipSettings } from './tag-chip-demo';
import { resetTicketRowDemo, ticketRowSettings } from './ticket-row-demo';

describe('UX demo catalog', () => {
  it('has unique routes and the implemented component set', () => {
    const entries = flattenCatalog(demoCatalog);
    expect(new Set(entries.map(entry => entry.id)).size).toBe(entries.length);
    expect(entries.filter(entry => entry.implemented).map(entry => entry.id)).toEqual(['app-shell', 'project-sidebar', 'project-summary', 'repository-summary', 'repository-status-popover', 'change-evidence-dialog', 'view-navigation', 'command-navigation', 'command-settings-editor', 'drive-control', 'drive-options-menu', 'workspace-header', 'page-header', 'project-tab', 'project-tabs', 'resizable-region', 'connection-state-banner', 'connection-details-dialog', 'settings-navigation', 'notification-navigation', 'quick-ticket-composer', 'ticket-list', 'ticket-row', 'ticket-board', 'ticket-board-column', 'ticket-inspector', 'ticket-inspector-skeleton', 'ticket-info-panel', 'ticket-timeline', 'ticket-code-review', 'ticket-attachments', 'ticket-category-select', 'ticket-priority-select', 'ticket-status-menu', 'status-badge', 'tag-chip', 'ticket-reader', 'markdown-editor', 'attachment-gallery', 'ticket-close-dialog', 'not-working-dialog', 'bulk-ticket-dialog', 'saved-view-dialog', 'ticket-link-choice-dialog', 'project-dialog', 'project-close-dialog', 'conversation-export-dialog', 'command-run-dialog', 'note-composer', 'note-card', 'ai-conversation', 'ai-tool-settings', 'manual-model-dialog', 'permission-request', 'notification-center', 'terminal-drawer', 'terminal-dashboard', 'terminal-operations-sidebar', 'terminal-ticket-rail', 'fixed-aspect-terminal-card', 'terminal-visibility-dialog', 'terminal-rename-dialog', 'app-empty-state', 'content-transition', 'app-tab', 'select', 'toolbar', 'floating-toolbar', 'list-item', 'list-header', 'toolbar-control-group', 'toolbar-text', 'dialog-header', 'value-table', 'pending-attachment-picker', 'hs1-migration-dialog', 'hs1-migration-banner', 'ticket-source-setup-dialog', 'provider-setup-form', 'ticket-sources-settings', 'settings-workspace', 'keyboard-settings', 'trash-settings']);
    expect(findDemo('tag-chip')?.name).toBe('TagChip');
    expect(findDemo('list-item')).toMatchObject({ name: 'ListItem', implemented: true });
    expect(findDemo('list-header')).toMatchObject({ name: 'ListHeader', implemented: true });
    expect(findDemo('menu-item')).toBeUndefined();
    expect(findDemo('menu-header')).toBeUndefined();
    expect(findDemo('ticket-row')?.uses).toEqual(['status-badge', 'tag-chip']);
    expect(demosUsing('tag-chip').map(entry => entry.id)).toEqual(['ticket-row', 'ticket-info-panel']);
    expect(demosUsing('ticket-row').map(entry => entry.id)).toEqual(['ticket-list', 'ticket-board-column']);
    expect(findDemo('ticket-board')?.uses).toEqual(['ticket-board-column']);
    expect(findDemo('workspace-header')?.uses).toEqual(['toolbar-text', 'toolbar-control-group', 'page-header', 'ticket-list', 'ticket-board']);
    expect(demosUsing('toolbar-control-group').map(entry => entry.id)).toEqual(['app-shell', 'workspace-header', 'ticket-inspector', 'terminal-dashboard', 'toolbar', 'floating-toolbar']);
    expect(demosUsing('floating-toolbar').map(entry => entry.id)).toEqual(['app-shell', 'terminal-dashboard']);
    expect(findDemo('project-tabs')?.uses).toEqual(['project-tab']);expect(findDemo('project-tab')?.uses).toEqual(['app-tab']);expect(findDemo('terminal-drawer')?.uses).toEqual(['app-tab','list-item','list-header','ai-conversation']);expect(findDemo('terminal-dashboard')?.uses).toEqual(['fixed-aspect-terminal-card','floating-toolbar','toolbar-control-group','list-item']);expect(findDemo('terminal-visibility-dialog')?.uses).toEqual(['list-item']);
    expect(demosUsing('fixed-aspect-terminal-card').map(entry=>entry.id)).toEqual(['terminal-dashboard']);
    expect(demosUsing('project-tab').map(entry => entry.id)).toEqual(['project-tabs']);
    expect(findDemo('ticket-inspector')?.uses).toEqual(['toolbar', 'toolbar-text', 'toolbar-control-group', 'ticket-info-panel', 'ticket-timeline', 'ticket-code-review', 'ticket-attachments', 'note-card', 'note-composer']);
    expect(findDemo('ticket-info-panel')?.uses).toContain('list-header');
    expect(findDemo('ticket-reader')?.uses).toEqual(['ticket-inspector']);
    expect(findDemo('repository-status-popover')?.uses).toEqual(['dialog-header','value-table','list-item','list-header','ticket-code-review']);
    expect(findDemo('change-evidence-dialog')?.uses).toEqual(['dialog-header','list-item','list-header']);
    expect(findDemo('connection-details-dialog')?.uses).toEqual(['dialog-header','value-table']);
    expect(demosUsing('note-card').map(entry => entry.id)).toEqual(['ticket-inspector', 'ticket-info-panel']);
    expect(entries.flatMap(entry => entry.uses ?? []).every(id => findDemo(id))).toBe(true);
  });

  it('maps nested categories and relationship metadata into the flat kerf catalog contract',()=>{
    const sections=kerfCatalogSections(demoCatalog,{'ticket-row':'2026-09-21T00:00:00Z'}),ticketList=sections.find(section=>section.category==='Ticket workspace · List'),ticketRow=ticketList?.entries.find(entry=>entry.id==='ticket-row');
    expect(ticketList?.entries.map(entry=>entry.id)).toEqual(['quick-ticket-composer','ticket-list','ticket-row']);
    expect(ticketRow?.tags).toEqual(expect.arrayContaining(['Feature floor']));
    expect(ticketRow?.related?.filter(entry=>entry.group==='Uses').map(entry=>entry.id)).toEqual(['status-badge','tag-chip']);
    expect(ticketRow?.related?.filter(entry=>entry.group==='Used by').map(entry=>entry.id)).toEqual(['ticket-list','ticket-board-column']);
    expect(sections.find(section=>section.category==='Ticket inspector · Notes and activity')?.entries.map(entry=>entry.id)).toContain('note-card');
    expect(sections.find(section=>section.category==='Setup and settings')?.entries.find(entry=>entry.id==='welcome-screen')?.tags).toContain('Planned');
  });

  it('shows Kerf geometry for components but not composed layouts',()=>{
    expect(usesCatalogGeometryOverlay('app-shell')).toBe(false);
    expect(usesCatalogGeometryOverlay('tag-chip')).toBe(true);
    expect(usesCatalogGeometryOverlay('unknown-future-component')).toBe(true);
  });

  it('publishes every implemented app-owned catalog surface with the Kerf consumer metadata contract',()=>{
    const extension=JSON.parse(readFileSync(new URL('../../ai/component-catalog-extension.json',import.meta.url),'utf8')) as {schemaVersion:number;package:string;entries:Array<{id:string;name:string;kind:string;purpose:string;useWhen:string[];avoidWhen:string[];publicClasses:string[];publicTokens:string[];geometry:{margin:string;border:string;padding:string;notes?:string[]};documentation:string}>},schema=JSON.parse(readFileSync(new URL('../../node_modules/@kerfjs/ui/ai/component-catalog-extension.schema.json',import.meta.url),'utf8')) as {$defs:{geometryOwner:{enum:string[]}}};
    expect(extension.schemaVersion).toBe(1);expect(extension.package).toBe('hotsheet-web');expect(extension.entries.length).toBeGreaterThan(60);expect(new Set(extension.entries.map(entry=>entry.id)).size).toBe(extension.entries.length);
    const owners=new Set(schema.$defs.geometryOwner.enum),implemented=new Map(flattenCatalog().filter(entry=>entry.implemented).map(entry=>[entry.id,entry]));
    for(const entry of extension.entries){
      expect(implemented.get(entry.id)?.name).toBe(entry.name);expect(entry.purpose).toBe(implemented.get(entry.id)?.description);expect(entry.useWhen.length).toBeGreaterThan(0);expect(entry.avoidWhen.length).toBeGreaterThan(0);expect(Array.isArray(entry.publicClasses)).toBe(true);expect(Array.isArray(entry.publicTokens)).toBe(true);expect(entry.documentation).toBe('docs/ux-components.md');
      for(const owner of [entry.geometry.margin,entry.geometry.border,entry.geometry.padding])expect(owners.has(owner)).toBe(true);
      if(Object.values(entry.geometry).includes('conditional'))expect(entry.geometry.notes?.length).toBeGreaterThan(0);
      expect(demoKind(entry.id)).toBe(entry.kind);
    }
    expect(demoKind('app-shell')).toBe('composition');expect(demoKind('tag-chip')).toBe('component');expect(demoKind('toolbar')).toBe('component');
  });

  // Every production component module must be represented in the UX-demo catalog (by matching
  // catalog id or exported component name) or be explicitly exempt with a reason. This guards the
  // "all app-level components are always included" invariant (HS2-3GE5ZA): a newly added component
  // fails this test until it is either given a catalog entry or classified as a non-catalog helper.
  it('accounts for every production component in the catalog or a documented exemption', () => {
    // Helpers, sub-components, context menus, transient banners, and states rendered only within a
    // demoed parent are not standalone catalog surfaces. Keep each reason accurate.
    const EXEMPT: Record<string, string> = {
      'ai-content-label': 'Inline AI attribution label rendered within AIConversation / NoteCard (both demoed).',
      'app-error': 'Transient top-level error banner, not a standalone catalog surface.',
      'main-shell': 'Typed application configuration boundary around the cataloged AppShell.',
      'notification-inspector': 'Empty notification-mode inspector chrome rendered within the cataloged AppShell.',
      'attachment-context-menu': 'Context menu rendered by the demoed attachment surfaces (inspector/gallery).',
      'corrupt-ticket-row': 'Parse-error row variant rendered within TicketList (demoed).',
      'flow-back-button': 'Shared back affordance rendered inside multi-step dialogs/flows.',
      'lucide-icon-picker': 'Icon-picker sub-control of CommandSettingsEditor (demoed).',
      'markdown-preview': 'Markdown rendering helpers used by MarkdownEditor and NoteCard (demoed).',
      'project-restore-error': 'Project-restore failure state rendered within AppShell (demoed).',
      'project-tab-context-menu': 'Context menu for ProjectTabBar (demoed via ProjectTabBar).',
      'provider-icon': 'Provider glyph helper shown within ListItem and provider rows (demoed).',
      'provider-model-effort-menu': 'Shared Provider/Model/Effort submenu composed into DriveOptionsMenu and AIConversation (demoed).',
      'repository-setup': 'Initialize/remote setup steps rendered within RepositoryStatusPopover (demoed).',
      'reader-overlay-surfaces': 'Internal typed composition boundary for individually cataloged reader, dialog, menu, gallery, and transient surfaces.',
      'server-busy-bars': 'Small server-activity indicator composed into headers/banners.',
      'ticket-duplicate-backlinks': 'Duplicate-of backlink panel rendered within TicketInfoPanel (demoed).',
      'ticket-empty-state': 'Empty-collection state rendered within TicketList (demoed).',
      'ticket-field-conflict': 'Concurrent-edit conflict affordance rendered within the inspector editors (demoed).',
      'ticket-inspector-placeholder': 'No-selection placeholder rendered within the AppShell inspector (demoed).',
      'ticket-inspector-surface': 'State adapter composing the cataloged TicketInspector and its placeholder/corrupt variants.',
      'ticket-notes': 'Notes section composed into TicketInspector / TicketInfoPanel (demoed).',
      'ticket-row-context-menu': 'Context menu for TicketRow (demoed via TicketList/TicketBoard).',
      'ticket-tag-editor': 'Tag-editing helpers composed into TicketInfoPanel (demoed); TagPicker is the standalone entry.',
      'workspace-composition-surfaces': 'Internal typed composition boundary whose constituent production surfaces are cataloged individually.',
    };
    const componentsDir = fileURLToPath(new URL('../components', import.meta.url));
    const files = readdirSync(componentsDir).filter(name => name.endsWith('.tsx') && !name.endsWith('.test.tsx'));
    const catalogIds = new Set(flattenCatalog().map(entry => entry.id));
    const catalogNames = new Set(flattenCatalog().map(entry => entry.name));
    const unaccounted: string[] = [];
    for (const file of files) {
      const base = file.slice(0, -'.tsx'.length);
      if (catalogIds.has(base)) continue;
      const source = readFileSync(`${componentsDir}/${file}`, 'utf8');
      const exports = [...source.matchAll(/export (?:function|const) ([A-Z]\w*)/g)].map(match => match[1]);
      if (exports.some(name => catalogNames.has(name))) continue;
      if (!EXEMPT[base]) unaccounted.push(base);
    }
    expect(unaccounted, 'components neither cataloged nor exempt (add a ux-demo entry or an exemption reason)').toEqual([]);
    // No stale exemptions: every exempt module still exists and is genuinely not otherwise cataloged.
    for (const base of Object.keys(EXEMPT)) {
      expect(files, `exempt component ${base} no longer exists`).toContain(`${base}.tsx`);
      expect(catalogIds.has(base), `exempt component ${base} is now cataloged by id — remove the exemption`).toBe(false);
    }
  });

  it('records the planned ProjectSidebar composition', () => {
    expect(findDemo('project-sidebar')?.uses).toEqual(['toolbar', 'project-summary', 'repository-summary', 'view-navigation', 'command-navigation', 'drive-control', 'drive-options-menu', 'list-item', 'list-header']);
    expect(demosUsing('drive-control').map(entry => entry.id)).toEqual(['project-sidebar','ai-conversation']);
  });

  it('serves UX markup only when development is explicitly enabled', async () => {
    const dev = await createDevApp(true).request('/ux-demo');
    expect(dev.status).toBe(200);
    expect(await dev.text()).toContain('/src/ux-demo/main.tsx');
    expect((await createDevApp(false).request('/ux-demo')).status).toBe(404);
  });

  it('reports dependency-aware modification times only in development', async () => {
    const response = await createDevApp(true).request('/__hotsheet/demo-modified');
    expect(response.status).toBe(200);
    const modified = await response.json() as Record<string, string>;
    expect(Date.parse(modified['app-shell'])).not.toBeNaN();
    expect(Date.parse(modified['ticket-reader'])).not.toBeNaN();
    expect(modified['attachment-list']).toBeUndefined();
    expect((await createDevApp(false).request('/__hotsheet/demo-modified')).status).toBe(404);
  });

  it('resets every canonical TagChip demo setting', () => {
    tagChipSettings.label.value = 'changed';
    tagChipSettings.variant.value = 'danger';
    tagChipSettings.appearance.value = 'accent';
    tagChipSettings.size.value = 'large';
    tagChipSettings.removable.value = false;
    tagChipSettings.pill.value = false;
    tagChipSettings.disabled.value = true;
    tagChipSettings.event.value = 'Changed';
    resetTagChipDemo();
    expect({
      label: tagChipSettings.label.value, variant: tagChipSettings.variant.value,
      appearance: tagChipSettings.appearance.value, size: tagChipSettings.size.value,
      removable: tagChipSettings.removable.value, pill: tagChipSettings.pill.value,
      disabled: tagChipSettings.disabled.value, event: tagChipSettings.event.value,
    }).toEqual({
      label: 'needs-design', variant: 'neutral', appearance: 'filled', size: 'small',
      removable: true, pill: false, disabled: false, event: 'No actions yet',
    });
  });

  it('resets every canonical StatusBadge demo setting', () => {
    statusBadgeSettings.status.value = 'verified';
    statusBadgeSettings.showIcon.value = false;
    statusBadgeSettings.appearance.value = 'plain';
    statusBadgeSettings.compact.value = true;
    resetStatusBadgeDemo();
    expect({ status: statusBadgeSettings.status.value, showIcon: statusBadgeSettings.showIcon.value, appearance: statusBadgeSettings.appearance.value, compact: statusBadgeSettings.compact.value }).toEqual({ status: 'started', showIcon: true, appearance: 'filled', compact: false });
  });

  it('offers and resets every repository-status headline scenario', () => {
    const settings=String(RepositoryStatusPopoverSettings());
    const demo=String(RepositoryStatusPopoverDemo()),style=readFileSync(fileURLToPath(new URL('./style.css',import.meta.url)),'utf8');
    expect(demo).toContain('class="repository-status-demo"');
    expect(style).toMatch(/\.demo-catalog-examples \{ width: 100%; min-width: 0; \}/);
    expect(style).toMatch(/\.repository-status-demo \{ width: 100%; min-width: 0; \}/);
    for(const scenario of ['clean','dirty','ahead','behind','diverged','conflicted','error'])expect(settings).toContain(`value="${scenario}"`);
    expect((['clean','dirty','ahead','behind','diverged','conflicted'] as const).map(scenario=>[scenario,repositoryStatusForScenario(scenario)])).toMatchObject([
      ['clean',{ahead:0,behind:0,conflicted:0,clean:true}],
      ['dirty',{ahead:0,behind:0,conflicted:0,clean:false}],
      ['ahead',{ahead:2,behind:0,conflicted:0}],
      ['behind',{ahead:0,behind:2,conflicted:0}],
      ['diverged',{ahead:2,behind:1,conflicted:0}],
      ['conflicted',{conflicted:1}],
    ]);
    expect(repositoryStatusForScenario('error')).toBeNull();
    repositoryDemoScenario.value='clean';
    resetRepositoryStatusDemo();
    expect(repositoryDemoScenario.value).toBe('conflicted');
  });

  it('offers and resets every server compatibility detail state',()=>{
    const settings=String(ConnectionDetailsDialogSettings()),scenarios=['source-stale','revision-mismatch','server-too-old-safe','server-too-old-manual','client-too-old','unknown'] as const;
    for(const scenario of scenarios)expect(settings).toContain(`value="${scenario}"`);
    expect(scenarios.map(scenario=>connectionDetailsAssessment(scenario).kind)).toEqual(['compatible','compatible','server_too_old','server_too_old','client_too_old','unknown']);
    connectionDetailsScenario.value='unknown';resetConnectionDetailsDemo();expect(connectionDetailsScenario.value).toBe('source-stale');
  });

  it('resets every canonical TicketRow demo setting', () => {
    ticketRowSettings.title.value = 'Changed';
    ticketRowSettings.status.value = 'verified';
    ticketRowSettings.priority.value = 'urgent';
    ticketRowSettings.category.value = 'bug';
    ticketRowSettings.tags.value = 'one';
    ticketRowSettings.upNext.value = false;
    ticketRowSettings.blocked.value = true;
    ticketRowSettings.needsReview.value = true;
    ticketRowSettings.selected.value = true;
    ticketRowSettings.busy.value = false;
    ticketRowSettings.categoryIcon.value = 'bug';
    ticketRowSettings.categoryColor.value = '#ef4444';
    ticketRowSettings.agentName.value = 'Codex';
    ticketRowSettings.updatedLabel.value = 'Now';
    ticketRowSettings.event.value = 'Changed';
    resetTicketRowDemo();
    expect({
      title: ticketRowSettings.title.value, status: ticketRowSettings.status.value,
      priority: ticketRowSettings.priority.value, category: ticketRowSettings.category.value,
      tags: ticketRowSettings.tags.value, upNext: ticketRowSettings.upNext.value,
      blocked: ticketRowSettings.blocked.value, needsReview: ticketRowSettings.needsReview.value,
      selected: ticketRowSettings.selected.value, busy: ticketRowSettings.busy.value,
      categoryIcon: ticketRowSettings.categoryIcon.value, categoryColor: ticketRowSettings.categoryColor.value,
      agentName: ticketRowSettings.agentName.value, updatedLabel: ticketRowSettings.updatedLabel.value,
      event: ticketRowSettings.event.value,
    }).toEqual({
      title: 'Build the first client ticket list', status: 'started', priority: 'high',
      category: 'feature', tags: 'client, ux', upNext: true, selected: false,
      blocked: false, needsReview: false, busy: true, categoryIcon: 'sparkles',
      categoryColor: '#3b82f6', event: 'No actions yet',
      agentName: 'Claude', updatedLabel: '1h ago',
    });
  });
});
