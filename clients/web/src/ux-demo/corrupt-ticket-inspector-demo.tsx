import { CatalogExample } from '@kerfjs/ui/catalog';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { CorruptTicket } from '../api';
import { CorruptTicketInspector, type CorruptTicketRecoveryState } from '../components/corrupt-ticket-row';
import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import { DemoEvent } from './components/demo-event';
import type { DemoModule } from './demo-module';

const INVALID_TICKET: CorruptTicket = {
    store: 'local',
    store_path: '/work/demo.hs2',
    path: '/work/demo.hs2/tickets/01/01M1BROKEN.md',
    id: '01M1BROKEN',
    slug: 'HS2-BROKEN',
    error: 'unsupported content follows the bounded Notes section',
  },
  UPGRADE_TICKET: CorruptTicket = {
    store: 'local',
    store_path: '/work/demo.hs2',
    path: '/work/demo.hs2/tickets/02/01M2NEWER.md',
    id: '01M2NEWER',
    slug: 'HS2-NEWER',
    error: 'This ticket uses schema hotsheet/v3, which needs a newer Hot Sheet 2.',
    error_code: 'upgrade_required',
  },
  REMOTE_TICKET: CorruptTicket = { ...INVALID_TICKET, store: 'remote' };

type CorruptDemoExample = 'invalid' | 'upgrade' | 'remote';
const CORRUPT_DEMO_TICKETS: Record<CorruptDemoExample, CorruptTicket> = {
  invalid: INVALID_TICKET,
  upgrade: UPGRADE_TICKET,
  remote: REMOTE_TICKET,
};

/** Per-example fixture recovery state, keyed by the production `data-corrupt-key`. */
export const corruptInspectorRecovery = signal<Readonly<Record<string, CorruptTicketRecoveryState>>>({}),
  corruptInspectorEvent = signal('Reveal and AI repair update this example’s recovery state.');

export function resetCorruptTicketInspectorDemo() {
  corruptInspectorRecovery.value = {};
  corruptInspectorEvent.value = 'Reveal and AI repair update this example’s recovery state.';
}

const recoveryFor = (ticket: CorruptTicket) => corruptInspectorRecovery.value[`${ticket.store}:${ticket.path}`];

export function CorruptTicketInspectorDemo() {
  return (
    <>
      <DemoEvent>{corruptInspectorEvent.value}</DemoEvent>
      <CatalogExample label="Unreadable Ticket" viewport={{ width: 'medium' }}>
        <CorruptTicketInspector ticket={INVALID_TICKET} recovery={recoveryFor(INVALID_TICKET)} hostActionsAvailable />
      </CatalogExample>
      <CatalogExample label="Hot Sheet 2 Update Required" viewport={{ width: 'medium' }}>
        <CorruptTicketInspector ticket={UPGRADE_TICKET} recovery={recoveryFor(UPGRADE_TICKET)} hostActionsAvailable />
      </CatalogExample>
      {/* A remote client cannot reveal the server's file: the host-only control is disabled with a reason (HS2-TYS5SS). */}
      <CatalogExample label="Remote Device" viewport={{ width: 'medium' }}>
        <CorruptTicketInspector
          ticket={REMOTE_TICKET}
          recovery={recoveryFor(REMOTE_TICKET)}
          hostActionsAvailable={false}
        />
      </CatalogExample>
    </>
  );
}

/** Install this file's demo interactions on the catalog root: fixture reveal and repair outcomes. */
function wireCorruptTicketInspectorDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  const settle = (key: string | undefined, state: CorruptTicketRecoveryState, event: string) => {
    if (!key || !Object.values(CORRUPT_DEMO_TICKETS).some((ticket) => `${ticket.store}:${ticket.path}` === key)) return;
    corruptInspectorRecovery.value = { ...corruptInspectorRecovery.value, [key]: state };
    corruptInspectorEvent.value = event;
  };
  demoListeners.add(
    delegate(root, 'click', NAVIGATION_AND_TABS_ACTIONS.revealCorruptTicket.selector, (_event, target) => {
      settle(
        (target as HTMLElement).dataset.corruptKey,
        { message: 'Revealed the ticket file on the host.' },
        'Reveal opened the file location on the Hot Sheet host.',
      );
    }),
  );
  demoListeners.add(
    delegate(root, 'click', NAVIGATION_AND_TABS_ACTIONS.repairCorruptTicket.selector, (_event, target) => {
      settle(
        (target as HTMLElement).dataset.corruptKey,
        { message: 'AI repair queued as a new ticket.' },
        'AI repair queued a repair ticket for this file.',
      );
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The corrupt-ticket recovery panel in host and remote-device states (HS2-6M6VM2). */
export const demoModule: DemoModule = {
  entries: {
    'corrupt-ticket-inspector': {
      catalogExamples: true,
      Demo: CorruptTicketInspectorDemo,
      reset: resetCorruptTicketInspectorDemo,
    },
  },
  wire: wireCorruptTicketInspectorDemo,
};
