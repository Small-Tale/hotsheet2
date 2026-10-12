import { delegate, delegateCapture, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { TicketBoard, type TicketColumnProps } from '../components/ticket-board';
import { TicketBoardColumn } from '../components/ticket-board-column';
import { TicketList } from '../components/ticket-list';
import type { TicketRowProps } from '../components/ticket-row';
import {
  selectAllTickets,
  type TicketSelectionIntent,
  type TicketSelectionState,
  updateTicketSelection,
} from '../components/ticket-selection';
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
    <section class="collection-demo" aria-label="TicketList demo">
      <TicketList tickets={collectionTickets.value} label="Example ticket list" />
      <div class="collection-demo__empty-states" aria-label="TicketList empty states">
        <article class="collection-demo__empty-state">
          <h3 class="collection-demo__caption">New project</h3>
          <TicketList tickets={[]} emptyState={{ kind: 'project' }} label="Empty new project" />
        </article>
        <article class="collection-demo__empty-state">
          <h3 class="collection-demo__caption">Empty view</h3>
          <TicketList tickets={[]} emptyState={{ kind: 'view', viewLabel: 'Backlog' }} label="Empty Backlog" />
        </article>
        <article class="collection-demo__empty-state">
          <h3 class="collection-demo__caption">No search results</h3>
          <TicketList tickets={[]} emptyState={{ kind: 'search', query: 'parser' }} label="Empty search" />
        </article>
      </div>
      <p class="component-stage__event" aria-live="polite">
        {collectionEvent.value}
      </p>
      <p class="component-stage__guidance">
        The list gives TicketRow the full content width for fast scanning while preserving the same selection and
        metadata contract used in columns.
      </p>
    </section>
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
    <section class="collection-demo collection-demo--board" aria-label="TicketBoard demo">
      <div class="collection-demo__board-stage">
        <TicketBoard columns={columns} label="Example status board" />
      </div>
      <article class="collection-demo__paged-board">
        <h3 class="collection-demo__caption">Paged layout</h3>
        <p>Phone widths show one column at a time and snap to the nearest column when a swipe is released.</p>
        <div class="collection-demo__board-stage">
          <TicketBoard columns={columns} layout="paged" label="Paged status board" />
        </div>
      </article>
      <article class="collection-demo__empty-board">
        <h3 class="collection-demo__caption">Empty search</h3>
        <div class="collection-demo__board-stage">
          <TicketBoard
            columns={columns.map((column) => ({ ...column, tickets: [] }))}
            emptyState={{ kind: 'search', query: 'parser' }}
            label="Empty search board"
          />
        </div>
      </article>
      <p class="component-stage__event" aria-live="polite">
        {collectionEvent.value}
      </p>
      <p class="component-stage__guidance">
        Each narrow column activates TicketRow’s own card presentation; no TicketCard component or parallel summary
        markup is involved.
      </p>
    </section>
  );
}

