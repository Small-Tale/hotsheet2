import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Row } from '@kerfjs/ui/row';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { DesktopRemoteDialog } from '../components/desktop-remote-dialog';
import { ProjectSourceChoiceDialog } from '../components/project-source-choice-dialog';
import type { DesktopRemoteDiscovery, DesktopRemotePairing, DesktopRemoteServer } from '../remote-projects/client';
import { DemoEvent } from './components/demo-event';
import type { DemoModule } from './demo-module';

/** Synthetic fixtures only; no real host, address, or credential (HS2-BT4KN6). */
const servers: DesktopRemoteServer[] = [
  {
    id: 'studio',
    label: 'Studio Mac',
    origin: 'http://studio.example.test:4174',
    identity: { server_id: 'srv-studio-demo', ca_fingerprint: null },
    revision: 2,
    revoked: false,
    last_failure: null,
    stale: false,
  },
  {
    id: 'build',
    label: 'Build Mac',
    origin: 'https://build.example.test:4443',
    identity: { server_id: 'srv-build-demo', ca_fingerprint: 'sha256:demo' },
    revision: 1,
    revoked: false,
    last_failure: 'The server did not answer.',
    stale: false,
  },
];

const discovery: DesktopRemoteDiscovery = {
  remote: servers[0],
  checkouts: [
    { id: 'demo-a1b2c3', name: 'demo', state: 'available' },
    { id: 'website-d4e5f6', name: 'website', state: 'available' },
    { id: 'archive-0a0b0c', name: 'archive', state: 'inaccessible' },
  ],
};

const pairing: DesktopRemotePairing = {
  handle: 'demo-handle',
  approvalId: 'K7Q2',
  serverId: 'srv-studio-demo',
  origin: 'http://studio.example.test:4174',
  label: 'Demo Laptop',
};

const variants = {
  choice: 'Local or remote',
  servers: 'Approved servers',
  form: 'Add server',
  pairing: 'Awaiting approval',
  projects: 'Remote projects',
  error: 'Connection error',
} as const;
type DesktopRemoteDemoVariant = keyof typeof variants;

const variant = signal<DesktopRemoteDemoVariant>('choice'),
  open = signal(true),
  result = signal('');

/** Restore the initial demo state. */
export function resetDesktopRemoteDialogDemo(): void {
  variant.value = 'choice';
  open.value = true;
  result.value = '';
}

export function DesktopRemoteDialogDemo() {
  const remote = variant.value !== 'choice';
  const formOpen = variant.value === 'form' || variant.value === 'pairing';
  return (
    <CatalogExample
      label="Interactive"
      align="inline-control"
      note="Each button opens one desktop project-source or remote-server state."
      rootAttributes={{ 'data-desktop-remote-dialog-demo': '' }}
    >
      <Row gap="xs" wrap>
        {Object.entries(variants).map(([id, label]) => (
          <Button action="show-desktop-remote-dialog-demo" rootAttributes={{ 'data-variant': id }}>
            {label}
          </Button>
        ))}
      </Row>
      <DemoEvent>{result.value}</DemoEvent>
      <ProjectSourceChoiceDialog open={open.value && !remote} />
      <DesktopRemoteDialog
        open={open.value && remote}
        servers={servers}
        formOpen={formOpen}
        discovery={variant.value === 'projects' ? discovery : undefined}
        pairing={variant.value === 'pairing' ? pairing : undefined}
        busy={false}
        error={variant.value === 'error' ? 'Could not reach or authenticate the approved remote server.' : ''}
      />
    </CatalogExample>
  );
}

/**
 * Wire demo-only fixture effects; production actions report what they would do.
 *
 * @returns A teardown that removes every listener this call installed.
 */
export function wireDesktopRemoteDialogDemo(root: HTMLElement): () => void {
  const scope = '[data-desktop-remote-dialog-demo]';
  const listeners = createScope();
  const show = (next: DesktopRemoteDemoVariant) => {
    variant.value = next;
    open.value = true;
  };
  listeners.add(
    delegate(root, 'click', `${scope} [data-action="show-desktop-remote-dialog-demo"]`, (_event, target) => {
      const next = (target as HTMLElement).dataset.variant;
      if (next && next in variants) show(next as DesktopRemoteDemoVariant);
    }),
  );
  listeners.add(
    delegate(root, 'click', `${scope} [data-action="choose-local-project"]`, () => {
      result.value = 'Production opens the local project folder dialog.';
      open.value = false;
    }),
  );
  listeners.add(
    delegate(root, 'click', `${scope} [data-action="choose-desktop-remote-project"]`, () => {
      show('servers');
    }),
  );
  listeners.add(
    delegate(root, 'click', `${scope} [data-action="show-desktop-remote-form"]`, () => {
      show('form');
    }),
  );
  listeners.add(
    delegate(root, 'click', `${scope} [data-action="cancel-desktop-remote-form"]`, () => {
      show('servers');
    }),
  );
  listeners.add(
    delegate(root, 'submit', `${scope} [data-action="begin-desktop-remote-pair"]`, (event) => {
      event.preventDefault();
      show('pairing');
    }),
  );
  listeners.add(
    delegate(root, 'click', `${scope} [data-action="finish-desktop-remote-pair"]`, () => {
      show('projects');
    }),
  );
  listeners.add(
    delegate(root, 'click', `${scope} [data-action="discover-desktop-remote-server"]`, () => {
      show('projects');
    }),
  );
  listeners.add(
    delegate(root, 'click', `${scope} [data-action="open-desktop-remote-checkout"]`, (_event, target) => {
      result.value = `Opened ${target.closest<HTMLElement>('[data-checkout-id]')?.dataset.checkoutId ?? ''}`;
      open.value = false;
    }),
  );
  listeners.add(
    delegate(
      root,
      'click',
      `${scope} [data-action="edit-desktop-remote-server"], ${scope} [data-action="remove-desktop-remote-server"]`,
      (_event, target) => {
        const id = target.closest<HTMLElement>('[data-remote-id]')?.dataset.remoteId ?? '';
        result.value = `${(target as HTMLElement).dataset.action === 'edit-desktop-remote-server' ? 'Edit' : 'Remove'} ${id}`;
      },
    ),
  );
  listeners.add(
    delegate(
      root,
      'click',
      `${scope} [data-action="cancel-project-source-choice"], ${scope} [data-action="cancel-desktop-remote-dialog"]`,
      () => {
        open.value = false;
      },
    ),
  );
  listeners.add(
    delegate(root, 'kui-request-close', `${scope} dialog.kui-dialog-surface`, () => {
      open.value = false;
    }),
  );
  return () => {
    listeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-BT4KN6). */
export const demoModule: DemoModule = {
  entries: {
    'project-source-choice-dialog': {
      catalogExamples: true,
      Demo: DesktopRemoteDialogDemo,
      reset: resetDesktopRemoteDialogDemo,
    },
    'desktop-remote-dialog': {
      catalogExamples: true,
      Demo: DesktopRemoteDialogDemo,
      reset: resetDesktopRemoteDialogDemo,
    },
  },
  wire: wireDesktopRemoteDialogDemo,
};
