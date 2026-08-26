import { delegate, delegateCapture, mount, signal } from 'kerfjs';
import '@awesome.me/webawesome/dist/styles/webawesome.css';
import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/checkbox/checkbox.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import '@awesome.me/webawesome/dist/components/option/option.js';
import '@awesome.me/webawesome/dist/components/select/select.js';
import '@awesome.me/webawesome/dist/components/tag/tag.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import '@awesome.me/webawesome/dist/components/divider/divider.js';
import './style.css';
import { demosUsing, demoCatalog, findDemo, type DemoCategory, type DemoDefinition } from './catalog';
import { resetStatusBadgeDemo, StatusBadgeDemo, StatusBadgeSettings, statusBadgeSettings } from './status-badge-demo';
import { resetTagChipDemo, TagChipDemo, TagChipSettings, tagChipSettings } from './tag-chip-demo';
import { resetTicketRowDemo, TicketRowDemo, TicketRowSettings, ticketRowSettings } from './ticket-row-demo';

type FormControl = HTMLElement & { checked: boolean; value: string };
const defaultDemo = 'tag-chip';
const fromUrl = () => new URL(location.href).searchParams.get('component') ?? defaultDemo;
const selectedId = signal(findDemo(fromUrl())?.id ?? defaultDemo);
const settingsOpen = signal(false);
const contextMenu = signal<{ x: number; y: number } | undefined>(undefined);

function demoLink(item: DemoDefinition) {
  const selected = item.id === selectedId.value;
  return <li><a href={`/ux-demo?component=${encodeURIComponent(item.id)}`} data-demo-id={item.id} aria-current={selected ? 'page' : undefined}><span>{item.name}</span><small>{item.implemented ? 'Demo' : item.phase.replace('-', ' ')}</small></a></li>;
}

function demoNavigation(category: DemoCategory) {
  return <section class="catalog-group"><h2>{category.name}</h2>{category.demos && <ul>{category.demos.map(demoLink)}</ul>}{category.children?.map(child => <section class="catalog-subgroup"><h3>{child.name}</h3><ul>{child.demos?.map(demoLink)}</ul></section>)}</section>;
}

function demoContent(item: DemoDefinition) {
  if (item.id === 'status-badge') return <StatusBadgeDemo />;
  if (item.id === 'tag-chip') return <TagChipDemo />;
  if (item.id === 'ticket-row') return <TicketRowDemo />;
  return <section class="planned-demo" aria-label={`${item.name} planned demo`}><span>Planned component</span><p>The catalog entry and navigation are ready. Its real component demo will be added in a later slice.</p></section>;
}

function DemoRelationships({ item }: { item: DemoDefinition }) {
  const uses = (item.uses ?? []).map(findDemo).filter((demo): demo is DemoDefinition => Boolean(demo));
  const usedBy = demosUsing(item.id);
  if (uses.length === 0 && usedBy.length === 0) return null;
  const links = (items: DemoDefinition[]) => <ul>{items.map(demoLink)}</ul>;
  return <footer class="demo-relationships" aria-label="Component relationships">
    {uses.length > 0 && <section><h2>Uses</h2>{links(uses)}</section>}
    {usedBy.length > 0 && <section><h2>Used by</h2>{links(usedBy)}</section>}
  </footer>;
}

function TicketContextMenu() {
  const position = contextMenu.value;
  if (!position) return null;
  return <div class="ticket-context-menu" role="menu" aria-label="Ticket actions" style={`left:${position.x}px;top:${position.y}px`}>
    <wa-dropdown-item data-context-action="Open ticket">Open ticket</wa-dropdown-item>
    <wa-divider></wa-divider>
    <wa-dropdown-item data-context-action="Change category">Change category</wa-dropdown-item>
    <wa-dropdown-item data-context-action="Change priority">Change priority</wa-dropdown-item>
    <wa-dropdown-item data-context-action="Change status">Change status</wa-dropdown-item>
    <wa-dropdown-item data-context-action="Toggle Up Next">Toggle Up Next</wa-dropdown-item>
    <wa-divider></wa-divider>
    <wa-dropdown-item data-context-action="Add tag">Add tag</wa-dropdown-item>
    <wa-dropdown-item data-context-action="Duplicate ticket">Duplicate ticket</wa-dropdown-item>
    <wa-dropdown-item data-context-action="Archive ticket">Archive ticket</wa-dropdown-item>
    <wa-dropdown-item data-context-action="Delete ticket" variant="danger">Delete ticket</wa-dropdown-item>
  </div>;
}