export function TicketBoardColumnDemo() {
  const tickets = collectionTickets.value.filter((ticket) => ticket.status === 'started');
  return (
    <section class="collection-demo collection-demo--column" aria-label="TicketBoardColumn demo">
      <div class="collection-demo__column-stage">
        <TicketBoardColumn id="in-progress-demo" title="In progress" tickets={tickets} />
      </div>
      <p class="component-stage__event" aria-live="polite">
        {collectionEvent.value}
      </p>
      <p class="component-stage__guidance">
        The column owns its fixed heading, derived count, independently scrollable ticket region, and shared responsive
        TicketRow composition.
      </p>
    </section>
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

  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.selectTicketRow.selector, (event, target) => {
      if ((event.target as Element).closest('[data-action="toggle-row-up-next"]')) return;
      const row = target as HTMLElement;
      if (usesCollectionState()) {
        const pointer = event as MouseEvent;
        selectCollectionTicket(row.dataset.ticketSlug!, {
          range: pointer.shiftKey,
          toggle: pointer.metaKey || pointer.ctrlKey,
        });
        // Like the workspace grid's rail, a plain click in the TerminalTicketRail demo pushes that ticket's
        // detail onto its NavStack (HS2-FY06N4).
        if (row.closest('.terminal-ticket-rail-demo') && !pointer.shiftKey && !pointer.metaKey && !pointer.ctrlKey) {
          terminalRailDemoTicket.value = row.dataset.ticketSlug;
          recordCollectionEvent(`${row.dataset.ticketSlug} pushed onto the ticket rail`);
        }
        return;
      }
      ticketRowSettings.selected.value = !ticketRowSettings.selected.value;
      ticketRowSettings.event.value = ticketRowSettings.selected.value ? 'Ticket selected' : 'Ticket deselected';
      const selected = root.querySelector<FormControl>('[data-settings="ticket-list-row"] [name="selected"]');
      if (selected) selected.checked = ticketRowSettings.selected.value;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.selectTicketColumn.selector, (event, target) => {
      event.stopImmediatePropagation();
      const column = (target as HTMLElement).closest<HTMLElement>('[data-component="ticket-board-column"]');
      if (!column || !usesCollectionState()) return;
      const slugs = new Set(
        [...column.querySelectorAll<HTMLElement>('[data-ticket-slug]')].map((row) => row.dataset.ticketSlug),
      );
      collectionTickets.value = collectionTickets.value.map((ticket) => ({
        ...ticket,
        selected: slugs.has(ticket.slug),
      }));
      recordCollectionEvent(`${slugs.size} tickets selected`);
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
  demoListeners.add(
    delegate(root, 'keydown', DEMO_ACTIONS.selectTicketRow.selector, (event, target) => {
      const keyboard = event as KeyboardEvent;
      const key = keyboard.key;
      const row = target as HTMLElement;
      if (usesCollectionState() && (keyboard.metaKey || keyboard.ctrlKey) && key.toLowerCase() === 'a') {
        event.preventDefault();
        selectAllCollectionTickets();
        return;
      }
      if (usesCollectionState() && (key === 'ArrowUp' || key === 'ArrowDown')) {
        event.preventDefault();
        const scope = row.closest<HTMLElement>('[data-ticket-selection-root]')!;
        const rows = [...scope.querySelectorAll<HTMLElement>('[data-action="select-ticket-row"]')];
        const index = rows.indexOf(row);
        const next = rows[Math.max(0, Math.min(rows.length - 1, index + (key === 'ArrowDown' ? 1 : -1)))];
        next.focus();
        selectCollectionTicket(next.dataset.ticketSlug!, {
          range: keyboard.shiftKey,
        });
        return;
      }
      if (key !== 'Enter' && key !== ' ') return;
      event.preventDefault();
      if (usesCollectionState())
        selectCollectionTicket(row.dataset.ticketSlug!, {
          range: keyboard.shiftKey,
          toggle: keyboard.metaKey || keyboard.ctrlKey,
        });
      else row.click();
    }),
  );
  demoListeners.add(
    delegate(root, 'contextmenu', DEMO_ACTIONS.selectTicketRow.selector, (event, target) => {
      event.preventDefault();
      const pointer = event as MouseEvent;
      const row = target as HTMLElement;
      if (usesCollectionState()) {
        if (!collectionTickets.value.find((ticket) => ticket.slug === row.dataset.ticketSlug)?.selected)
          selectCollectionTicket(row.dataset.ticketSlug!);
        recordCollectionEvent(`Context menu opened for ${row.dataset.ticketSlug}`);
        contextMenu.value = {
          x: pointer.clientX,
          y: pointer.clientY,
          ticketSlug: row.dataset.ticketSlug,
        };
        return;
      }
      ticketRowSettings.selected.value = true;
      ticketRowSettings.event.value = 'Context menu opened';
      const selected = root.querySelector<FormControl>('[data-settings="ticket-list-row"] [name="selected"]');
      if (selected) selected.checked = true;
      contextMenu.value = {
        x: pointer.clientX,
        y: pointer.clientY,
        ticketSlug: row.dataset.ticketSlug,
      };
    }),
  );
  demoListeners.add(
    delegate(root, 'dblclick', DEMO_ACTIONS.selectTicketRow.selector, (event, target) => {
      if ((event.target as Element).closest('button, input, textarea, select, a, [contenteditable="true"]')) return;
      if (usesCollectionState()) selectCollectionTicket((target as HTMLElement).dataset.ticketSlug!);
      recordCollectionEvent(`Ticket reader opened for ${(target as HTMLElement).dataset.ticketSlug}`);
      openDemoTicketReader(root);
    }),
  );
  demoListeners.add(
    delegate(root, 'click', '[data-context-field]', (event, target) => {
      event.stopPropagation();
      const field = (target as HTMLElement).dataset.contextField as 'category' | 'priority' | 'status';
      const value = (target as HTMLElement).dataset.contextValue!;
      if (usesCollectionState() && contextMenu.value?.ticketSlug) {
        const selected = new Set(
          collectionTickets.value.filter((ticket) => ticket.selected).map((ticket) => ticket.slug),
        );
        if (!selected.size) selected.add(contextMenu.value.ticketSlug);
        collectionTickets.value = collectionTickets.value.map((ticket) => {
          if (!selected.has(ticket.slug)) return ticket;
          if (field === 'category') return { ...ticket, category: value };
          if (field === 'priority') return { ...ticket, priority: value as typeof ticket.priority };
          return { ...ticket, status: value as typeof ticket.status };
        });
        recordCollectionEvent(
          `${field} changed to ${value} for ${selected.size} ticket${selected.size === 1 ? '' : 's'}`,
        );
      }
      contextMenu.value = undefined;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', '[data-context-action]', (_event, target) => {
      const action = (target as HTMLElement).dataset.contextAction!;
      if (usesCollectionState() && contextMenu.value?.ticketSlug) {
        const slug = contextMenu.value.ticketSlug;
        if (action === 'Toggle Up Next') toggleCollectionTicketUpNext(slug);
        if (action === 'Report not working') {
          selectDemo('not-working-dialog');
          notWorkingDemoOpen.value = true;
        }
        recordCollectionEvent(`${action} selected for ${slug}`);
        contextMenu.value = undefined;
        return;
      }
      if (action === 'Toggle Up Next') {
        ticketRowSettings.upNext.value = !ticketRowSettings.upNext.value;
        const control = root.querySelector<FormControl>('[data-settings="ticket-list-row"] [name="up-next"]');
        if (control) control.checked = ticketRowSettings.upNext.value;
      }
      ticketRowSettings.event.value = `${action} selected`;
      contextMenu.value = undefined;
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'ticket-list': { Demo: TicketListDemo },
    'ticket-board': { Demo: TicketBoardDemo },
    'ticket-board-column': { Demo: TicketBoardColumnDemo },
  },
  wire: wireTicketCollectionsDemo,
};
