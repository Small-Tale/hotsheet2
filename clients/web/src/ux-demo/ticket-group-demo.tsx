import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Checkbox } from '@kerfjs/ui/checkbox';
import { List } from '@kerfjs/ui/list';
import { TextField } from '@kerfjs/ui/text-field';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { wireContextMenuDismissal } from '../components/context-menu-dismissal';
import { TicketGroup } from '../components/ticket-group';
import { TicketGroupContextMenu } from '../components/ticket-group-context-menu';
import { wireTicketGroups } from '../components/ticket-group-interactions';
import { listEdge } from '../components/ticket-list';
import { TicketRow, type TicketRowProps } from '../components/ticket-row';
import { normalizeTicketGroupName, ticketGroupNameProblem } from '../ticket-groups';
import { DemoEvent } from './components/demo-event';
import { selectDemo, selectedId } from './components/demo-state';
import { DEMO_ACTIONS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';
import {
  closeTicketGroupDialogDemo,
  setTicketGroupDialogScenario,
  TICKET_GROUP_DEMO_GROUPS,
  ticketGroupDialogSettings,
} from './ticket-group-dialog-demo';

const INITIAL_NAME = 'Board drag & drop';
const INITIAL_EVENT = 'Toggle the group, or right-click its header for the group actions.';

export const TICKET_GROUP_DEMO_TICKETS: TicketRowProps[] = [
  {
    slug: 'HS2-9DPX3L',
    title: 'Drag tickets between columns keeps order',
    status: 'not_started',
    priority: 'high',
    category: 'bug',
    tags: [],
    categoryIcon: 'bug',
    categoryColor: '#ef4444',
    updatedLabel: 'Recently',
  },
  {
    slug: 'HS2-4KQ2MP',
    title: 'Persist column widths per view',
    status: 'started',
    priority: 'high',
    category: 'feature',
    tags: [],
    upNext: true,
    categoryIcon: 'sparkles',
    categoryColor: '#8b5cf6',
    updatedLabel: 'Recently',
  },
  {
    slug: 'HS2-3WAJ7N',
    title: 'Fix flicker when the board reconnects after a deliberately long outage on a narrow column',
    status: 'started',
    priority: 'default',
    category: 'bug',
    tags: [],
    categoryIcon: 'bug',
    categoryColor: '#ef4444',
    updatedLabel: 'Recently',
  },
];

export const ticketGroupSettings = {
  name: signal(INITIAL_NAME),
  collapsed: signal(false),
  dropTarget: signal(false),
  menu: signal<{ x: number; y: number; group: string } | undefined>(undefined),
  event: signal(INITIAL_EVENT),
};

export function resetTicketGroupDemo(root: ParentNode): void {
  ticketGroupSettings.name.value = INITIAL_NAME;
  ticketGroupSettings.collapsed.value = false;
  ticketGroupSettings.dropTarget.value = false;
  ticketGroupSettings.menu.value = undefined;
  ticketGroupSettings.event.value = INITIAL_EVENT;
  syncSettingsControls(root, 'ticket-group', {
    values: { 'group-name': INITIAL_NAME },
    checked: { 'group-collapsed': false, 'group-drop-target': false },
  });
}

export function TicketGroupDemo() {
  const { name, collapsed, dropTarget, menu } = ticketGroupSettings,
    label = normalizeTicketGroupName(name.value) || INITIAL_NAME;
  return (
    <>
      <CatalogExample
        label="List"
        note="A list group holds its tickets on their own rounded card, and its title starts on the left edge of the ticket type icons."
        viewport={{ width: 'full' }}
      >
        <TicketGroup
          name={label}
          count={TICKET_GROUP_DEMO_TICKETS.length}
          scope="list"
          collapsed={collapsed.value}
          dropTarget={dropTarget.value}
        >
          <List gap="none" rootAttributes={{ role: 'none' }}>
            {TICKET_GROUP_DEMO_TICKETS.map((ticket, index) => (
              <TicketRow
                {...ticket}
                presentation="list"
                listEdge={listEdge(index, TICKET_GROUP_DEMO_TICKETS.length)}
                draggable={false}
              />
            ))}
          </List>
        </TicketGroup>
      </CatalogExample>
      <CatalogExample
        label="Board Column"
        note="A column shows only its own members of the group, as cards, with a per-column count."
        viewport={{ width: 'compact' }}
      >
        <TicketGroup
          name={label}
          count={2}
          scope="started"
          presentation="column"
          collapsed={collapsed.value}
          dropTarget={dropTarget.value}
        >
          {TICKET_GROUP_DEMO_TICKETS.slice(1).map((ticket) => (
            <TicketRow {...ticket} qualifiedId={`column:${ticket.slug}`} presentation="column" draggable={false} />
          ))}
        </TicketGroup>
      </CatalogExample>
      {menu.value && <TicketGroupContextMenu {...menu.value} />}
      <DemoEvent>{ticketGroupSettings.event.value}</DemoEvent>
    </>
  );
}

export function TicketGroupSettings() {
  return (
    <DemoSettingsForm settings="ticket-group">
      <TextField name="group-name" label="Group Name" value={ticketGroupSettings.name.value} />
      <Checkbox name="group-collapsed" checked={ticketGroupSettings.collapsed.value} label="Collapsed" />
      <Checkbox name="group-drop-target" checked={ticketGroupSettings.dropTarget.value} label="Drop target" />
      <Button type="button" action={DEMO_ACTIONS.resetSettings.value}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireTicketGroupDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'input', '[data-settings="ticket-group"] [name="group-name"]', (_event, target) => {
      ticketGroupSettings.name.value = (target as FormControl).value;
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="ticket-group"] [name]', (_event, target) => {
      const control = target as FormControl,
        name = control.getAttribute('name');
      if (name === 'group-collapsed') ticketGroupSettings.collapsed.value = control.checked;
      if (name === 'group-drop-target') ticketGroupSettings.dropTarget.value = control.checked;
    }),
  );
  // The production group header, group menu, and group name dialog handlers, with fixture side
  // effects (HS2-WZ3JGZ, HS2-8CDFKR).
  demoListeners.add(
    wireTicketGroups(root, {
      toggle: (group) => {
        const collapsed = !ticketGroupSettings.collapsed.value;
        ticketGroupSettings.collapsed.value = collapsed;
        ticketGroupSettings.event.value = `“${group}” ${collapsed ? 'collapsed' : 'expanded'}.`;
        syncSettingsControls(root, 'ticket-group', { checked: { 'group-collapsed': collapsed } });
      },
      openMenu: (group, x, y) => {
        ticketGroupSettings.menu.value = { x, y, group };
        ticketGroupSettings.event.value = `Group menu opened for “${group}”.`;
      },
      rename: (group) => {
        ticketGroupSettings.menu.value = undefined;
        ticketGroupSettings.event.value = `Rename requested for “${group}”.`;
        selectDemo('ticket-group-dialog');
        setTicketGroupDialogScenario('rename');
      },
      ungroup: (group) => {
        ticketGroupSettings.menu.value = undefined;
        ticketGroupSettings.event.value = `Ungroup requested for “${group}”.`;
      },
      editName: (name) => {
        ticketGroupDialogSettings.name.value = name;
      },
      submitName: (name) => {
        const renaming = ticketGroupDialogSettings.scenario.value.startsWith('rename');
        if (ticketGroupNameProblem(name, TICKET_GROUP_DEMO_GROUPS, renaming ? INITIAL_NAME : undefined)) return;
        closeTicketGroupDialogDemo(
          `${renaming ? 'Rename to' : 'New group'} “${normalizeTicketGroupName(name)}” requested.`,
        );
      },
      dismissDialog: () => {
        if (selectedId.value === 'ticket-group-dialog') closeTicketGroupDialogDemo('Dismissed; no group changed.');
      },
    }),
  );
  demoListeners.add(
    wireContextMenuDismissal(root, [
      {
        surface: 'ticket',
        isOpen: () => Boolean(ticketGroupSettings.menu.value),
        stateToken: () => ticketGroupSettings.menu.value,
        close: () => {
          ticketGroupSettings.menu.value = undefined;
        },
        hide: true,
      },
    ]),
  );
  return () => {
    demoListeners.dispose();
  };
}
/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'ticket-group': {
      catalogExamples: true,
      Demo: TicketGroupDemo,
      Settings: TicketGroupSettings,
      reset: resetTicketGroupDemo,
    },
  },
  wire: wireTicketGroupDemo,
};