function DemoApp() {
  const selected = findDemo(selectedId.value) ?? findDemo(defaultDemo)!;
  return (
    <main class={settingsOpen.value ? 'demo-shell demo-shell--settings-open' : 'demo-shell'}>
      <aside class="demo-master" aria-label="Component catalog">
        <header><p class="eyebrow">Hot Sheet</p><h1>UX components</h1><p>Production components with deterministic development support.</p></header>
        <nav>{demoCatalog.map(demoNavigation)}</nav>
        <DemoRelationships item={selected} />
      </aside>
      <article class="demo-detail">
        <header class="demo-detail__header"><div><p class="eyebrow">{selected.phase.replace('-', ' ')}</p><h1>{selected.name}</h1><p>{selected.description}</p></div></header>
        {demoContent(selected)}
      </article>
      {settingsOpen.value && <aside class="settings-inspector" aria-label={`${selected.name} settings`}>
        <header><div><p class="eyebrow">Demo settings</p><h2>{selected.name}</h2></div></header>
        {selected.id === 'tag-chip' ? <TagChipSettings /> : selected.id === 'status-badge' ? <StatusBadgeSettings /> : selected.id === 'ticket-row' ? <TicketRowSettings /> : <p>This demo has no adjustable settings.</p>}
      </aside>}
      {selected.implemented && <wa-button class="settings-toggle" data-action="toggle-settings" aria-expanded={settingsOpen.value ? 'true' : 'false'}>{settingsOpen.value ? 'Close settings' : 'Settings'}</wa-button>}
      <TicketContextMenu />
    </main>
  );
}

const root = document.querySelector<HTMLElement>('#ux-demo')!;
mount(root, DemoApp);

function selectDemo(id: string, push = true): void {
  if (!findDemo(id)) return;
  selectedId.value = id;
  contextMenu.value = undefined;
  if (push) history.pushState(null, '', `/ux-demo?component=${encodeURIComponent(id)}`);
}

