import { CatalogExample } from '@kerfjs/ui/catalog';
import { delegateCapture, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { TicketBoard, type TicketColumnProps } from '../components/ticket-board';
import { TicketBoardColumn } from '../components/ticket-board-column';
import { wireTicketCollection } from '../components/ticket-collection-interactions';
import { TicketList } from '../components/ticket-list';
import type { TicketRowProps } from '../components/ticket-row';
import {
  selectAllTickets,
  type TicketSelectionIntent,
  type TicketSelectionState,
  updateTicketSelection,
} from '../components/ticket-selection';
import { DemoEvent } from './components/demo-event';
import { contextMenu, selectDemo, selectedId } from './components/demo-state';
import { openDemoTicketReader } from './content-components-demo';
import { DEMO_ACTIONS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { notWorkingDemoOpen } from './not-working-dialog-demo';
import { ticketRowSettings } from './ticket-row-demo';
import { terminalRailDemoTicket } from './workspace-components-demo';

const initialTickets: TicketRowProps[] = [
  {
    slug: 'HS2-R76MMW',
    title: 'Build TicketList and TicketBoard around shared responsive TicketRow',
    status: 'started',
    priority: 'high',
    category: 'feature',
    tags: ['client', 'ux'],
    upNext: true,
    busy: true,
    feedbackNeeded: true,
    categoryIcon: 'sparkles',
    categoryColor: '#3b82f6',
    agentName: 'Codex',
    updatedLabel: 'Now',
  },
  {
    slug: 'HS2-K00QPZ',
    title: 'Expose StatusBadge plain appearance as a supported demo variant',
    status: 'completed',
    priority: 'default',
    category: 'bug',
    tags: ['client'],
    needsReview: true,
    categoryIcon: 'bug',
    categoryColor: '#ef4444',
    agentName: 'Claude',
    updatedLabel: '8m ago',
    latestConfidence: 55,
  },
  {
    slug: 'HS2-RPVFA4',
    title: 'Add a repository-status snapshot and server endpoint',
    status: 'verified',
    priority: 'default',
    category: 'task',
    tags: ['core', 'server'],
    categoryColor: '#14b8a6',
    agentName: 'AI',
    updatedLabel: '2h ago',
    latestConfidence: 94,
  },
  {
    slug: 'HS2-JN3X4W',
    title: 'Define safe command execution, streaming, cancellation, and history contracts',
    status: 'started',
    priority: 'urgent',
    category: 'feature',
    tags: ['server', 'commands', 'security'],
    blocked: true,
    categoryIcon: 'terminal',
    categoryColor: '#8b5cf6',
    agentName: 'Claude',
    updatedLabel: '4h ago',
  },
  {
    slug: 'HS2-SG1BKJ',
    title:
      'Explore a deliberately long ticket title to verify that every metadata item remains readable in a narrow board column',
    status: 'backlog',
    priority: 'low',
    category: 'investigation',
    tags: ['client', 'responsive', 'long-tag-example'],
    categoryIcon: 'search',
    categoryColor: '#f59e0b',
    agentName: 'AI',
    updatedLabel: '1d ago',
  },
  {
    slug: 'HS2-DEMO01',
    title: 'Plan keyboard movement between board columns',
    status: 'backlog',
    priority: 'default',
    category: 'task',
    tags: ['client', 'board'],
    categoryColor: '#14b8a6',
    agentName: 'AI',
    updatedLabel: '1d ago',
  },
  {
    slug: 'HS2-DEMO02',
    title: 'Document empty-state behavior for saved views',
    status: 'not_started',
    priority: 'low',
    category: 'task',
    tags: ['docs', 'views'],
    categoryColor: '#14b8a6',
    agentName: 'Claude',
    updatedLabel: '2d ago',
  },
  {
    slug: 'HS2-DEMO03',
    title: 'Review provider capability messaging',
    status: 'backlog',
    priority: 'high',
    category: 'feature',
    tags: ['providers', 'ux'],
    categoryColor: '#8b5cf6',
    agentName: 'AI',
    updatedLabel: '2d ago',
  },
  {
    slug: 'HS2-DEMO04',
    title: 'Explore compact attachment summaries',
    status: 'not_started',
    priority: 'default',
    category: 'investigation',
    tags: ['attachments', 'client'],
    categoryColor: '#f59e0b',
    agentName: 'Claude',
    updatedLabel: '3d ago',
  },
  {
    slug: 'HS2-DEMO05',
    title: 'Specify cross-project drag feedback',
    status: 'backlog',
    priority: 'urgent',
    category: 'feature',
    tags: ['drag', 'projects'],
    categoryColor: '#8b5cf6',
    agentName: 'AI',
    updatedLabel: '3d ago',
  },
  {
    slug: 'HS2-DEMO06',
    title: 'Implement saved filter chip removal',
    status: 'started',
    priority: 'high',
    category: 'feature',
    tags: ['filters', 'client'],
    categoryColor: '#8b5cf6',
    agentName: 'Codex',
    updatedLabel: '12m ago',
  },
  {
    slug: 'HS2-DEMO07',
    title: 'Refine command execution progress states',
    status: 'started',
    priority: 'default',
    category: 'task',
    tags: ['commands', 'ux'],
    categoryColor: '#14b8a6',
    agentName: 'Claude',
    updatedLabel: '24m ago',
  },
  {
    slug: 'HS2-DEMO08',
    title: 'Add connection recovery affordances',
    status: 'started',
    priority: 'urgent',
    category: 'bug',
    tags: ['server', 'client'],
    categoryColor: '#ef4444',
    agentName: 'Codex',
    updatedLabel: '31m ago',
  },
  {
    slug: 'HS2-DEMO09',
    title: 'Verify responsive inspector transitions',
    status: 'started',
    priority: 'default',
    category: 'task',
    tags: ['inspector', 'responsive'],
    categoryColor: '#14b8a6',
    agentName: 'AI',
    updatedLabel: '48m ago',
  },
  {
    slug: 'HS2-DEMO10',
    title: 'Polish project switching focus restoration',
    status: 'started',
    priority: 'low',
    category: 'bug',
    tags: ['projects', 'focus'],
    categoryColor: '#ef4444',
    agentName: 'Claude',
    updatedLabel: '1h ago',
  },
  {
    slug: 'HS2-DEMO11',
    title: 'Ship accessible metadata popup controls',
    status: 'completed',
    priority: 'high',
    category: 'feature',
    tags: ['metadata', 'a11y'],
    categoryColor: '#8b5cf6',
    agentName: 'Codex',
    updatedLabel: '2h ago',
  },
  {
    slug: 'HS2-DEMO12',
    title: 'Validate repository status summaries',
    status: 'verified',
    priority: 'default',
    category: 'task',
    tags: ['repository', 'server'],
    categoryColor: '#14b8a6',
    agentName: 'AI',
    updatedLabel: '3h ago',
  },
  {
    slug: 'HS2-DEMO13',
    title: 'Complete context menu icon audit',
    status: 'completed',
    priority: 'default',
    category: 'task',
    tags: ['menus', 'icons'],
    categoryColor: '#14b8a6',
    agentName: 'Claude',
    updatedLabel: '4h ago',
  },
  {
    slug: 'HS2-DEMO14',
    title: 'Verify toolbar geometry across viewports',
    status: 'verified',
    priority: 'low',
    category: 'investigation',
    tags: ['toolbar', 'visual'],
    categoryColor: '#f59e0b',
    agentName: 'AI',
    updatedLabel: '5h ago',
  },
  {
    slug: 'HS2-DEMO15',
    title: 'Publish the component coverage matrix',
    status: 'completed',
    priority: 'default',
    category: 'task',
    tags: ['testing', 'docs'],
    categoryColor: '#14b8a6',
    agentName: 'Codex',
    updatedLabel: '6h ago',
  },
];

export const collectionTickets = signal(initialTickets.map((ticket) => ({ ...ticket, tags: [...ticket.tags] })));
export const collectionEvent = signal('Select a ticket or toggle its Up Next star.');
let collectionSelection: TicketSelectionState = { selected: new Set<string>() };

export function resetTicketCollections(): void {
  collectionTickets.value = initialTickets.map((ticket) => ({ ...ticket, tags: [...ticket.tags] }));
  collectionSelection = { selected: new Set() };
  collectionEvent.value = 'Select a ticket or toggle its Up Next star.';
}

export function selectCollectionTicket(slug: string, intent: TicketSelectionIntent = {}): void {
  collectionSelection = updateTicketSelection(
    collectionTickets.value.map((ticket) => ticket.slug),
    collectionSelection,
    slug,
    intent,
  );
  collectionTickets.value = collectionTickets.value.map((ticket) => ({
    ...ticket,
    selected: collectionSelection.selected.has(ticket.slug),
  }));
  collectionEvent.value = `${collectionSelection.selected.size} ticket${collectionSelection.selected.size === 1 ? '' : 's'} selected`;
}

export function selectAllCollectionTickets(): void {
  collectionSelection = selectAllTickets(collectionTickets.value.map((ticket) => ticket.slug));
  collectionTickets.value = collectionTickets.value.map((ticket) => ({ ...ticket, selected: true }));
  collectionEvent.value = `${collectionSelection.selected.size} tickets selected`;
}

export function recordCollectionEvent(message: string): void {
  collectionEvent.value = message;
}

export function toggleCollectionTicketUpNext(slug: string): void {
  collectionTickets.value = collectionTickets.value.map((ticket) =>
    ticket.slug === slug ? { ...ticket, upNext: !ticket.upNext } : ticket,
  );
  const upNext = collectionTickets.value.find((ticket) => ticket.slug === slug)?.upNext;
  collectionEvent.value = `${slug} ${upNext ? 'added to' : 'removed from'} Up Next`;
}

export function TicketListDemo() {
  return (
    <>
      <CatalogExample
        label="Interactive"
        note="The list gives TicketRow the full content width for fast scanning while preserving the same selection and metadata contract used in columns."
        viewport={{ width: 'full' }}
      >
        <TicketList tickets={collectionTickets.value} label="Example ticket list" />
      </CatalogExample>
      <CatalogExample label="Empty state: new project" viewport={{ width: 'full' }}>
        <TicketList tickets={[]} emptyState={{ kind: 'project' }} label="Empty new project" />
      </CatalogExample>
      <CatalogExample label="Empty state: empty view" viewport={{ width: 'full' }}>
        <TicketList tickets={[]} emptyState={{ kind: 'view', viewLabel: 'Backlog' }} label="Empty Backlog" />
      </CatalogExample>
      <CatalogExample label="Empty state: no search results" viewport={{ width: 'full' }}>
        <TicketList tickets={[]} emptyState={{ kind: 'search', query: 'parser' }} label="Empty search" />
      </CatalogExample>
      <DemoEvent>{collectionEvent.value}</DemoEvent>
    </>
  );
}

export function TicketBoardDemo() {
  const tickets = collectionTickets.value;
  const columns: TicketColumnProps[] = [
    {
      id: 'backlog',
      title: 'Backlog',
      tickets: tickets.filter((ticket) => ticket.status === 'backlog' || ticket.status === 'not_started'),
    },
    { id: 'in-progress', title: 'In progress', tickets: tickets.filter((ticket) => ticket.status === 'started') },
    {
      id: 'done',
      title: 'Done',
      tickets: tickets.filter((ticket) => ticket.status === 'completed' || ticket.status === 'verified'),
    },
  ];
  return (
    <>
      <CatalogExample
        label="Interactive"
        note="Each narrow column activates TicketRow’s own card presentation; no TicketCard component or parallel summary markup is involved."
        viewport={{ width: 'full', height: 'app' }}
      >
        <TicketBoard columns={columns} label="Example status board" />
      </CatalogExample>
      <CatalogExample
        label="Paged layout"
        note="Phone widths show one column at a time and snap to the nearest column when a swipe is released."
        viewport={{ width: 'compact', height: 'app' }}
      >
        <TicketBoard columns={columns} layout="paged" label="Paged status board" />
      </CatalogExample>
      <CatalogExample label="Empty search" viewport={{ width: 'full', height: 'tall' }}>
        <TicketBoard
          columns={columns.map((column) => ({ ...column, tickets: [] }))}
          emptyState={{ kind: 'search', query: 'parser' }}
          label="Empty search board"
        />
      </CatalogExample>
      <DemoEvent>{collectionEvent.value}</DemoEvent>
    </>
  );
}

export function TicketBoardColumnDemo() {
  const tickets = collectionTickets.value.filter((ticket) => ticket.status === 'started');
  return (
    <>
      <CatalogExample
        label="Interactive"
        note="The column owns its fixed heading, derived count, independently scrollable ticket region, and shared responsive TicketRow composition."
        viewport={{ width: 'compact', height: 'app' }}
      >
        <TicketBoardColumn id="in-progress-demo" title="In progress" tickets={tickets} />
      </CatalogExample>
      <DemoEvent>{collectionEvent.value}</DemoEvent>
    </>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireTicketCollectionsDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  const usesCollectionState = () =>
    [
      'ticket-list',
      'ticket-board',
      'workspace-header',
      'terminal-ticket-rail',
      'quick-ticket-composer',
      'app-shell',
    ].includes(selectedId.value);
  const rowSelector = DEMO_ACTIONS.selectTicketRow.selector;
  /** The lone TicketRow demo keeps its selection in the row settings form. */
  function setRowDemoSelected(selected: boolean, message: string) {
    ticketRowSettings.selected.value = selected;
    ticketRowSettings.event.value = message;
    const control = root.querySelector<FormControl>('[data-settings="ticket-list-row"] [name="selected"]');
    if (control) control.checked = selected;
  }

  // The production TicketList/TicketBoard handlers, with fixture side effects (HS2-RWY78R).
  demoListeners.add(
    wireTicketCollection(root, {
      selectedSlugs: () => {
        if (usesCollectionState())
          return collectionTickets.value.filter((ticket) => ticket.selected).map((ticket) => ticket.slug);
        const slug = root.querySelector<HTMLElement>(rowSelector)?.dataset.ticketSlug;
        return ticketRowSettings.selected.value && slug ? [slug] : [];
      },
      selectionOrder: (row) =>
        [...(row.closest('[data-ticket-selection-root]') ?? root).querySelectorAll<HTMLElement>(rowSelector)].map(
          (item) => item.dataset.ticketSlug!,
        ),
      select: (slug, intent, _ordered, row, event) => {
        if (usesCollectionState()) {
          selectCollectionTicket(slug, intent);
          // Like the workspace grid's rail, a plain click in the TerminalTicketRail demo pushes that ticket's
          // detail onto its NavStack (HS2-FY06N4).
          if (event.type === 'click' && row.closest('.terminal-ticket-rail-demo') && !intent.range && !intent.toggle) {
            terminalRailDemoTicket.value = slug;
            recordCollectionEvent(`${slug} pushed onto the ticket rail`);
          }
          return;
        }
        if (event.type === 'contextmenu') setRowDemoSelected(true, 'Ticket selected');
        else if (!ticketRowSettings.selected.value) setRowDemoSelected(true, 'Ticket selected');
        else setRowDemoSelected(false, 'Ticket deselected');
      },
      isSelectAll: (keyboard) =>
        usesCollectionState() && (keyboard.metaKey || keyboard.ctrlKey) && keyboard.key.toLowerCase() === 'a',
      selectAll: () => {
        selectAllCollectionTickets();
      },
      selectColumn: (slugs) => {
        if (!usesCollectionState()) return;
        const chosen = new Set(slugs);
        collectionTickets.value = collectionTickets.value.map((ticket) => ({
          ...ticket,
          selected: chosen.has(ticket.slug),
        }));
        recordCollectionEvent(`${chosen.size} tickets selected`);
      },
      openReader: (slug) => {
        if (usesCollectionState()) selectCollectionTicket(slug);
        recordCollectionEvent(`Ticket reader opened for ${slug}`);
        openDemoTicketReader(root);
      },
      contextMenu: () => {
        const menu = contextMenu.value;
        return menu?.ticketSlug ? { x: menu.x, y: menu.y, ticketSlug: menu.ticketSlug } : undefined;
      },
      setContextMenu: (menu) => {
        contextMenu.value = menu;
        if (!menu) return;
        if (usesCollectionState()) recordCollectionEvent(`Context menu opened for ${menu.ticketSlug}`);
        else ticketRowSettings.event.value = 'Context menu opened';
      },
      onContextField: (field, value, slugs) => {
        if (!usesCollectionState()) return;
        const chosen = new Set(slugs);
        collectionTickets.value = collectionTickets.value.map((ticket) => {
          if (!chosen.has(ticket.slug)) return ticket;
          if (field === 'category') return { ...ticket, category: value };
          if (field === 'priority') return { ...ticket, priority: value as typeof ticket.priority };
          return { ...ticket, status: value as typeof ticket.status };
        });
        recordCollectionEvent(`${field} changed to ${value} for ${chosen.size} ticket${chosen.size === 1 ? '' : 's'}`);
      },
      onContextAction: (action, _slugs, menuSlug) => {
        if (usesCollectionState()) {
          if (action === 'Toggle Up Next') toggleCollectionTicketUpNext(menuSlug);
          if (action === 'Report not working') {
            selectDemo('not-working-dialog');
            notWorkingDemoOpen.value = true;
          }
          recordCollectionEvent(`${action} selected for ${menuSlug}`);
          return;
        }
        if (action === 'Toggle Up Next') {
          ticketRowSettings.upNext.value = !ticketRowSettings.upNext.value;
          const control = root.querySelector<FormControl>('[data-settings="ticket-list-row"] [name="up-next"]');
          if (control) control.checked = ticketRowSettings.upNext.value;
        }
        ticketRowSettings.event.value = `${action} selected`;
      },
    }),
  );
  function toggleRowUpNext(target?: Element): void {
    if (usesCollectionState()) {
      const row = target?.closest('[data-component="ticket-list-row"]') as HTMLElement | null;
      if (row) toggleCollectionTicketUpNext(row.dataset.ticketSlug!);
      return;
    }
    ticketRowSettings.upNext.value = !ticketRowSettings.upNext.value;
    ticketRowSettings.event.value = ticketRowSettings.upNext.value ? 'Added to Up Next' : 'Removed from Up Next';
    const control = root.querySelector<FormControl>('[data-settings="ticket-list-row"] [name="up-next"]');
    if (control) control.checked = ticketRowSettings.upNext.value;
  }
  demoListeners.add(
    delegateCapture(root, 'click', DEMO_ACTIONS.toggleRowUpNext.selector, (event) => {
      event.stopPropagation();
      toggleRowUpNext(event.target as Element);
    }),
  );
  demoListeners.add(
    delegateCapture(root, 'keydown', DEMO_ACTIONS.toggleRowUpNext.selector, (event) => {
      const key = (event as KeyboardEvent).key;
      if (key !== 'Enter' && key !== ' ') return;
      event.preventDefault();
      event.stopPropagation();
      toggleRowUpNext(event.target as Element);
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}
/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'ticket-list': { catalogExamples: true, Demo: TicketListDemo },
    'ticket-board': { catalogExamples: true, Demo: TicketBoardDemo },
    'ticket-board-column': { catalogExamples: true, Demo: TicketBoardColumnDemo },
  },
  wire: wireTicketCollectionsDemo,
};