delegate(root, 'click', '[data-demo-id]', (event, target) => { event.preventDefault(); selectDemo((target as HTMLElement).dataset.demoId!); });
delegate(root, 'click', '[data-action="toggle-settings"]', () => { settingsOpen.value = !settingsOpen.value; });
delegate(root, 'click', '[data-action="reset-settings"]', () => {
  if (selectedId.value === 'tag-chip') resetTagChipDemo(root);
  if (selectedId.value === 'status-badge') resetStatusBadgeDemo(root);
  if (selectedId.value === 'ticket-row') resetTicketRowDemo(root);
});
delegate(root, 'input', '[data-settings="tag-chip"] [name="label"]', (_event, target) => { tagChipSettings.label.value = (target as FormControl).value; });
delegate(root, 'change', '[data-settings="tag-chip"] [name]', (_event, target) => {
  const control = target as FormControl;
  switch (control.getAttribute('name')) {
    case 'variant': tagChipSettings.variant.value = control.value as typeof tagChipSettings.variant.value; break;
    case 'appearance': tagChipSettings.appearance.value = control.value as typeof tagChipSettings.appearance.value; break;
    case 'size': tagChipSettings.size.value = control.value as typeof tagChipSettings.size.value; break;
    case 'removable': tagChipSettings.removable.value = control.checked; break;
    case 'pill': tagChipSettings.pill.value = control.checked; break;
    case 'disabled': tagChipSettings.disabled.value = control.checked; break;
  }
});
delegate(root, 'wa-remove', '[data-component="tag-chip"]', (_event, target) => {
  if ((target as HTMLElement).dataset.disabled !== 'true') tagChipSettings.event.value = `Remove requested for ${(target as HTMLElement).dataset.tagId}`;
});
delegate(root, 'change', '[data-settings="status-badge"] [name]', (_event, target) => {
  const control = target as FormControl;
  if (control.getAttribute('name') === 'status') statusBadgeSettings.status.value = control.value as typeof statusBadgeSettings.status.value;
  if (control.getAttribute('name') === 'show-icon') statusBadgeSettings.showIcon.value = control.checked;
});
delegate(root, 'input', '[data-settings="ticket-list-row"] wa-input', (_event, target) => {
  const control = target as FormControl;
  if (control.getAttribute('name') === 'title') ticketRowSettings.title.value = control.value;
  if (control.getAttribute('name') === 'category') ticketRowSettings.category.value = control.value;
  if (control.getAttribute('name') === 'tags') ticketRowSettings.tags.value = control.value;
  if (control.getAttribute('name') === 'agent') ticketRowSettings.agentName.value = control.value;
  if (control.getAttribute('name') === 'updated') ticketRowSettings.updatedLabel.value = control.value;
});
delegate(root, 'change', '[data-settings="ticket-list-row"] [name]', (_event, target) => {
  const control = target as FormControl;
  switch (control.getAttribute('name')) {
    case 'status': ticketRowSettings.status.value = control.value as typeof ticketRowSettings.status.value; break;
    case 'priority': ticketRowSettings.priority.value = control.value as typeof ticketRowSettings.priority.value; break;
    case 'category-icon': ticketRowSettings.categoryIcon.value = control.value; break;
    case 'category-color': ticketRowSettings.categoryColor.value = control.value; break;
    case 'up-next': ticketRowSettings.upNext.value = control.checked; break;
    case 'blocked': ticketRowSettings.blocked.value = control.checked; break;
    case 'needs-review': ticketRowSettings.needsReview.value = control.checked; break;
    case 'selected': ticketRowSettings.selected.value = control.checked; break;
    case 'busy': ticketRowSettings.busy.value = control.checked; break;
  }
});
delegate(root, 'click', '[data-action="select-ticket-row"]', (event) => {
  if ((event.target as Element).closest('[data-action="toggle-row-up-next"]')) return;
  ticketRowSettings.selected.value = !ticketRowSettings.selected.value;
  ticketRowSettings.event.value = ticketRowSettings.selected.value ? 'Ticket selected' : 'Ticket deselected';
  const selected = root.querySelector('[data-settings="ticket-list-row"] [name="selected"]') as FormControl | null;
  if (selected) selected.checked = ticketRowSettings.selected.value;
});
function toggleRowUpNext(): void {
  ticketRowSettings.upNext.value = !ticketRowSettings.upNext.value;
  ticketRowSettings.event.value = ticketRowSettings.upNext.value ? 'Added to Up Next' : 'Removed from Up Next';
  const control = root.querySelector('[data-settings="ticket-list-row"] [name="up-next"]') as FormControl | null;
  if (control) control.checked = ticketRowSettings.upNext.value;
}
delegateCapture(root, 'click', '[data-action="toggle-row-up-next"]', (event) => {
  event.stopPropagation();
  toggleRowUpNext();
});
delegateCapture(root, 'keydown', '[data-action="toggle-row-up-next"]', (event) => {
  const key = (event as KeyboardEvent).key;
  if (key !== 'Enter' && key !== ' ') return;
  event.preventDefault();
  event.stopPropagation();
  toggleRowUpNext();
});
delegate(root, 'keydown', '[data-action="select-ticket-row"]', (event, target) => {
  const key = (event as KeyboardEvent).key;
  if (key !== 'Enter' && key !== ' ') return;
  event.preventDefault();
  (target as HTMLElement).click();
});
delegate(root, 'contextmenu', '[data-action="select-ticket-row"]', (event) => {
  event.preventDefault();
  const pointer = event as MouseEvent;
  ticketRowSettings.selected.value = true;
  ticketRowSettings.event.value = 'Context menu opened';
  const selected = root.querySelector('[data-settings="ticket-list-row"] [name="selected"]') as FormControl | null;
  if (selected) selected.checked = true;
  contextMenu.value = { x: pointer.clientX, y: pointer.clientY };
});
delegate(root, 'click', '[data-context-action]', (_event, target) => {
  const action = (target as HTMLElement).dataset.contextAction!;
  if (action === 'Toggle Up Next') {
    ticketRowSettings.upNext.value = !ticketRowSettings.upNext.value;
    const control = root.querySelector('[data-settings="ticket-list-row"] [name="up-next"]') as FormControl | null;
    if (control) control.checked = ticketRowSettings.upNext.value;
  }
  ticketRowSettings.event.value = `${action} selected`;
  contextMenu.value = undefined;
});
addEventListener('pointerdown', event => {
  if (contextMenu.value && !(event.target as Element).closest('.ticket-context-menu')) contextMenu.value = undefined;
});
addEventListener('keydown', event => { if (event.key === 'Escape') contextMenu.value = undefined; });
addEventListener('popstate', () => selectDemo(fromUrl(), false));
